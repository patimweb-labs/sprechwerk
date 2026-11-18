//! Mitschrift des Protokolls für die Verwaltungsoberfläche.
//!
//! Der Server protokolliert über `tracing` nach stdout (Container-Regel,
//! ADR 0007). Die Verwaltung im Browser soll **dieselben** Zeilen zeigen — ohne
//! zweite Protokollierung, die irgendwann etwas anderes sagt. Deshalb schreibt
//! `tracing` in einen [`Mitschreiber`]: der gibt jede Zeile unverändert an
//! stdout weiter und behält die letzten [`ZEILEN`] zusätzlich im Speicher.
//!
//! Jede Zeile bekommt eine fortlaufende Nummer. Die Oberfläche fragt „alles ab
//! Nummer N" und bekommt nur, was neu ist — auch wenn vorne schon Zeilen aus
//! dem Puffer gefallen sind (dann beginnt die Antwort bei der ältesten, die es
//! noch gibt).

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// So viele Zeilen behält der Speicher. Eine Zeile ist ~150 Byte, das sind
/// unter einem Megabyte — und reicht für Stunden normalen Betriebs.
pub const ZEILEN: usize = 5_000;

/// Eine Zeile mit ihrer laufenden Nummer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zeile {
    pub nr: u64,
    pub text: String,
}

/// Antwort auf „was ist neu ab Nummer N".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auszug {
    pub zeilen: Vec<Zeile>,
    /// Damit fragt man beim nächsten Mal.
    pub weiter_ab: u64,
}

#[derive(Debug, Default)]
struct Speicher {
    zeilen: VecDeque<Zeile>,
    naechste: u64,
}

/// Der gemeinsame Speicher. Billig zu klonen (ein `Arc`).
#[derive(Debug, Clone, Default)]
pub struct Mitschrift {
    speicher: Arc<Mutex<Speicher>>,
    grenze: usize,
}

impl Mitschrift {
    pub fn neu() -> Self {
        Self::mit_grenze(ZEILEN)
    }

    fn mit_grenze(grenze: usize) -> Self {
        Self {
            speicher: Arc::default(),
            grenze: grenze.max(1),
        }
    }

    /// Nimmt Text auf, der eine oder mehrere ganze Zeilen enthält.
    pub fn aufnehmen(&self, text: &str) {
        let Ok(mut s) = self.speicher.lock() else {
            return;
        };
        for zeile in text.lines() {
            let zeile = ohne_farben(zeile);
            if zeile.trim().is_empty() {
                continue;
            }
            let nr = s.naechste;
            s.naechste += 1;
            s.zeilen.push_back(Zeile { nr, text: zeile });
            while s.zeilen.len() > self.grenze {
                s.zeilen.pop_front();
            }
        }
    }

    /// Alle Zeilen ab Nummer `ab` (einschließlich).
    pub fn ab(&self, ab: u64) -> Auszug {
        let Ok(s) = self.speicher.lock() else {
            return Auszug {
                zeilen: Vec::new(),
                weiter_ab: ab,
            };
        };
        Auszug {
            zeilen: s.zeilen.iter().filter(|z| z.nr >= ab).cloned().collect(),
            weiter_ab: s.naechste,
        }
    }

    /// Schreiber für `tracing_subscriber::fmt().with_writer(…)`.
    pub fn schreiber(&self) -> Mitschreiber {
        Mitschreiber(self.clone())
    }
}

/// Entfernt ANSI-Farbfolgen (`ESC [ … m`). `tracing` färbt die Ausgabe für das
/// Terminal; in der Oberfläche wären das Zeichensalat.
fn ohne_farben(zeile: &str) -> String {
    let mut aus = String::with_capacity(zeile.len());
    let mut zeichen = zeile.chars().peekable();
    while let Some(c) = zeichen.next() {
        if c == '\u{1b}' && zeichen.peek() == Some(&'[') {
            zeichen.next();
            // Parameter und Zwischenzeichen bis zum Endbuchstaben überspringen.
            for weiter in zeichen.by_ref() {
                if weiter.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        aus.push(c);
    }
    aus
}

/// Erzeugt je Ereignis einen [`Ereignisschreiber`].
#[derive(Clone)]
pub struct Mitschreiber(Mitschrift);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Mitschreiber {
    type Writer = Ereignisschreiber;

    fn make_writer(&'a self) -> Self::Writer {
        Ereignisschreiber {
            puffer: Vec::new(),
            mitschrift: self.0.clone(),
        }
    }
}

/// Sammelt **ein** Ereignis und gibt es beim Fallenlassen weiter — an stdout
/// und an die Mitschrift. So landet nie eine halbe Zeile im Speicher.
pub struct Ereignisschreiber {
    puffer: Vec<u8>,
    mitschrift: Mitschrift,
}

impl Write for Ereignisschreiber {
    fn write(&mut self, daten: &[u8]) -> std::io::Result<usize> {
        self.puffer.extend_from_slice(daten);
        Ok(daten.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Ereignisschreiber {
    fn drop(&mut self) {
        if self.puffer.is_empty() {
            return;
        }
        let mut aus = std::io::stdout().lock();
        let _ = aus.write_all(&self.puffer);
        let _ = aus.flush();
        self.mitschrift
            .aufnehmen(&String::from_utf8_lossy(&self.puffer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neue_zeilen_ab_einer_nummer() {
        let m = Mitschrift::neu();
        m.aufnehmen("eins\nzwei\n");
        let alles = m.ab(0);
        assert_eq!(alles.zeilen.len(), 2);
        assert_eq!(alles.weiter_ab, 2);
        m.aufnehmen("drei\n");
        let neu = m.ab(alles.weiter_ab);
        assert_eq!(
            neu.zeilen,
            vec![Zeile {
                nr: 2,
                text: "drei".into()
            }]
        );
        assert_eq!(m.ab(neu.weiter_ab).zeilen, vec![]);
    }

    #[test]
    fn alte_zeilen_fallen_heraus_die_nummern_bleiben() {
        let m = Mitschrift::mit_grenze(2);
        m.aufnehmen("a\nb\nc\n");
        let a = m.ab(0);
        assert_eq!(
            a.zeilen.iter().map(|z| z.nr).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(a.weiter_ab, 3);
    }

    #[test]
    fn farben_und_leerzeilen_verschwinden() {
        let m = Mitschrift::neu();
        m.aufnehmen("\u{1b}[2m2026-11-17\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m lauscht\n\n");
        assert_eq!(m.ab(0).zeilen[0].text, "2026-11-17  INFO lauscht");
        assert_eq!(m.ab(0).zeilen.len(), 1);
    }

    #[test]
    fn tracing_schreibt_in_die_mitschrift() {
        let m = Mitschrift::neu();
        let abonnent = tracing_subscriber::fmt()
            .with_writer(m.schreiber())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(abonnent, || {
            tracing::info!(port = 7443, "lauscht");
        });
        let zeilen = m.ab(0).zeilen;
        assert_eq!(zeilen.len(), 1);
        assert!(zeilen[0].text.contains("lauscht"), "{zeilen:?}");
        assert!(zeilen[0].text.contains("port=7443"), "{zeilen:?}");
    }
}

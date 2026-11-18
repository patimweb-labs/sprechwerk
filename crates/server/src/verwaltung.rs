// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Verwaltung durch den Betreiber (ADR 0013, ersetzt ADR 0011): Stand,
//! Mitglieder, Rollen, Protokoll — ohne Browser.
//!
//! Zwei Wege zum selben [`Verwaltung`]-Wert:
//! * **Server-Fenster** (`crates/server-fenster`): ruft die Methoden direkt auf.
//! * **Konsole** (`sprechwerk stand | rolle … | protokoll`): spricht den
//!   laufenden Prozess über den [Steuerkanal](bedienen) an — 127.0.0.1, eine
//!   JSON-Zeile hin, eine zurück.
//!
//! **Zugang:** der Steuerkanal verlangt den Schlüssel aus
//! `<daten>/verwaltung.schluessel`. Lesen kann ihn, wer das Datenverzeichnis
//! lesen kann — also der Betreiber, im Container per `docker exec`. Kein Code
//! im Protokoll, kein Keks, kein Port nach außen.

use crate::kern::{jetzt_ms, Kern};
use crate::mitschrift::{Auszug, Mitschrift};
use serde::{Deserialize, Serialize};
use sprechwerk_domain::{MemberId, Role};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Name der Schlüsseldatei im Datenverzeichnis.
pub const SCHLUESSELDATEI: &str = "verwaltung.schluessel";

/// Höchstlänge einer Anfrage — mehr braucht kein Auftrag.
const ANFRAGE_GRENZE: usize = 4096;

#[derive(Clone)]
pub struct Verwaltung {
    pub kern: Arc<Mutex<Kern>>,
    pub mitschrift: Mitschrift,
    /// SHA-256 des Zertifikats (zum Weitergeben an die Mitglieder).
    pub fingerabdruck: Arc<str>,
    /// Start des Prozesses (ms seit der Epoche).
    pub gestartet: i64,
}

/// Alles, was die Übersicht zeigt. Ein Abruf, ein Bild (AGENTS.md §2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stand {
    pub name: String,
    pub version: String,
    /// Wohin die Mitglieder sich verbinden (UDP).
    pub adresse: String,
    pub port: u16,
    pub fingerabdruck: String,
    pub gestartet: i64,
    pub jetzt: i64,
    pub raeume: usize,
    pub verbundene: usize,
    /// Noch niemand da: der Erste, der sich verbindet, wird Administrator.
    pub erster_wird_administrator: bool,
    pub sicherung: Sicherungsstand,
    pub mitglieder: Vec<Mitgliedszeile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sicherungsstand {
    pub aktiv: bool,
    pub verzeichnis: String,
    /// Zeitpunkt der jüngsten Datei im Sicherungsverzeichnis.
    pub letzte: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mitgliedszeile {
    pub id: MemberId,
    pub name: String,
    pub kennung: String,
    pub rolle: Role,
    pub verbunden: bool,
    pub raum: Option<String>,
    pub zuletzt_gesehen: i64,
}

/// Warum eine Rolle nicht gesetzt wurde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Rollenfehler {
    #[error("{name} ist der letzte Administrator und bleibt es — ernenne zuerst einen zweiten")]
    LetzterAdministrator { name: String },
    #[error("niemand passt zu „{wer}“")]
    Unbekannt { wer: String },
    #[error("„{wer}“ passt auf mehrere: {namen} — nimm die Kennung")]
    Mehrdeutig { wer: String, namen: String },
    #[error("{0}")]
    Anders(String),
}

fn letzte_sicherung(verzeichnis: &Path) -> Option<i64> {
    std::fs::read_dir(verzeichnis)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

impl Verwaltung {
    /// Der ganze Stand für Übersicht und Konsole.
    pub fn stand(&self) -> Stand {
        let (mut stand, verzeichnis) = {
            let k = self.kern.lock().expect("Sperre");
            let raumname = |id| {
                k.state
                    .raumliste()
                    .into_iter()
                    .find(|r| r.id == id)
                    .map(|r| r.name.clone())
            };
            let mut mitglieder: Vec<Mitgliedszeile> = k
                .state
                .mitglieder
                .values()
                .map(|m| {
                    let verbunden = k.teilnehmer.contains_key(&m.id);
                    Mitgliedszeile {
                        id: m.id,
                        name: m.name.clone(),
                        kennung: m.schluessel.clone(),
                        rolle: m.rolle,
                        verbunden,
                        raum: if verbunden {
                            m.raum.and_then(raumname)
                        } else {
                            None
                        },
                        zuletzt_gesehen: m.zuletzt_gesehen,
                    }
                })
                .collect();
            // Verbundene zuerst, dann nach Name — so steht man selbst oben,
            // sobald man mit der App verbunden ist.
            mitglieder.sort_by(|a, b| {
                b.verbunden
                    .cmp(&a.verbunden)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            let stand = Stand {
                name: k.state.server_name.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                adresse: k.konfig.adresse(),
                port: k.konfig.server.port,
                fingerabdruck: self.fingerabdruck.to_string(),
                gestartet: self.gestartet,
                jetzt: jetzt_ms(),
                raeume: k.state.raeume.len(),
                verbundene: k.teilnehmer.len(),
                erster_wird_administrator: k.state.mitglieder.is_empty(),
                sicherung: Sicherungsstand {
                    aktiv: k.konfig.sicherung.aktiv,
                    verzeichnis: k.konfig.sicherung.verzeichnis.display().to_string(),
                    letzte: None,
                },
                mitglieder,
            };
            (stand, k.konfig.sicherung.verzeichnis.clone())
        };
        // Dateisystem erst nach der Sperre: der Sprachpfad wartet sonst mit.
        stand.sicherung.letzte = letzte_sicherung(&verzeichnis);
        stand
    }

    /// Protokollzeilen ab Nummer `ab`.
    pub fn protokoll(&self, ab: u64) -> Auszug {
        self.mitschrift.ab(ab)
    }

    /// Setzt eine Rolle. `wer` ist die interne Kennung (`mitglied_…`), der
    /// Anfang des öffentlichen Schlüssels (mindestens 8 Zeichen) oder der
    /// Name — der Name nur, wenn er eindeutig ist.
    pub fn rolle_setzen(&self, wer: &str, rolle: Role) -> Result<Stand, Rollenfehler> {
        let ziel = self.finden(wer)?;
        let ergebnis = self
            .kern
            .lock()
            .expect("Sperre")
            .betreiber_rolle_setzen(ziel.0, rolle);
        match ergebnis {
            Ok(()) => Ok(self.stand()),
            Err(sprechwerk_domain::Error::LetzterAdministrator) => {
                Err(Rollenfehler::LetzterAdministrator { name: ziel.1 })
            }
            Err(sprechwerk_domain::Error::MitgliedUnbekannt) => Err(Rollenfehler::Unbekannt {
                wer: wer.to_string(),
            }),
            Err(andere) => Err(Rollenfehler::Anders(andere.to_string())),
        }
    }

    fn finden(&self, wer: &str) -> Result<(MemberId, String), Rollenfehler> {
        let wer = wer.trim();
        let k = self.kern.lock().expect("Sperre");
        if let Ok(id) = MemberId::from_text(wer) {
            if let Some(m) = k.state.mitglied(id) {
                return Ok((m.id, m.name.clone()));
            }
        }
        let klein = wer.to_lowercase();
        let schluessel_passt = wer.len() >= 8 && wer.chars().all(|c| c.is_ascii_hexdigit());
        let treffer: Vec<(MemberId, String)> = k
            .state
            .mitglieder
            .values()
            .filter(|m| {
                m.name.to_lowercase() == klein
                    || (schluessel_passt && m.schluessel.to_lowercase().starts_with(&klein))
            })
            .map(|m| (m.id, m.name.clone()))
            .collect();
        match treffer.len() {
            0 => Err(Rollenfehler::Unbekannt {
                wer: wer.to_string(),
            }),
            1 => Ok(treffer.into_iter().next().expect("ein Treffer")),
            _ => Err(Rollenfehler::Mehrdeutig {
                wer: wer.to_string(),
                namen: treffer
                    .iter()
                    .map(|t| t.1.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }
}

// ---- Steuerkanal ---------------------------------------------------------

/// Was die Konsole will.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "auftrag", rename_all = "snake_case")]
pub enum Auftrag {
    Stand,
    Protokoll {
        #[serde(default)]
        ab: u64,
    },
    Rolle {
        wer: String,
        rolle: Role,
    },
}

/// Was der Server antwortet.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "antwort", rename_all = "snake_case")]
pub enum Antwort {
    Stand {
        stand: Stand,
    },
    Protokoll {
        auszug: Auszug,
    },
    Rollenfehler {
        fehler: Rollenfehler,
    },
    /// Falscher Schlüssel oder unlesbare Anfrage.
    Abgelehnt {
        grund: String,
    },
}

#[derive(Serialize, Deserialize)]
struct Anfrage {
    schluessel: String,
    #[serde(flatten)]
    auftrag: Auftrag,
}

impl Verwaltung {
    /// Ein Auftrag, wie ihn der Steuerkanal ausführt.
    pub fn ausfuehren(&self, auftrag: Auftrag) -> Antwort {
        match auftrag {
            Auftrag::Stand => Antwort::Stand {
                stand: self.stand(),
            },
            Auftrag::Protokoll { ab } => Antwort::Protokoll {
                auszug: self.protokoll(ab),
            },
            Auftrag::Rolle { wer, rolle } => match self.rolle_setzen(&wer, rolle) {
                Ok(stand) => Antwort::Stand { stand },
                Err(fehler) => Antwort::Rollenfehler { fehler },
            },
        }
    }
}

/// Schreibt einen neuen Schlüssel in `<daten>/verwaltung.schluessel` (nur für
/// den Eigentümer lesbar) und gibt ihn zurück. Jeder Start ein neuer.
pub fn schluessel_schreiben(daten: &Path) -> std::io::Result<String> {
    let schluessel = crate::netz::zufall_hex(32);
    let pfad = daten.join(SCHLUESSELDATEI);
    let _ = std::fs::remove_file(&pfad);
    let mut optionen = std::fs::OpenOptions::new();
    optionen.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        optionen.mode(0o600);
    }
    use std::io::Write;
    optionen.open(&pfad)?.write_all(schluessel.as_bytes())?;
    Ok(schluessel)
}

/// Pfad der Schlüsseldatei.
pub fn schluesseldatei(daten: &Path) -> PathBuf {
    daten.join(SCHLUESSELDATEI)
}

/// Vergleich in gleichbleibender Zeit — die Antwortzeit verrät nicht, wie
/// viele Zeichen am Anfang stimmen.
fn gleich(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Bedient den Steuerkanal auf `127.0.0.1:port`. Nur dieser Rechner (im
/// Container: nur der Container) — der Port wird nie nach außen gereicht.
pub async fn bedienen(v: Verwaltung, port: u16, schluessel: String) -> anyhow::Result<()> {
    let hoerer = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let schluessel: Arc<str> = schluessel.into();
    loop {
        let (strom, _) = hoerer.accept().await?;
        let v = v.clone();
        let schluessel = schluessel.clone();
        tokio::spawn(async move {
            let _ = antworten(strom, &v, &schluessel).await;
        });
    }
}

async fn antworten(
    strom: tokio::net::TcpStream,
    v: &Verwaltung,
    schluessel: &str,
) -> std::io::Result<()> {
    let (lesen, mut schreiben) = strom.into_split();
    let mut zeile = String::new();
    let mut leser = BufReader::new(lesen.take(ANFRAGE_GRENZE as u64));
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        leser.read_line(&mut zeile),
    )
    .await
    .map_err(|_| std::io::Error::other("Zeitüberschreitung"))??;
    let antwort = match serde_json::from_str::<Anfrage>(zeile.trim()) {
        Ok(a) if gleich(a.schluessel.as_bytes(), schluessel.as_bytes()) => {
            let v = v.clone();
            tokio::task::spawn_blocking(move || v.ausfuehren(a.auftrag))
                .await
                .unwrap_or(Antwort::Abgelehnt {
                    grund: "intern".into(),
                })
        }
        Ok(_) => {
            // Falscher Schlüssel: kurz warten, damit Durchprobieren sich
            // nicht lohnt, und im Protokoll vermerken.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            tracing::warn!("Steuerkanal: falscher Schlüssel");
            Antwort::Abgelehnt {
                grund: "falscher Schlüssel".into(),
            }
        }
        Err(fehler) => Antwort::Abgelehnt {
            grund: format!("unlesbare Anfrage: {fehler}"),
        },
    };
    let mut text = serde_json::to_string(&antwort).map_err(std::io::Error::other)?;
    text.push('\n');
    schreiben.write_all(text.as_bytes()).await?;
    schreiben.shutdown().await
}

/// Die Konsolenseite: einen Auftrag an den laufenden Server schicken.
pub fn fragen(daten: &Path, port: u16, auftrag: Auftrag) -> anyhow::Result<Antwort> {
    use std::io::{BufRead, Write};
    let pfad = schluesseldatei(daten);
    let schluessel = std::fs::read_to_string(&pfad).map_err(|f| {
        anyhow::anyhow!(
            "{} nicht lesbar ({f}) — läuft der Server, und ist das sein Datenverzeichnis?",
            pfad.display()
        )
    })?;
    let mut strom = std::net::TcpStream::connect(("127.0.0.1", port))
        .map_err(|f| anyhow::anyhow!("Server auf 127.0.0.1:{port} nicht erreichbar: {f}"))?;
    strom.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let mut text = serde_json::to_string(&Anfrage {
        schluessel: schluessel.trim().to_string(),
        auftrag,
    })?;
    text.push('\n');
    strom.write_all(text.as_bytes())?;
    let mut zeile = String::new();
    std::io::BufReader::new(strom).read_line(&mut zeile)?;
    Ok(serde_json::from_str(zeile.trim())?)
}

/// Stand als Text für die Konsole.
pub fn stand_als_text(s: &Stand) -> String {
    use std::fmt::Write;
    let mut aus = String::new();
    let _ = writeln!(aus, "{} · Version {}", s.name, s.version);
    let _ = writeln!(aus, "Adresse       {} (UDP)", s.adresse);
    let _ = writeln!(aus, "Fingerabdruck {}", gruppiert(&s.fingerabdruck));
    let _ = writeln!(
        aus,
        "Läuft seit    {} min · {} Räume · {} verbunden",
        (s.jetzt - s.gestartet).max(0) / 60_000,
        s.raeume,
        s.verbundene
    );
    let sicherung = match (s.sicherung.aktiv, s.sicherung.letzte) {
        (false, _) => "aus".to_string(),
        (true, None) => "noch keine".to_string(),
        (true, Some(t)) => format!("vor {} min", (s.jetzt - t).max(0) / 60_000),
    };
    let _ = writeln!(
        aus,
        "Sicherung     {sicherung} ({})",
        s.sicherung.verzeichnis
    );
    if s.erster_wird_administrator {
        let _ = writeln!(
            aus,
            "\nNoch niemand da: wer sich zuerst mit der App verbindet, wird Administrator."
        );
    }
    if !s.mitglieder.is_empty() {
        let _ = writeln!(
            aus,
            "\n{:<24} {:<14} {:<10} Kennung",
            "Name", "Rolle", "Status"
        );
        for m in &s.mitglieder {
            let status = match (&m.verbunden, &m.raum) {
                (true, Some(r)) => format!("in {r}"),
                (true, None) => "verbunden".into(),
                (false, _) => "—".into(),
            };
            let _ = writeln!(
                aus,
                "{:<24} {:<14} {:<10} {}",
                m.name,
                m.rolle.text(),
                status,
                m.kennung.get(..16).unwrap_or(&m.kennung)
            );
        }
    }
    aus
}

fn gruppiert(hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::rollenmatrix::{schluessel, verzeichnis};
    use crate::konfig::{Konfig, Server, Sicherung};

    struct Aufbau {
        verwaltung: Verwaltung,
        ada: MemberId,
        daten: PathBuf,
    }

    fn aufbau(name: &str) -> Aufbau {
        let daten = verzeichnis(&format!("verwaltung-{name}"));
        let konfig = Konfig {
            server: Server {
                daten: daten.clone(),
                ..Default::default()
            },
            sicherung: Sicherung {
                aktiv: false,
                ..Default::default()
            },
            verwaltung: Default::default(),
        };
        let mut kern = Kern::neu(konfig).unwrap();
        let (ada, _) = kern.anmelden(&schluessel(1), "Ada", "10.0.0.1").unwrap();
        kern.anmelden(&schluessel(2), "Bosse", "10.0.0.2").unwrap();
        let mitschrift = Mitschrift::neu();
        mitschrift.aufnehmen("sprechwerk startet\nlauscht\n");
        Aufbau {
            verwaltung: Verwaltung {
                kern: Arc::new(Mutex::new(kern)),
                mitschrift,
                fingerabdruck: "ab".repeat(32).into(),
                gestartet: 1,
            },
            ada,
            daten,
        }
    }

    #[test]
    fn stand_zeigt_mitglieder_und_ersten_administrator() {
        let a = aufbau("stand");
        let s = a.verwaltung.stand();
        assert_eq!(s.mitglieder.len(), 2);
        assert!(!s.erster_wird_administrator);
        let ada = s.mitglieder.iter().find(|m| m.id == a.ada).unwrap();
        assert_eq!(
            ada.rolle,
            Role::Administrator,
            "der Erste wird Administrator"
        );
        assert!(stand_als_text(&s).contains("Bosse"));
    }

    #[test]
    fn rolle_nach_name_und_kennung() {
        let a = aufbau("rolle");
        let s = a.verwaltung.rolle_setzen("bosse", Role::Moderator).unwrap();
        let bosse = s.mitglieder.iter().find(|m| m.name == "Bosse").unwrap();
        assert_eq!(bosse.rolle, Role::Moderator);
        let anfang = bosse.kennung[..10].to_string();
        let s = a.verwaltung.rolle_setzen(&anfang, Role::Gast).unwrap();
        assert_eq!(
            s.mitglieder
                .iter()
                .find(|m| m.name == "Bosse")
                .unwrap()
                .rolle,
            Role::Gast
        );
    }

    #[test]
    fn letzter_administrator_bleibt() {
        let a = aufbau("letzter");
        assert!(matches!(
            a.verwaltung.rolle_setzen("Ada", Role::Mitglied),
            Err(Rollenfehler::LetzterAdministrator { .. })
        ));
        assert!(matches!(
            a.verwaltung.rolle_setzen("niemand", Role::Mitglied),
            Err(Rollenfehler::Unbekannt { .. })
        ));
    }

    #[tokio::test]
    async fn steuerkanal_mit_und_ohne_schluessel() {
        let a = aufbau("kanal");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let schluessel = schluessel_schreiben(&a.daten).unwrap();
        tokio::spawn(bedienen(a.verwaltung.clone(), port, schluessel));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let daten = a.daten.clone();
        let antwort = tokio::task::spawn_blocking(move || fragen(&daten, port, Auftrag::Stand))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(antwort, Antwort::Stand { ref stand } if stand.mitglieder.len() == 2));

        let daten = a.daten.clone();
        let antwort =
            tokio::task::spawn_blocking(move || fragen(&daten, port, Auftrag::Protokoll { ab: 0 }))
                .await
                .unwrap()
                .unwrap();
        assert!(
            matches!(antwort, Antwort::Protokoll { ref auszug } if auszug.zeilen.len() == 2),
            "{antwort:?}"
        );

        // Falscher Schlüssel: abgelehnt.
        std::fs::write(schluesseldatei(&a.daten), "falsch").unwrap();
        let daten = a.daten.clone();
        let antwort = tokio::task::spawn_blocking(move || fragen(&daten, port, Auftrag::Stand))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(antwort, Antwort::Abgelehnt { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn schluesseldatei_nur_fuer_den_eigentuemer() {
        use std::os::unix::fs::PermissionsExt;
        let d = verzeichnis("verwaltung-rechte");
        std::fs::create_dir_all(&d).unwrap();
        schluessel_schreiben(&d).unwrap();
        let modus = std::fs::metadata(schluesseldatei(&d))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(modus & 0o777, 0o600);
    }
}

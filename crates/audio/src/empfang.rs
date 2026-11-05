//! Die Empfangsseite: Datagramme rein, hörbarer Ton raus.
//!
//! **Je Sprecher ein eigener Dekodierer und ein eigener Jitter-Puffer.** Das ist
//! der Grund, warum die Sprecher-Nummer im Sprachkopf steht: zwei Stimmen durch
//! einen Dekodierer zu schicken klingt wie Rauschen, weil Opus seinen Zustand
//! über die Rahmen hinweg fortschreibt.
//!
//! Der Jitter-Puffer (`sprechwerk_adapters::sprechpfad`) hält 60 ms, sortiert
//! und verwirft Verspätetes. Was fehlt, wird **verdeckt** statt mit Stille
//! gefüllt: Stille knackt, Verdeckung klingt wie ein kurzer Aussetzer.

use crate::opus::Dekodierer;
use crate::pcm::{dazu_mischen, RAHMEN_SAMPLES};
use crate::verlust::Wiederhersteller;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Ein Sprecher im Empfang.
struct Sprecher {
    dekodierer: Dekodierer,
    /// Sortiert, füllt Lücken aus Paritätsrahmen und hält eine Gruppe lang
    /// zurück — das ist zugleich der Jitter-Puffer.
    hersteller: Wiederhersteller,
    /// Fertige Rahmen in Reihenfolge, warten auf die Wiedergabe.
    schlange: VecDeque<Vec<u8>>,
    /// Läuft dieser Sprecher gerade (für Verdeckung statt Stille)?
    laeuft: bool,
    /// Wann zuletzt etwas kam (für das Aufräumen alter Sprecher).
    zuletzt: Instant,
    /// Wie oft ein Rahmen fehlte.
    verluste: u64,
}

/// Empfängt Sprache von mehreren Sprechern und mischt sie.
pub struct Empfänger {
    sprecher: HashMap<u16, Sprecher>,
    /// Mischpuffer für den nächsten Ausgabeblock.
    misch: Vec<i16>,
    /// Verworfene Datagramme (falscher Kopf, alte Version, unbekannt).
    verworfen: u64,
    /// Angenommene Rahmen.
    angenommen: u64,
}

impl Default for Empfänger {
    fn default() -> Self {
        Self::neu()
    }
}

impl Empfänger {
    pub fn neu() -> Self {
        Self {
            sprecher: HashMap::new(),
            misch: vec![0i16; RAHMEN_SAMPLES],
            verworfen: 0,
            angenommen: 0,
        }
    }

    /// Wie viele Sprecher gerade bekannt sind.
    pub fn anzahl_sprecher(&self) -> usize {
        self.sprecher.len()
    }

    /// Zähler: (angenommen, verworfen, Sprecher).
    pub fn zaehler(&self) -> (u64, u64, usize) {
        (self.angenommen, self.verworfen, self.sprecher.len())
    }

    /// Nimmt ein Sprach-Datagramm auf.
    ///
    /// Liefert `true`, wenn es angenommen wurde. Unbekannte Versionen und
    /// kaputte Köpfe werden verworfen, nicht als Fehler behandelt: ein
    /// fehlerhaftes Datagramm darf den Ton nicht anhalten.
    pub fn nimm(&mut self, datagramm: &[u8]) -> bool {
        // Sprecher-Nummer steht im Kopf; ohne sie kein Empfang.
        let Some((kopf, _)) = sprechwerk_protocol::Sprachkopf::lesen(datagramm) else {
            self.verworfen += 1;
            return false;
        };
        if kopf.version != sprechwerk_protocol::SPRACHKOPF_VERSION {
            self.verworfen += 1;
            return false;
        }
        let von = kopf.von;
        if !self.sprecher.contains_key(&von) {
            let Ok(dekodierer) = Dekodierer::neu() else {
                self.verworfen += 1;
                return false;
            };
            self.sprecher.insert(
                von,
                Sprecher {
                    dekodierer,
                    hersteller: Wiederhersteller::neu(),
                    schlange: VecDeque::new(),
                    laeuft: false,
                    zuletzt: Instant::now(),
                    verluste: 0,
                },
            );
        }
        let eintrag = self.sprecher.get_mut(&von).expect("gerade angelegt");
        eintrag.zuletzt = Instant::now();
        // Doppelte und zu späte Rahmen: nicht zweimal spielen.
        if let crate::verlust::Empfangen::Sprache { folge, .. } = crate::verlust::lese(datagramm) {
            if eintrag.hersteller.zu_alt(folge) {
                self.verworfen += 1;
                return false;
            }
        }
        let fertig = eintrag.hersteller.nimm(datagramm);
        let angenommen = !fertig.is_empty() || crate::verlust::lese(datagramm) != crate::verlust::Empfangen::Verworfen;
        for (_, daten) in fertig {
            eintrag.schlange.push_back(daten);
        }
        if angenommen {
            self.angenommen += 1;
        } else {
            self.verworfen += 1;
        }
        angenommen
    }

    /// Holt den nächsten hörbaren Block (960 Werte).
    ///
    /// Wird im 20-ms-Takt aufgerufen. Ergebnis ist immer gefüllt; wo niemand
    /// spricht, steht Stille.
    pub fn naechster_block(&mut self) -> Vec<i16> {
        self.misch.iter_mut().for_each(|w| *w = 0);

        let mut pcm = vec![0i16; RAHMEN_SAMPLES];
        for sprecher in self.sprecher.values_mut() {
            match sprecher.schlange.pop_front() {
                Some(rahmen) => {
                    sprecher.laeuft = true;
                    match sprecher.dekodierer.dekodiere(&rahmen, &mut pcm) {
                        Ok(len) => dazu_mischen(&mut self.misch, &pcm[..len.min(RAHMEN_SAMPLES)]),
                        Err(_) => sprecher.verluste += 1,
                    }
                }
                None if sprecher.laeuft => {
                    // Lücke: verdecken statt Stille — Stille knackt hörbar,
                    // Verdeckung klingt wie ein kurzer Aussetzer.
                    if sprecher.dekodierer.verschollen(&mut pcm).is_ok() {
                        dazu_mischen(&mut self.misch, &pcm[..RAHMEN_SAMPLES]);
                    }
                    sprecher.verluste += 1;
                    // Nach 10 leeren Blöcken ist der Sprecher weg.
                    if sprecher.verluste > 10 && sprecher.schlange.is_empty() {
                        sprecher.laeuft = false;
                    }
                }
                None => {}
            }
        }

        // Sprecher aufräumen, die 30 s nichts geschickt haben.
        let grenze = Duration::from_secs(30);
        self.sprecher
            .retain(|_, s| s.zuletzt.elapsed() < grenze);

        self.misch.clone()
    }

    /// Setzt alles zurück (Raumwechsel, neue Verbindung).
    pub fn leeren(&mut self) {
        self.sprecher.clear();
    }

    /// Wie viele Sprecher warten gerade auf Rahmen?
    pub fn wartende_rahmen(&self) -> usize {
        self.sprecher.values().map(|s| s.schlange.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_protocol::Sprachkopf;
    use crate::opus::{Kodierer, RAHMEN_SAMPLES as OPUS_SAMPLES};

    /// Eine ganze Paritätsgruppe: vier Rahmen plus Parität.
    fn gruppe(basis: u16, von: u16, pcm: &[i16]) -> Vec<Vec<u8>> {
        use crate::verlust::{paritaets_datagramm, Paritaet, GRUPPE};
        let mut k = Kodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        let mut rahmen = Vec::new();
        for i in 0..GRUPPE {
            let len = k.kodiere(pcm, &mut puffer).unwrap();
            rahmen.push(puffer[..len].to_vec());
            let _ = i;
        }
        let laengen: Vec<u16> = rahmen.iter().map(|r| r.len() as u16).collect();
        let summe = Paritaet::summe(&rahmen);
        let mut raus: Vec<Vec<u8>> = rahmen
            .iter()
            .enumerate()
            .map(|(i, daten)| Sprachkopf::neu(basis + i as u16).mit_von(von).schreiben(daten))
            .collect();
        let mut paritaet = paritaets_datagramm(basis, &laengen, &summe);
        // Der Server stempelt die Sprecher-Nummer in jeden Rahmen — hier im
        // Test muss sie von Hand hinein.
        paritaet[4..6].copy_from_slice(&von.to_be_bytes());
        raus.push(paritaet);
        raus
    }

    fn sinus(amplitude: f32) -> Vec<i16> {
        (0..OPUS_SAMPLES)
            .map(|i| {
                let w = amplitude
                    * (2.0 * std::f32::consts::PI * 500.0 * i as f32 / 48_000.0).sin();
                (w * i16::MAX as f32) as i16
            })
            .collect()
    }

    fn energie(block: &[i16]) -> f32 {
        (block.iter().map(|s| (*s as f32).powi(2)).sum::<f32>() / block.len() as f32).sqrt()
    }

    #[test]
    fn unbekannter_kopf_wird_verworfen_ohne_absturz() {
        let mut e = Empfänger::neu();
        assert!(!e.nimm(&[]));
        assert!(!e.nimm(&[1, 2, 3]));
        assert!(!e.nimm(&[9, 0, 0, 1, 0, 0, 1, 2, 3]));
        assert_eq!(e.zaehler().1 >= 3, true);
        // Und der Ausgabeblock bleibt trotzdem gültig.
        assert_eq!(e.naechster_block().len(), RAHMEN_SAMPLES);
    }

    #[test]
    fn zwei_sprecher_werden_gemischt() {
        let mut e = Empfänger::neu();
        let ton = sinus(0.5);
        for d in gruppe(0, 1, &ton) {
            e.nimm(&d);
        }
        for d in gruppe(0, 2, &ton) {
            e.nimm(&d);
        }
        assert_eq!(e.anzahl_sprecher(), 2);
        // Vier Blöcke ausgeben; irgendwo muss Ton drin sein.
        let laut = (0..4)
            .map(|_| energie(&e.naechster_block()))
            .fold(0.0f32, f32::max);
        assert!(laut > 1000.0, "gemischter Block ist still: {laut}");
    }

    #[test]
    fn ein_sprecher_kommt_hoerbar_an() {
        let mut e = Empfänger::neu();
        let ton = sinus(0.7);
        for d in gruppe(0, 5, &ton) {
            e.nimm(&d);
        }
        let laut = (0..4)
            .map(|_| energie(&e.naechster_block()))
            .fold(0.0f32, f32::max);
        assert!(laut > 2000.0, "kaum Ton: {laut}");
    }

    #[test]
    fn doppelte_rahmen_kommen_nicht_in_die_warteschlange() {
        let mut e = Empfänger::neu();
        let ton = sinus(0.5);
        let g = gruppe(0, 1, &ton);
        for d in &g {
            e.nimm(d);
        }
        assert_eq!(e.wartende_rahmen(), 4, "vier Rahmen warten auf Wiedergabe");
        // Dieselbe Gruppe noch einmal: darf nichts nachlegen.
        for d in &g {
            e.nimm(d);
        }
        assert_eq!(
            e.wartende_rahmen(),
            4,
            "Doppelte dürfen die Warteschlange nicht füllen"
        );
        assert!(e.zaehler().1 >= 4, "Doppelte müssen als verworfen zählen");
    }

    #[test]
    fn luecke_wird_verdeckt_statt_verschluckt() {
        // Ein Rahmen fehlt, die Parität ersetzt ihn.
        let mut e = Empfänger::neu();
        let ton = sinus(0.6);
        let g = gruppe(0, 1, &ton);
        for (i, d) in g.iter().enumerate() {
            if i == 2 {
                continue; // Rahmen 2 geht verloren
            }
            e.nimm(d);
        }
        assert_eq!(
            e.wartende_rahmen(),
            4,
            "Parität muss den verlorenen Rahmen ersetzen"
        );
    }

    #[test]
    fn leeren_raeumt_auf() {
        let mut e = Empfänger::neu();
        e.nimm(&gruppe(0, 1, &sinus(0.5))[0]);
        assert_eq!(e.anzahl_sprecher(), 1);
        e.leeren();
        assert_eq!(e.anzahl_sprecher(), 0);
    }
}

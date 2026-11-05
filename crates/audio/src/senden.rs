//! Die Sendeseite: Block rein, Datagramme raus.
//!
//! Reihenfolge ist festgelegt (und begründet in `docs/AUDIO.md`):
//!
//! ```text
//! Block ──► Rauschunterdrückung ──► Noisegate ──► Opus ──► Vorlauf ──► Datagramm
//! ```
//!
//! Der **Vorlauf** ist der Ersatz für TeamSpeaks 300-ms-Wartezeit bei
//! Push-to-Talk: es wird immer mitkodiert, die letzten 60 ms liegen bereit, und
//! beim Tastendruck gehen sie **vor** dem Live-Ton hinaus. Verzögerung 0 ms,
//! kein abgeschnittener Wortanfang.
//!
//! Dieses Modul kennt kein Gerät und kein Netz — es nimmt Blöcke und gibt
//! fertige Datagramme zurück. Damit ist die ganze Kette prüfbar.

use crate::opus::{Kodierer, RAHMEN_SAMPLES};
use crate::verlust::{paritaets_datagramm, Paritaet, GRUPPE};
use anyhow::Result;
use sprechwerk_adapters::klang::{pegel_aus_rms, Entrauscher, Noisegate};
use sprechwerk_adapters::sprechpfad::Vorlauf;
use sprechwerk_protocol::Sprachkopf;

/// Einstellungen der Sendeseite (ändern sich zur Laufzeit über die Oberfläche).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sendeeinstellungen {
    /// Rauschunterdrückung an.
    pub entrauschen: bool,
    /// Noisegate an (unter der Schwelle wird nichts gesendet).
    pub noisegate: bool,
    /// Sprechschwelle 0–100.
    pub schwelle: u8,
    /// Vorlauf in Millisekunden (wird auf ganze Rahmen abgerundet).
    pub vorlauf_ms: u32,
    /// Paritätsrahmen senden (Verlustausgleich, +25 % Last).
    pub paritaet: bool,
}

impl Default for Sendeeinstellungen {
    fn default() -> Self {
        Self {
            entrauschen: true,
            noisegate: true,
            schwelle: 30,
            vorlauf_ms: 60,
            paritaet: true,
        }
    }
}

/// Verarbeitet Aufnahmeblöcke zu Sprach-Datagrammen.
pub struct Sender {
    entrauscher: Entrauscher,
    gate: Noisegate,
    kodierer: Kodierer,
    vorlauf: Vorlauf,
    einstellungen: Sendeeinstellungen,
    /// Folgenummer für den nächsten **gesendeten** Rahmen.
    folge: u16,
    /// Wurde im letzten Block gesendet? (für den Vorlauf beim Öffnen)
    sendete: bool,
    /// Zuletzt gemessener Pegel 0–100.
    pegel: u8,
    /// Fertige Datagramme (Zwischenspeicher, um Allokation zu sparen).
    puffer: Vec<u8>,
    /// Die Rahmen der laufenden Paritätsgruppe.
    gruppe: Vec<Vec<u8>>,
}

impl Sender {
    pub fn neu(einstellungen: Sendeeinstellungen) -> Result<Self> {
        Ok(Self {
            entrauscher: Entrauscher::neu(crate::pcm::ABTASTRATE),
            gate: Noisegate::neu(einstellungen.schwelle),
            kodierer: Kodierer::neu()?,
            vorlauf: Vorlauf::neu(einstellungen.vorlauf_ms),
            einstellungen,
            folge: 0,
            sendete: false,
            pegel: 0,
            puffer: vec![0u8; 4000],
            gruppe: Vec::with_capacity(GRUPPE as usize),
        })
    }

    /// Übernimmt neue Einstellungen (die Oberfläche darf jederzeit ändern).
    pub fn einstellen(&mut self, einstellungen: Sendeeinstellungen) {
        if einstellungen.schwelle != self.einstellungen.schwelle {
            self.gate = Noisegate::neu(einstellungen.schwelle);
        }
        if einstellungen.vorlauf_ms != self.einstellungen.vorlauf_ms {
            self.vorlauf = Vorlauf::neu(einstellungen.vorlauf_ms);
        }
        self.einstellungen = einstellungen;
    }

    /// Letzter Pegel 0–100 (für die Anzeige im Fenster).
    pub fn pegel(&self) -> u8 {
        self.pegel
    }

    /// Verarbeitet genau einen 20-ms-Block (960 Werte, -1.0 … 1.0).
    ///
    /// `senden_erlaubt` ist die Entscheidung von außen: Push-to-Talk gedrückt
    /// beziehungsweise offenes Mikrofon und nicht stummgeschaltet.
    ///
    /// Rückgabe: die Datagramme, die jetzt hinausgehen sollen (mit Kopf).
    pub fn block(&mut self, block: &mut [f32], senden_erlaubt: bool) -> Result<Vec<Vec<u8>>> {
        assert_eq!(
            block.len(),
            RAHMEN_SAMPLES,
            "Block muss genau einen 20-ms-Rahmen fassen"
        );

        if self.einstellungen.entrauschen {
            self.entrauscher.verarbeite(block);
        }
        self.pegel = pegel_aus_rms(sprechwerk_adapters::klang::rms(block));

        // Noisegate: unter der Schwelle wird **nicht** gesendet. Zweites
        // Kriterium ist die Sprachwahrscheinlichkeit von RNNoise — ein leises
        // „ja" ist unter der Schwelle, aber klar Sprache; ein Rascheln ist
        // laut, aber keine.
        let gate_offen = if self.einstellungen.noisegate {
            let ueber_schwelle = self.gate.durchlassen(self.pegel);
            let sprache = self.entrauscher.sprachwahrscheinlichkeit()
                >= sprechwerk_adapters::klang::SPRACHWAHRSCHEINLICHKEIT_GRENZE
                && self.pegel >= 5;
            ueber_schwelle || sprache
        } else {
            true
        };
        let senden = senden_erlaubt && gate_offen;

        // Kodiert wird immer — sonst gäbe es beim Öffnen keinen Vorlauf.
        let pcm = crate::pcm::nach_i16(block);
        let laenge = self.kodierer.kodiere(&pcm, &mut self.puffer)?;
        let rahmen = self.puffer[..laenge].to_vec();

        let mut raus = Vec::new();
        if senden {
            if !self.sendete {
                // Vorlauf ausliefern: die letzten Rahmen **vor** dem Live-Ton.
                let alt = self.vorlauf.abholen();
                for r in alt {
                    raus.push(Sprachkopf::neu(self.naechste_folge()).schreiben(&r.daten));
                }
            }
            raus.push(Sprachkopf::neu(self.naechste_folge()).schreiben(&rahmen));
            // Paritätsgruppe füllen. Der Paritätsrahmen geht **sofort** nach dem
            // letzten Rahmen der Gruppe hinaus — dann ist die Wartezeit im
            // Empfänger am kürzesten.
            if self.einstellungen.paritaet {
                self.gruppe.push(rahmen.clone());
                if self.gruppe.len() == GRUPPE as usize {
                    let laengen: Vec<u16> =
                        self.gruppe.iter().map(|r| r.len() as u16).collect();
                    let summe = Paritaet::summe(&self.gruppe);
                    let ab = self.folge.wrapping_sub(GRUPPE);
                    raus.push(paritaets_datagramm(ab, &laengen, &summe));
                    self.gruppe.clear();
                }
            } else {
                self.gruppe.clear();
            }
            self.sendete = true;
        } else {
            self.vorlauf.aufnehmen(rahmen);
            self.sendete = false;
        }
        Ok(raus)
    }

    fn naechste_folge(&mut self) -> u16 {
        let folge = self.folge;
        self.folge = self.folge.wrapping_add(1);
        folge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sinus(anzahl: usize, amplitude: f32) -> Vec<f32> {
        (0..anzahl)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * 500.0 * i as f32 / crate::pcm::ABTASTRATE as f32)
                    .sin()
            })
            .collect()
    }

    #[test]
    fn stumm_sendet_nichts_und_fuellt_den_vorlauf() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: false,
            ..Default::default()
        })
        .unwrap();
        for _ in 0..10 {
            let mut block = sinus(RAHMEN_SAMPLES, 0.5);
            let raus = s.block(&mut block, false).unwrap();
            assert!(raus.is_empty(), "stumm darf nichts senden");
        }
        // Beim Drücken kommt der Vorlauf mit — also mehrere Rahmen auf einmal.
        let mut block = sinus(RAHMEN_SAMPLES, 0.5);
        let raus = s.block(&mut block, true).unwrap();
        assert!(
            raus.len() >= 3,
            "Vorlauf muss mitgehen, waren nur {} Rahmen",
            raus.len()
        );
        for datagramm in &raus {
            let (kopf, nutzlast) = Sprachkopf::lesen(datagramm).expect("gültiger Kopf");
            assert_eq!(kopf.version, sprechwerk_protocol::SPRACHKOPF_VERSION);
            assert!(!nutzlast.is_empty());
        }
    }

    #[test]
    fn folgenummern_laufen_und_bleiben_im_rahmen() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: false,
            ..Default::default()
        })
        .unwrap();
        let mut letzte: Option<u16> = None;
        for _ in 0..50 {
            let mut block = sinus(RAHMEN_SAMPLES, 0.5);
            for datagramm in s.block(&mut block, true).unwrap() {
                let (kopf, _) = Sprachkopf::lesen(&datagramm).unwrap();
                // Paritätsrahmen stehen neben der Folgenreihe, nicht darin.
                if kopf.codec == crate::verlust::CODEC_PARITAET {
                    continue;
                }
                if let Some(alt) = letzte {
                    assert_eq!(
                        kopf.folge,
                        alt.wrapping_add(1),
                        "Folgenummer muss lückenlos sein"
                    );
                }
                letzte = Some(kopf.folge);
            }
        }
    }

    #[test]
    fn paritaet_kommt_je_vier_rahmen() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: false,
            paritaet: true,
            ..Default::default()
        })
        .unwrap();
        let mut rahmen = 0;
        let mut paritaeten = 0;
        for _ in 0..8 {
            let mut block = sinus(RAHMEN_SAMPLES, 0.5);
            for d in s.block(&mut block, true).unwrap() {
                let (kopf, _) = Sprachkopf::lesen(&d).unwrap();
                if kopf.codec == crate::verlust::CODEC_PARITAET {
                    paritaeten += 1;
                } else {
                    rahmen += 1;
                }
            }
        }
        assert_eq!(rahmen, 8);
        assert_eq!(paritaeten, 2, "je vier Rahmen ein Paritätsrahmen");
    }

    #[test]
    fn ohne_paritaet_bleibt_die_last_klein() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: false,
            paritaet: false,
            ..Default::default()
        })
        .unwrap();
        for _ in 0..8 {
            let mut block = sinus(RAHMEN_SAMPLES, 0.5);
            for d in s.block(&mut block, true).unwrap() {
                let (kopf, _) = Sprachkopf::lesen(&d).unwrap();
                assert_ne!(kopf.codec, crate::verlust::CODEC_PARITAET);
            }
        }
    }

    #[test]
    fn gate_haelt_stille_zurueck() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: true,
            schwelle: 40,
            ..Default::default()
        })
        .unwrap();
        // Sehr leiser Block: Pegel unter der Schwelle.
        let mut leise = sinus(RAHMEN_SAMPLES, 0.0005);
        let raus = s.block(&mut leise, true).unwrap();
        assert!(raus.is_empty(), "unter der Schwelle darf nichts raus");
        assert!(s.pegel() < 40);
    }

    #[test]
    fn lauter_block_kommt_durch() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: true,
            schwelle: 30,
            ..Default::default()
        })
        .unwrap();
        let mut laut = sinus(RAHMEN_SAMPLES, 0.5);
        let raus = s.block(&mut laut, true).unwrap();
        assert!(!raus.is_empty(), "lauter Block muss durch");
        assert!(s.pegel() > 30, "Pegel {}", s.pegel());
    }

    #[test]
    fn stummschaltung_wirkt_sofort() {
        let mut s = Sender::neu(Sendeeinstellungen {
            noisegate: false,
            ..Default::default()
        })
        .unwrap();
        let mut block = sinus(RAHMEN_SAMPLES, 0.5);
        assert!(!s.block(&mut block, true).unwrap().is_empty());
        let mut block = sinus(RAHMEN_SAMPLES, 0.5);
        assert!(
            s.block(&mut block, false).unwrap().is_empty(),
            "nach dem Stummschalten darf nichts mehr raus"
        );
    }
}

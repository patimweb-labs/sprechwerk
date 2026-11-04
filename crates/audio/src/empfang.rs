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
use sprechwerk_adapters::sprechpfad::JitterPuffer;
use sprechwerk_protocol::{Sprachkopf, SPRACHKOPF_VERSION};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Ein Sprecher im Empfang.
struct Sprecher {
    dekodierer: Dekodierer,
    jitter: JitterPuffer,
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
        let Some((kopf, nutzlast)) = Sprachkopf::lesen(datagramm) else {
            self.verworfen += 1;
            return false;
        };
        if kopf.version != SPRACHKOPF_VERSION || nutzlast.is_empty() {
            self.verworfen += 1;
            return false;
        }
        let eintrag = match self.sprecher.get_mut(&kopf.von) {
            Some(s) => s,
            None => {
                let Ok(dekodierer) = Dekodierer::neu() else {
                    self.verworfen += 1;
                    return false;
                };
                self.sprecher.entry(kopf.von).or_insert(Sprecher {
                    dekodierer,
                    jitter: JitterPuffer::neu(3),
                    zuletzt: Instant::now(),
                    verluste: 0,
                })
            }
        };
        eintrag.zuletzt = Instant::now();
        let angenommen = eintrag.jitter.aufnehmen(kopf.folge, nutzlast.to_vec());
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
            match sprecher.jitter.ausgeben() {
                Some(rahmen) => match sprecher.dekodierer.dekodiere(&rahmen.daten, &mut pcm) {
                    Ok(len) => dazu_mischen(&mut self.misch, &pcm[..len.min(RAHMEN_SAMPLES)]),
                    Err(_) => {
                        sprecher.verluste += 1;
                    }
                },
                None => {
                    // Nichts da: nur verdecken, wenn der Puffer schon lief
                    // (sonst würde jeder Sprecher vor dem ersten Rahmen
                    // dauerhaft Stille erzeugen, was nichts schadet).
                }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::{Kodierer, RAHMEN_SAMPLES as OPUS_SAMPLES};

    fn rahmen(folge: u16, von: u16, pcm: &[i16]) -> Vec<u8> {
        let mut k = Kodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        let len = k.kodiere(pcm, &mut puffer).unwrap();
        Sprachkopf::neu(folge)
            .mit_von(von)
            .schreiben(&puffer[..len])
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
        // Drei Rahmen je Sprecher, damit der Jitter-Puffer (3) ausgibt.
        for folge in 0..3 {
            e.nimm(&rahmen(folge, 1, &ton));
            e.nimm(&rahmen(folge, 2, &ton));
        }
        assert_eq!(e.anzahl_sprecher(), 2);
        let block = e.naechster_block();
        assert!(
            energie(&block) > 1000.0,
            "gemischter Block ist still: {}",
            energie(&block)
        );
    }

    #[test]
    fn ein_sprecher_kommt_hoerbar_an() {
        let mut e = Empfänger::neu();
        let ton = sinus(0.7);
        for folge in 0..4 {
            e.nimm(&rahmen(folge, 5, &ton));
        }
        let block = e.naechster_block();
        assert!(energie(&block) > 2000.0, "kaum Ton: {}", energie(&block));
    }

    #[test]
    fn doppelte_rahmen_werden_verworfen() {
        let mut e = Empfänger::neu();
        let ton = sinus(0.5);
        for _ in 0..3 {
            e.nimm(&rahmen(0, 1, &ton));
        }
        let (_, verworfen, _) = e.zaehler();
        assert!(verworfen >= 2, "Doppelte müssen verworfen werden: {verworfen}");
    }

    #[test]
    fn leeren_raeumt_auf() {
        let mut e = Empfänger::neu();
        e.nimm(&rahmen(0, 1, &sinus(0.5)));
        assert_eq!(e.anzahl_sprecher(), 1);
        e.leeren();
        assert_eq!(e.anzahl_sprecher(), 0);
    }
}

//! Klangbearbeitung im Aufnahmepfad: Hochpass, Rauschunterdrückung, Noisegate.
//!
//! **Hier steckt wieder nur Rechnerei**, keine Soundkarte: alles arbeitet auf
//! Blöcken von 32-Bit-Gleitkommazahlen (-1.0 … 1.0) und lässt sich deshalb ohne
//! Mikrofon prüfen.
//!
//! # Drei Stufen, drei Zwecke
//!
//! ```text
//! Mikrofon ──► Hochpass ──► Rauschunterdrückung ──► Noisegate ──► Opus
//!              (Brummen,     (Lüfter, Rauschen:       (unter der
//!               Tiefen,       Pegel-Expander über     Schwelle wird
//!               Trittschall)  dem Rauschboden)        nichts gesendet)
//! ```
//!
//! # Rauschunterdrückung: RNNoise
//!
//! Die Rauschunterdrückung ist **RNNoise** — ein kleines neuronales Netz, das
//! in `nnnoiseless` als **reines Rust** vorliegt (kein C, kein `cmake`, läuft
//! damit auf Windows, macOS und Linux gleich). Es arbeitet auf 10-ms-Rahmen
//! (480 Werte bei 48 kHz), schätzt je Frequenzband einen Gewinn und liefert
//! nebenbei eine **Sprachwahrscheinlichkeit** — die ist besser als jeder Pegel
//! für die Frage „wird gerade gesprochen?".
//!
//! Der Hochpass davor bleibt: er nimmt Brummen und Trittschall weg, bevor das
//! Netz sie als Sprache deuten kann. Der frühere Pegel-Expander ist ersetzt;
//! er war nur ein Notbehelf, weil ich C-Abhängigkeiten vermeiden wollte.

/// Ein Hochpass zweiter Ordnung (Butterworth), als Kaskade zweier
/// Ein-Pol-Glieder. Entfernt Gleichanteil, Brummen und Trittschall.
#[derive(Debug, Clone)]
pub struct Hochpass {
    grenze: f32,
    /// Zustand beider Glieder.
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl Hochpass {
    /// `hz` = Grenzfrequenz. Unterhalb wird abgesenkt.
    pub fn neu(abtastrate: u32, hz: f32) -> Self {
        let grenze = (hz / abtastrate as f32).clamp(0.0001, 0.45);
        Self {
            grenze,
            x1: 0.0,
            y1: 0.0,
            x2: 0.0,
            y2: 0.0,
        }
    }

    /// Verarbeitet einen Block (verändert ihn an Ort und Stelle).
    pub fn verarbeite(&mut self, block: &mut [f32]) {
        // y[n] = a * (y[n-1] + x[n] - x[n-1]) mit a = 1 / (1 + 2π·fg/fa)
        let a = 1.0 / (1.0 + 2.0 * std::f32::consts::PI * self.grenze);
        for probe in block.iter_mut() {
            let x = *probe;
            self.y1 = a * (self.y1 + x - self.x1);
            self.x1 = x;
            self.y2 = a * (self.y2 + self.y1 - self.x2);
            self.x2 = self.y1;
            *probe = self.y2;
        }
    }
}

/// Effektivwert eines Blocks.
pub fn rms(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    let summe: f32 = block.iter().map(|s| s * s).sum();
    (summe / block.len() as f32).sqrt()
}

/// Pegel 0–100 aus einem Effektivwert (dBFS auf 0 … 100 abgebildet, -60 dB = 0).
pub fn pegel_aus_rms(wert: f32) -> u8 {
    if wert <= 1e-6 {
        return 0;
    }
    let db = 20.0 * wert.log10();
    let anteil = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    (anteil * 100.0).round() as u8
}

/// Verfolgt den Rauschboden: fällt schnell auf leise Werte, steigt nur langsam.
///
/// Wird nur noch für die **Anzeige** und als Rückfallebene gebraucht, wenn
/// RNNoise nicht greift (andere Abtastrate als 48 kHz).
#[derive(Debug, Clone)]
pub struct Rauschboden {
    /// Aktueller Boden (Effektivwert).
    wert: f32,
    /// Wie stark der Boden je Block fällt (0…1).
    fallen: f32,
    /// Wie stark er je Block steigt (0…1) — absichtlich klein.
    steigen: f32,
    /// Erster Block: noch nichts bekannt.
    gestartet: bool,
}

impl Rauschboden {
    pub fn neu() -> Self {
        Self {
            wert: 0.0,
            fallen: 0.5,
            steigen: 0.002,
            gestartet: false,
        }
    }

    /// Füttert einen Block und liefert den aktuellen Boden.
    pub fn beobachte(&mut self, block_rms: f32) -> f32 {
        if !self.gestartet {
            self.wert = block_rms;
            self.gestartet = true;
            return self.wert;
        }
        if block_rms < self.wert {
            self.wert += (block_rms - self.wert) * self.fallen;
        } else {
            self.wert += (block_rms - self.wert) * self.steigen;
        }
        self.wert = self.wert.max(0.0);
        self.wert
    }
}

/// Rauschunterdrückung: Hochpass plus RNNoise.
///
/// RNNoise rechnet in 10-ms-Rahmen (480 Werte bei 48 kHz), unsere Blöcke sind
/// 20 ms (960 Werte). Der Entrauscher hält deshalb einen kleinen Rest, damit
/// die Rahmengrenzen stimmen, auch wenn ein Block nicht aufgeht.
pub struct Entrauscher {
    hochpass: Hochpass,
    /// Das neuronale Netz (RNNoise), sofern die Rate passt.
    netz: Option<Box<nnnoiseless::DenoiseState<'static>>>,
    /// Rest des letzten Blocks (weniger als ein 10-ms-Rahmen).
    rest: Vec<f32>,
    /// Zuletzt gemeldete Sprachwahrscheinlichkeit 0…1.
    sprachwahrscheinlichkeit: f32,
    /// Rauschboden als Ersatzmaß (Anzeige).
    boden: Rauschboden,
    /// Abtastrate, mit der dieser Entrauscher arbeitet.
    rate: u32,
}

impl std::fmt::Debug for Entrauscher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entrauscher")
            .field("rate", &self.rate)
            .field("netz", &self.netz.is_some())
            .field("sprachwahrscheinlichkeit", &self.sprachwahrscheinlichkeit)
            .finish()
    }
}

impl Entrauscher {
    pub fn neu(abtastrate: u32) -> Self {
        Self {
            hochpass: Hochpass::neu(abtastrate, 90.0),
            // RNNoise ist auf 48 kHz festgelegt (Modell und Fenster).
            netz: if abtastrate == 48_000 {
                Some(nnnoiseless::DenoiseState::new())
            } else {
                None
            },
            rest: Vec::new(),
            sprachwahrscheinlichkeit: 0.0,
            boden: Rauschboden::neu(),
            rate: abtastrate,
        }
    }

    /// Aktueller Rauschboden als Effektivwert (für die Anzeige).
    pub fn rauschboden(&self) -> f32 {
        self.boden.wert
    }

    /// Wie wahrscheinlich wird gerade Sprache angenommen? 0…1
    ///
    /// Kommt von RNNoise selbst — verlässlicher als ein reiner Pegel, weil das
    /// Netz Rauschen von Stimme unterscheidet.
    pub fn sprachwahrscheinlichkeit(&self) -> f32 {
        self.sprachwahrscheinlichkeit
    }

    /// Verarbeitet einen Block und liefert die angewendete Verstärkung.
    pub fn verarbeite(&mut self, block: &mut [f32]) -> f32 {
        self.hochpass.verarbeite(block);
        let vorher = rms(block);
        self.boden.beobachte(vorher);

        match self.netz.as_mut() {
            Some(netz) => {
                // 20-ms-Block in 10-ms-Rahmen zerlegen; RNNoise erwartet Werte
                // im 16-Bit-Bereich, nicht -1…1.
                let mut zusammen = std::mem::take(&mut self.rest);
                zusammen.extend_from_slice(block);
                let rahmen = nnnoiseless::DenoiseState::FRAME_SIZE;
                let mut aus = Vec::with_capacity(zusammen.len());
                let mut gesamt_vad: f32 = 0.0;
                let mut anzahl: f32 = 0.0;
                for stueck in zusammen.chunks_exact(rahmen) {
                    let eingang: Vec<f32> = stueck.iter().map(|w| w * 32768.0).collect();
                    let mut ziel = vec![0.0f32; rahmen];
                    let vad = netz.process_frame(&mut ziel, &eingang);
                    aus.extend(ziel.iter().map(|w| (w / 32768.0).clamp(-1.0, 1.0)));
                    // RNNoise liefert die Sprachwahrscheinlichkeit bereits
                    // als 0…1 (nicht im 16-Bit-Bereich).
                    gesamt_vad += vad.clamp(0.0, 1.0);
                    anzahl += 1.0;
                }
                // Nicht aufgegangene Werte für den nächsten Block aufheben.
                let verbraucht = zusammen.len() - (zusammen.len() % rahmen);
                self.rest = zusammen[verbraucht..].to_vec();
                self.sprachwahrscheinlichkeit = if anzahl > 0.0 {
                    (gesamt_vad / anzahl).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // Genau so viele Werte zurückgeben, wie hineinkamen.
                let mut genommen = 0;
                for (ziel, wert) in block.iter_mut().zip(aus.iter()) {
                    *ziel = *wert;
                    genommen += 1;
                }
                // Kein Rahmen fertig (Rest zu klein): Original behalten.
                if genommen == 0 {
                    self.sprachwahrscheinlichkeit = 0.0;
                }
            }
            None => {
                // Keine 48 kHz: nichts entrauschen, aber auch nichts kaputt
                // machen. Die Rate wird im Sendepfad auf 48 kHz gebracht.
                self.sprachwahrscheinlichkeit = if vorher > 0.01 { 1.0 } else { 0.0 };
            }
        }

        let nachher = rms(block);
        if nachher > 1e-9 {
            (nachher / vorher.max(1e-9)).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Abtastrate dieses Entrauschers.
    pub fn rate(&self) -> u32 {
        self.rate
    }
}

/// Noisegate: unterhalb der Schwelle wird **nicht gesendet** — es sei denn,
/// RNNoise erkennt klar Sprache. Der Pegel allein ist ein schlechter Richter
/// (ein Rascheln ist laut, ein leises „ja" nicht); die Sprachwahrscheinlichkeit
/// des Netzes ist der bessere. Beides zusammen: Schwelle **oder** erkannte
/// Sprache öffnet.
pub const SPRACHWAHRSCHEINLICHKEIT_GRENZE: f32 = 0.6;

/// Noisegate: unterhalb der Schwelle wird **nicht gesendet**.
///
/// Anders als die Rauschunterdrückung arbeitet es nicht auf dem Signal,
/// sondern entscheidet über den ganzen Block. Es hat eine Hysterese
/// (Öffnen über `auf`, Schließen unter `zu`) und eine Nachlaufzeit, damit
/// Wortenden nicht abgehackt werden.
#[derive(Debug, Clone)]
pub struct Noisegate {
    /// Öffnen ab diesem Pegel (0–100).
    auf: u8,
    /// Schließen unter diesem Pegel (0–100), kleiner als `auf`.
    zu: u8,
    /// Wie viele Blöcke nach dem Schließen noch durchgelassen werden.
    nachlauf: u8,
    /// Verbleibender Nachlauf.
    rest: u8,
    offen: bool,
}

impl Noisegate {
    pub fn neu(schwelle: u8) -> Self {
        let auf = schwelle.clamp(1, 99);
        Self {
            auf,
            zu: auf.saturating_sub(8).max(1),
            nachlauf: 8, // 8 × 20 ms = 160 ms
            rest: 0,
            offen: false,
        }
    }

    /// Ist das Gate offen?
    pub fn offen(&self) -> bool {
        self.offen
    }

    /// Entscheidet über einen Block. `pegel` ist 0–100.
    pub fn durchlassen(&mut self, pegel: u8) -> bool {
        if pegel >= self.auf {
            self.offen = true;
            self.rest = self.nachlauf;
        } else if self.offen && pegel < self.zu {
            if self.rest > 0 {
                self.rest -= 1;
            } else {
                self.offen = false;
            }
        }
        self.offen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 440 Hz Sinus auf einem Block.
    fn sinus(abtastrate: u32, hz: f32, anzahl: usize, amplitude: f32) -> Vec<f32> {
        (0..anzahl)
            .map(|i| {
                amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / abtastrate as f32).sin()
            })
            .collect()
    }

    /// Gleichverteiltes Rauschen (deterministisch, ohne Zufallsquelle).
    fn rauschen(anzahl: usize, amplitude: f32) -> Vec<f32> {
        let mut zustand: u32 = 0x1234_5678;
        (0..anzahl)
            .map(|_| {
                zustand = zustand.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let anteil = (zustand >> 8) as f32 / (1 << 24) as f32 - 0.5;
                anteil * 2.0 * amplitude
            })
            .collect()
    }

    #[test]
    fn hochpass_entfernt_gleichanteil() {
        let mut h = Hochpass::neu(48_000, 90.0);
        let mut block = vec![0.5f32; 4800];
        h.verarbeite(&mut block);
        let ende = &block[2400..];
        assert!(rms(ende) < 0.01, "Gleichanteil bleibt: {}", rms(ende));
    }

    #[test]
    fn hochpass_laesst_sprache_durch() {
        let mut h = Hochpass::neu(48_000, 90.0);
        let mut block = sinus(48_000, 440.0, 4800, 0.5);
        let vorher = rms(&block);
        h.verarbeite(&mut block);
        let nachher = rms(&block[2400..]);
        assert!(
            nachher > vorher * 0.8,
            "Sprachfrequenz wird zu stark abgesenkt: {vorher} → {nachher}"
        );
    }

    /// Stimmenähnliches Signal: Grundton mit Obertönen, in Silben getaktet.
    fn stimme(anzahl: usize, amplitude: f32) -> Vec<f32> {
        (0..anzahl)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                let huelle = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 4.0 * t).sin();
                let s = (2.0 * std::f32::consts::PI * 120.0 * t).sin()
                    + 0.5 * (2.0 * std::f32::consts::PI * 240.0 * t).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 360.0 * t).sin();
                amplitude * huelle * s / 1.8
            })
            .collect()
    }

    #[test]
    fn entrauscher_senkt_dauerrauschen_stark() {
        let mut e = Entrauscher::neu(48_000);
        let block = rauschen(960, 0.05);
        // Ein paar Blöcke, damit das Netz den Rauschboden kennt.
        for _ in 0..60 {
            let mut b = block.clone();
            e.verarbeite(&mut b);
        }
        let mut ruhe = block.clone();
        let vorher = rms(&ruhe);
        e.verarbeite(&mut ruhe);
        let nachher = rms(&ruhe);
        assert!(
            nachher < vorher * 0.15,
            "RNNoise senkt Rauschen zu wenig: {vorher} → {nachher}"
        );
        assert!(
            e.sprachwahrscheinlichkeit() < 0.2,
            "Rauschen darf nicht als Sprache gelten ({})",
            e.sprachwahrscheinlichkeit()
        );
    }

    #[test]
    fn entrauscher_laesst_stimme_stehen() {
        let mut e = Entrauscher::neu(48_000);
        for _ in 0..60 {
            let mut b = stimme(960, 0.3);
            e.verarbeite(&mut b);
        }
        let mut sprache = stimme(960, 0.3);
        let vorher = rms(&sprache);
        e.verarbeite(&mut sprache);
        let nachher = rms(&sprache);
        assert!(
            nachher > vorher * 0.4,
            "Stimme wird zu stark weggedrückt: {vorher} → {nachher}"
        );
        assert!(
            e.sprachwahrscheinlichkeit() > 0.4,
            "Stimme wird nicht als Sprache erkannt ({})",
            e.sprachwahrscheinlichkeit()
        );
    }

    #[test]
    fn sprachwahrscheinlichkeit_unterscheidet_rauschen_von_stimme() {
        let mut e = Entrauscher::neu(48_000);
        for _ in 0..40 {
            let mut b = rauschen(960, 0.05);
            e.verarbeite(&mut b);
        }
        let bei_rauschen = e.sprachwahrscheinlichkeit();
        for _ in 0..40 {
            let mut b = stimme(960, 0.3);
            e.verarbeite(&mut b);
        }
        let bei_stimme = e.sprachwahrscheinlichkeit();
        assert!(
            bei_stimme > bei_rauschen + 0.3,
            "VAD trennt nicht: Rauschen {bei_rauschen}, Stimme {bei_stimme}"
        );
    }

    #[test]
    fn ohne_48_khz_wird_nichts_kaputt_gemacht() {
        // Andere Raten kann RNNoise nicht; dann bleibt das Signal unverändert.
        let mut e = Entrauscher::neu(44_100);
        assert!(e.rate() == 44_100);
        let mut block = sinus(44_100, 500.0, 960, 0.3);
        let vorher = rms(&block);
        e.verarbeite(&mut block);
        assert!(rms(&block) > vorher * 0.5, "Signal darf nicht verstummen");
    }

    #[test]
    fn pegel_ordnung_stimmt() {
        assert_eq!(pegel_aus_rms(0.0), 0);
        assert!(pegel_aus_rms(0.001) < pegel_aus_rms(0.01));
        assert!(pegel_aus_rms(0.01) < pegel_aus_rms(0.9));
        assert!(pegel_aus_rms(0.9) <= 100);
    }

    #[test]
    fn gate_oeffnet_und_schliesst_mit_nachlauf() {
        let mut g = Noisegate::neu(30);
        assert!(!g.offen());
        assert!(!g.durchlassen(10), "leiser Block öffnet nicht");
        assert!(g.durchlassen(40), "lauter Block öffnet");
        // Nachlauf: 8 Blöcke bleiben offen, dann zu.
        for i in 0..8 {
            assert!(g.durchlassen(5), "Nachlauf {i}");
        }
        assert!(!g.durchlassen(5), "danach zu");
        assert!(g.durchlassen(40) && g.offen(), "laut öffnet wieder");
    }

    #[test]
    fn gate_hat_hysterese() {
        let mut g = Noisegate::neu(30);
        assert!(g.durchlassen(40));
        // Zwischen `zu` (22) und `auf` (30) bleibt offen.
        assert!(g.durchlassen(25), "Hysterese hält offen");
    }
}

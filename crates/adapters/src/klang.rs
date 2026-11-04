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
//! # Was die Rauschunterdrückung ist — und was nicht
//!
//! Sie ist **kein** Spektral-Denoiser wie RNNoise. Sie verfolgt den Rauschboden
//! (das leiseste, was das Mikrofon dauerhaft liefert) und regelt die
//! Verstärkung darüber herunter. Das hilft gegen Lüfter, Brummen und
//! Grundrauschen — gegen einzelne Geräusche (Tastatur, Papier) hilft es
//! weniger. Diese Grenze ist bewusst dokumentiert (`docs/AUDIO.md`), damit
//! niemand mehr erwartet, als drin ist.

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
/// Das ist der Kern der Rauschunterdrückung. Ein schneller Anstieg würde
/// Sprache als Rauschen deuten und wegdrücken; ein langsamer Abfall würde nach
/// einer lauten Stelle zu lange taub bleiben.
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

/// Rauschunterdrückung: Hochpass plus Pegel-Expander über dem Rauschboden.
#[derive(Debug, Clone)]
pub struct Entrauscher {
    hochpass: Hochpass,
    boden: Rauschboden,
    /// Wie weit über dem Boden ein Block als „Signal" gilt (Faktor).
    abstand: f32,
    /// Höchste Absenkung (0.1 = höchstens 20 dB leiser).
    kleinste_verstaerkung: f32,
    /// Geglättete Verstärkung, damit es nicht zischelt.
    verstaerkung: f32,
}

impl Entrauscher {
    pub fn neu(abtastrate: u32) -> Self {
        Self {
            hochpass: Hochpass::neu(abtastrate, 90.0),
            boden: Rauschboden::neu(),
            abstand: 2.5,
            kleinste_verstaerkung: 0.12,
            verstaerkung: 1.0,
        }
    }

    /// Aktueller Rauschboden als Effektivwert (für die Anzeige).
    pub fn rauschboden(&self) -> f32 {
        self.boden.wert
    }

    /// Verarbeitet einen Block und liefert die angewendete Verstärkung.
    pub fn verarbeite(&mut self, block: &mut [f32]) -> f32 {
        self.hochpass.verarbeite(block);
        let wert = rms(block);
        let boden = self.boden.beobachte(wert);

        // Verstärkung: 1.0 deutlich über dem Boden, darunter weich abgesenkt.
        let ziel = if boden <= 1e-6 {
            1.0
        } else {
            let verhaeltnis = wert / (boden * self.abstand);
            let roh = (verhaeltnis - 0.5).clamp(0.0, 1.0);
            // quadratisch: leise Stellen werden stärker abgesenkt als laute
            roh * roh * (1.0 - self.kleinste_verstaerkung) + self.kleinste_verstaerkung
        };
        // Schnell aufmachen, langsam zumachen: sonst wird der Wortanfang
        // leiser als der Rest des Wortes („Anschnitt").
        let tempo = if ziel > self.verstaerkung { 0.95 } else { 0.2 };
        self.verstaerkung += (ziel - self.verstaerkung) * tempo;
        for probe in block.iter_mut() {
            *probe *= self.verstaerkung;
        }
        self.verstaerkung
    }
}

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

    #[test]
    fn entrauscher_senkt_dauerrauschen() {
        let mut e = Entrauscher::neu(48_000);
        let block = rauschen(960, 0.02);
        // Ein paar Blöcke, damit der Boden steht.
        for _ in 0..50 {
            let mut b = block.clone();
            e.verarbeite(&mut b);
        }
        let mut ruhe = block.clone();
        let vorher = rms(&ruhe);
        e.verarbeite(&mut ruhe);
        let nachher = rms(&ruhe);
        assert!(
            nachher < vorher * 0.6,
            "Rauschen wird nicht gesenkt: {vorher} → {nachher}"
        );
    }

    #[test]
    fn entrauscher_laesst_sprache_stehen() {
        let mut e = Entrauscher::neu(48_000);
        let leise = rauschen(960, 0.002);
        for _ in 0..60 {
            let mut b = leise.clone();
            e.verarbeite(&mut b);
        }
        let mut sprache = sinus(48_000, 500.0, 960, 0.3);
        let vorher = rms(&sprache);
        e.verarbeite(&mut sprache);
        let nachher = rms(&sprache);
        assert!(
            nachher > vorher * 0.85,
            "Sprache wird weggedrückt: {vorher} → {nachher}"
        );
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

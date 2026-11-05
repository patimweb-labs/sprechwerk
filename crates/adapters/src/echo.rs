//! Echounterdrückung (AEC) — die eigene Wiedergabe aus der Aufnahme rechnen.
//!
//! # Warum das ohne Kopfhörer nötig ist
//!
//! Wer über Lautsprecher spricht, nimmt die anderen mit auf: das Mikrofon hört
//! die Boxen. Das Ergebnis ist eine Rückkopplung — erst ein hohler Klang, dann
//! Pfeifen. Kopfhörer lösen das, aber ein Verein kann das nicht voraussetzen.
//!
//! Die Lösung ist, den bekannten Anteil **herauszurechnen**: was wir selbst
//! abspielen, kennen wir (es liegt in diesem Prozess als Zahlen vor). Ein
//! adaptiver Filter schätzt den Weg „Wiedergabe → Raum → Mikrofon" und zieht
//! ihn vom Aufnahmesignal ab. Genau das ist AEC3 aus WebRTC, hier als
//! **reines Rust** (`aec3`-Crate) — damit auf Windows, macOS und Linux gleich.
//!
//! # Was es nicht kann
//!
//! Doppelsprechen (beide reden gleichzeitig) dämpft AEC3 nur teilweise, und
//! wenn sich der Raum ändert (jemand geht hin und her), braucht es einen
//! Moment zum Nachziehen. Es ersetzt keine Kopfhörer, aber es verhindert das
//! Pfeifen.
//!
//! # Aufbau dieses Moduls
//!
//! Die Bühne rechnet in 10-ms-Rahmen (480 Werte bei 48 kHz), der Sprechpfad in
//! 20-ms-Blöcken. Deshalb wird hier geteilt und wieder zusammengesetzt.

/// Ein 10-ms-Rahmen bei 48 kHz — die Rechengröße von AEC3.
const RAHMEN_10MS: usize = 480;

/// Echounterdrückung für den Aufnahmepfad.
pub struct Echounterdrueckung {
    /// Die fertige Kette aus dem `aec3`-Crate (Hochpass, AEC3, Rauschfilter).
    pipeline: Box<dyn EchoPipeline>,
    /// Wie viele Rahmen bisher verarbeitet wurden (für Prüfungen).
    verarbeitet: u64,
}

/// Kleine Hülle, damit dieses Modul nicht in jedem Test die ganze Kette bauen
/// muss (und damit ein Fehlschlag beim Bauen ehrlich gemeldet wird).
trait EchoPipeline {
    fn referenz(&mut self, rahmen: &[f32]);
    fn verarbeite(&mut self, rahmen: &[f32], aus: &mut [f32]);
}

struct Aec3 {
    pipeline: aec3::pipelines::linear::LinearPipeline,
}

impl EchoPipeline for Aec3 {
    fn referenz(&mut self, rahmen: &[f32]) {
        let _ = self.pipeline.handle_render_frame(rahmen);
    }

    fn verarbeite(&mut self, rahmen: &[f32], aus: &mut [f32]) {
        let _ = self.pipeline.process_capture_frame(rahmen, aus);
    }
}

impl Echounterdrueckung {
    /// Baut die Echounterdrückung. `None`, wenn die Abtastrate nicht 48 kHz ist
    /// (AEC3 ist darauf festgelegt).
    pub fn neu(abtastrate: u32, erwartete_verzoegerung_ms: i32) -> Option<Self> {
        if abtastrate != 48_000 {
            return None;
        }
        use aec3::nodes::audio::AudioFormat;
        let format = AudioFormat::ten_ms(48_000, 1);
        let pipeline = aec3::pipelines::linear::builder(format, format)
            // Die Verzögerung zwischen „wir spielen" und „das Mikrofon hört es"
            // ist nicht null (Puffer, Laufzeit im Raum). Der Hinweis spart
            // Konvergenzzeit; AEC3 schätzt sie sonst selbst.
            .initial_delay_ms(erwartete_verzoegerung_ms)
            // **Nur AEC.** Rauschunterdrückung und Pegelregelung machen wir
            // selbst (RNNoise im `klang`-Modul): zweimal entrauschen klingt
            // dumpf, und eine automatische Verstärkung würde die Sprechschwelle
            // unbrauchbar machen.
            .enable_noise_suppression(false)
            .enable_gain_controller2(false)
            .enable_post_filter(false)
            .export_metrics(false)
            .build()
            .ok()?;
        Some(Self {
            pipeline: Box::new(Aec3 { pipeline }),
            verarbeitet: 0,
        })
    }

    /// Meldet, was gerade **abgespielt** wird (Referenzsignal).
    ///
    /// Muss vor dem zugehörigen Aufnahmeblock kommen, sonst filtert AEC3 gegen
    /// die Vergangenheit.
    pub fn referenz(&mut self, block: &[f32]) {
        for stueck in block.chunks_exact(RAHMEN_10MS) {
            self.pipeline.referenz(stueck);
        }
    }

    /// Rechnet den bekannten Echoanteil aus einem 20-ms-Block heraus.
    pub fn verarbeite(&mut self, block: &mut [f32]) {
        let mut aus = vec![0.0f32; RAHMEN_10MS];
        for stueck in block.chunks_exact_mut(RAHMEN_10MS) {
            self.pipeline.verarbeite(stueck, &mut aus);
            stueck.copy_from_slice(&aus);
            self.verarbeitet += 1;
        }
    }

    /// Wie viele 10-ms-Rahmen verarbeitet wurden.
    pub fn verarbeitet(&self) -> u64 {
        self.verarbeitet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Breitbandiges Signal mit Sprachcharakter (Rauschen, tiefpassgefiltert).
    fn sprache(anzahl: usize, start: usize, amplitude: f32) -> Vec<f32> {
        let mut zustand: u32 = 0x1234_5678 ^ (start as u32);
        let mut tief = 0.0f32;
        (0..anzahl)
            .map(|_| {
                zustand = zustand.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let r = (zustand >> 8) as f32 / (1 << 24) as f32 - 0.5;
                tief = 0.7 * tief + 0.3 * r;
                amplitude * tief
            })
            .collect()
    }

    fn rms(block: &[f32]) -> f32 {
        (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
    }

    /// Ein Echo mit 40 ms Verzögerung und 30 % Pegel muss deutlich leiser
    /// werden. Ohne Kopfhörer ist genau das der Fall, den man hört: erst wird
    /// der Klang hohl, dann pfeift es.
    #[test]
    fn echo_wird_um_mehr_als_zwanzig_db_gesenkt() {
        let mut aec = Echounterdrueckung::neu(48_000, 40).expect("AEC");
        let block = 960; // 20 ms, wie im Sprechpfad
        let verzoegerung = 2; // 40 ms
        let gesamt = 300; // 6 s
        let gehoert: Vec<Vec<f32>> = (0..gesamt)
            .map(|i| sprache(block, i * block, 0.4))
            .collect();

        let (mut vorher, mut nachher) = (0.0f32, 0.0f32);
        for i in 0..gesamt {
            aec.referenz(&gehoert[i]);
            // Das Mikrofon hört nur das Echo (niemand spricht dazwischen).
            let mut aufnahme = vec![0.0f32; block];
            if i >= verzoegerung {
                for (k, wert) in gehoert[i - verzoegerung].iter().enumerate() {
                    aufnahme[k] = wert * 0.3;
                }
            }
            if i > 200 {
                vorher += rms(&aufnahme).powi(2);
            }
            aec.verarbeite(&mut aufnahme);
            if i > 200 {
                nachher += rms(&aufnahme).powi(2);
            }
        }

        let daempfung_db = 10.0 * (vorher / nachher.max(1e-12)).log10();
        println!("Echounterdrückung: {daempfung_db:.1} dB");
        assert!(
            daempfung_db > 20.0,
            "Echo zu wenig gesenkt: {daempfung_db:.1} dB — dann pfeift es weiter"
        );
        assert_eq!(aec.verarbeitet(), (gesamt * 2) as u64);
    }

    /// Doppelsprechen (beide reden): das Echo wird schlechter unterdrückt —
    /// das ist bei jedem AEC so —, aber die eigene Stimme darf **nicht**
    /// lauter werden und nicht verschwinden.
    #[test]
    fn doppelsprechen_macht_die_eigene_stimme_nicht_kaputt() {
        let mut aec = Echounterdrueckung::neu(48_000, 40).expect("AEC");
        let block = 960;
        let gesamt = 300;
        let gehoert: Vec<Vec<f32>> = (0..gesamt)
            .map(|i| sprache(block, i * block, 0.4))
            .collect();
        let eigene: Vec<Vec<f32>> = (0..gesamt)
            .map(|i| sprache(block, i * block + 7_777, 0.25))
            .collect();

        let (mut vorher, mut nachher) = (0.0f32, 0.0f32);
        for i in 0..gesamt {
            aec.referenz(&gehoert[i]);
            let mut aufnahme = vec![0.0f32; block];
            if i >= 2 {
                for (k, wert) in gehoert[i - 2].iter().enumerate() {
                    aufnahme[k] = wert * 0.3;
                }
            }
            for (k, wert) in eigene[i].iter().enumerate() {
                aufnahme[k] += wert;
            }
            if i > 200 {
                vorher += rms(&aufnahme).powi(2);
            }
            aec.verarbeite(&mut aufnahme);
            if i > 200 {
                nachher += rms(&aufnahme).powi(2);
            }
        }

        let verhaeltnis_db = 10.0 * (nachher.max(1e-12) / vorher.max(1e-12)).log10();
        assert!(
            verhaeltnis_db < 1.5,
            "Ausgang lauter als Eingang ({verhaeltnis_db:.1} dB) — das darf nicht sein"
        );
        // Der Echoanteil ist nur ein Teil; die eigene Stimme bleibt also hörbar.
        assert!(
            nachher.sqrt() > 0.01,
            "Ausgang ist fast still — die eigene Stimme wurde mitgelöscht"
        );
    }

    /// Wie viel Rechenzeit kostet die Echounterdrückung?
    ///
    /// Ohne diese Zahl wäre „kostet etwas Rechenzeit" eine Behauptung.
    #[test]
    fn rechenzeit_bleibt_weit_unter_echtzeit() {
        let mut aec = Echounterdrueckung::neu(48_000, 40).expect("AEC");
        let block = 960;
        let gesamt = 500; // 10 s Audio
        let gehoert: Vec<Vec<f32>> = (0..gesamt)
            .map(|i| sprache(block, i * block, 0.4))
            .collect();
        let start = std::time::Instant::now();
        for i in 0..gesamt {
            aec.referenz(&gehoert[i]);
            let mut aufnahme = gehoert[if i > 0 { i - 1 } else { 0 }].clone();
            aec.verarbeite(&mut aufnahme);
        }
        let dauer = start.elapsed().as_secs_f32();
        let audio = gesamt as f32 * 0.02;
        let anteil = dauer / audio * 100.0;
        println!("Echounterdrückung: {anteil:.1} % einer Rechenkernzeit (Echtzeitfaktor)");
        assert!(
            anteil < 100.0,
            "rechnet langsamer als Echtzeit: {anteil:.0} %"
        );
    }

    #[test]
    fn ohne_48_khz_gibt_es_keine_echounterdrueckung() {
        assert!(Echounterdrueckung::neu(44_100, 30).is_none());
        assert!(Echounterdrueckung::neu(48_000, 30).is_some());
    }
}

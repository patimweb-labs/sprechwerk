//! Aufnahme, Wiedergabe, Codec — der Sprachpfad des Fensters.
//!
//! # Aufbau
//!
//! ```text
//! Mikrofon ──► parec ──► 20-ms-Blöcke ──► Sender (klang + Opus + Vorlauf) ──► Datagramme
//! Datagramme ──► Empfänger (Jitter + Opus je Sprecher) ──► Mischen ──► pacat ──► Lautsprecher
//! ```
//!
//! **Die Rechnerei liegt woanders und ist dort geprüft:**
//! `sprechwerk_adapters::klang` (Hochpass, Rauschunterdrückung, Noisegate),
//! `sprechwerk_adapters::sprechpfad` (Vorlauf, Jitter-Puffer, Sprechsteuerung),
//! hier `senden` und `empfang` (Kette und Mischung), `opus` (Codec-Bindung).
//! Dieses Modul fügt nur das Gerät hinzu: zwei Kindprozesse von
//! PipeWire/PulseAudio (`parec`, `pacat`).
//!
//! **Warum Kindprozesse und nicht ALSA direkt?** Der Pfad muss auf jedem Rechner
//! bauen, ohne Entwicklungspakete. `parec`/`pacat` sprechen mit demselben
//! Server wie die Oberfläche, brauchen keine Übersetzungszeit für
//! Geräteauswahl und sind im Betrieb erprobt. Der Preis: ein Prozess mehr je
//! Richtung — gemessen wird er in `scripts/sprechprobe.sh`.

/// Zähler für Fehlersuche (wie viele Blöcke die Aufnahme verarbeitet hat).
pub static BLOECKE: AtomicU64 = AtomicU64::new(0);

pub mod empfang;
pub mod geraet;
pub mod verlust;
pub mod opus;
pub mod pcm;
pub mod senden;

pub use empfang::Empfänger;
pub use senden::{Sendeeinstellungen, Sender};

use anyhow::{anyhow, Context, Result};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender as Kanal};
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Zeit eines Blocks (20 ms) — der Takt des ganzen Pfads.
pub const TAKT: Duration = Duration::from_millis(20);

/// Was die Oberfläche zur Laufzeit umschalten darf.
///
/// Alles über Atomics: die Soundkarten-Threads lesen sie ohne Sperre, die
/// Oberfläche schreibt sie ohne Warten.
#[derive(Debug)]
pub struct Steuerung {
    /// Eigenes Mikrofon stumm.
    stumm: AtomicBool,
    /// Push-to-Talk gedrückt.
    taste: AtomicBool,
    /// Push-to-Talk aktiv (sonst offenes Mikrofon).
    push_to_talk: AtomicBool,
    /// Eigener Ton aus („taub"): nichts wird abgespielt.
    taub: AtomicBool,
}

impl Default for Steuerung {
    fn default() -> Self {
        Self::neu()
    }
}

impl Steuerung {
    pub fn neu() -> Self {
        Self {
            stumm: AtomicBool::new(false),
            taste: AtomicBool::new(false),
            push_to_talk: AtomicBool::new(false),
            taub: AtomicBool::new(false),
        }
    }

    pub fn stumm_setzen(&self, an: bool) {
        self.stumm.store(an, Ordering::Relaxed);
    }

    pub fn ist_stumm(&self) -> bool {
        self.stumm.load(Ordering::Relaxed)
    }

    pub fn taste_setzen(&self, an: bool) {
        self.taste.store(an, Ordering::Relaxed);
    }

    pub fn push_to_talk_setzen(&self, an: bool) {
        self.push_to_talk.store(an, Ordering::Relaxed);
    }

    pub fn taub_setzen(&self, an: bool) {
        self.taub.store(an, Ordering::Relaxed);
    }

    /// Sind die eigenen Ohren zu?
    pub fn ist_taub(&self) -> bool {
        self.taub.load(Ordering::Relaxed)
    }

    /// Darf in diesem Moment gesendet werden?
    pub fn senden_erlaubt(&self) -> bool {
        if self.stumm.load(Ordering::Relaxed) {
            return false;
        }
        if self.push_to_talk.load(Ordering::Relaxed) {
            return self.taste.load(Ordering::Relaxed);
        }
        true
    }
}

/// Ein laufender Sprachpfad mit Gerät.
pub struct Pfad {
    steuerung: Arc<Steuerung>,
    /// Fertige Datagramme für das Netz.
    zu_senden: Receiver<Vec<u8>>,
    /// Eingehende Datagramme aus dem Netz.
    zu_hoeren: Kanal<Vec<u8>>,
    /// Pegel des Mikrofons 0–100.
    pegel: Arc<AtomicU8>,
    /// Läuft der Pfad noch?
    laeuft: Arc<AtomicBool>,
    /// Was gerade abgespielt wurde — Referenz für die Echounterdrückung.
    gehoert: Arc<Mutex<std::collections::VecDeque<Vec<f32>>>>,
    /// Umstellen der Sendeseite.
    einstellungen: Kanal<Sendeeinstellungen>,
    /// Wie die Geräte heißen (für die Oberfläche). Die Geräte selbst gehören
    /// den Threads — deshalb steht hier nur der Name: `Pfad` bleibt damit
    /// `Send`, und ein Gerät wird nie von einem fremden Thread angefasst.
    name_aufnahme: String,
    name_wiedergabe: String,
}

impl Pfad {
    /// Startet Aufnahme und Wiedergabe.
    ///
    /// `mikrofon`/`lautsprecher` sind Gerätekennungen wie
    /// `alsa_input.usb-…` (leer = Vorgabe des Systems).
    pub fn starten(
        mikrofon: Option<&str>,
        lautsprecher: Option<&str>,
        einstellungen: Sendeeinstellungen,
    ) -> Result<Self> {
        let steuerung = Arc::new(Steuerung::neu());
        let pegel = Arc::new(AtomicU8::new(0));
        let laeuft = Arc::new(AtomicBool::new(true));
        // Höchstens zwei Sekunden: mehr braucht AEC3 nicht, und der Speicher
        // soll nicht wachsen, wenn die Wiedergabe stockt.
        let gehoert: Arc<Mutex<std::collections::VecDeque<Vec<f32>>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let gehoert_aufnahme = gehoert.clone();

        // ---- Aufnahme ------------------------------------------------
        let quelle = mikrofon.unwrap_or("");
        if quelle.ends_with(".monitor") {
            return Err(anyhow!(
                "„{quelle}“ ist ein Monitor (Wiedergabe), kein Mikrofon — \
                 sonst hört man die eigene Ausgabe"
            ));
        }
        let (sende_tx, zu_senden) = mpsc::channel::<Vec<u8>>();
        let (stell_tx, stell_rx) = mpsc::channel::<Sendeeinstellungen>();
        // Das Gerät wird **im** Thread geöffnet: ein cpal-Strom gehört dem
        // Thread, der ihn gebaut hat.
        let (start_tx, start_rx) = mpsc::channel::<Result<String, String>>();
        let quelle_geraet = quelle.to_string();
        {
            let steuerung = steuerung.clone();
            let pegel = pegel.clone();
            let laeuft = laeuft.clone();
            let einstellungen_start = einstellungen.clone();
            std::thread::Builder::new()
                .name("sprechwerk-aufnahme".into())
                .spawn(move || {
                    // Der Sender lebt ganz in diesem Thread: die
                    // Echounterdrückung (aec3) ist nicht `Send`, und ein Gerät
                    // gehört ohnehin dem Thread, der es geöffnet hat.
                    let mut sender = match Sender::neu(einstellungen_start) {
                        Ok(s) => s,
                        Err(fehler) => {
                            let _ = start_tx.send(Err(format!("Sendezweig: {fehler}")));
                            return;
                        }
                    };
                    let mut quelle = match geraet::aufnahme_oeffnen(&quelle_geraet) {
                        Ok(q) => q,
                        Err(fehler) => {
                            let _ = start_tx.send(Err(format!("Aufnahme: {fehler}")));
                            return;
                        }
                    };
                    let name = quelle.name();
                    let _ = start_tx.send(Ok(name.clone()));
                    let mut roh = vec![0u8; pcm::RAHMEN_BYTES];
                    while laeuft.load(Ordering::Relaxed) {
                        while let Ok(neu) = stell_rx.try_recv() {
                            sender.einstellen(neu);
                        }
                        if let Err(fehler) = quelle.lies(&mut roh) {
                            eprintln!("Aufnahme ({name}): {fehler}");
                            break;
                        }
                        let werte = pcm::bytes_nach_i16(&roh);
                        let mut block = pcm::nach_f32(&werte);
                        // Referenz nachreichen: was in der Zwischenzeit
                        // abgespielt wurde, ist genau das, was das Mikrofon
                        // als Echo hört.
                        if let Ok(mut puffer) = gehoert.lock() {
                            while let Some(gespielt) = puffer.pop_front() {
                                sender.referenz(&gespielt);
                            }
                        }
                        let erlaubt = steuerung.senden_erlaubt();
                        match sender.block(&mut block, erlaubt) {
                            Ok(datagramme) => {
                                pegel.store(sender.pegel(), Ordering::Relaxed);
                                for d in datagramme {
                                    if sende_tx.send(d).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(fehler) => {
                                eprintln!("Aufnahme ({name}): {fehler}");
                                break;
                            }
                        }
                    }
                    eprintln!("Aufnahme beendet ({name})");
                })
                .context("Aufnahmethread")?;
        }
        let name_aufnahme = start_rx
            .recv()
            .map_err(|_| anyhow!("Aufnahmethread ist nicht gestartet"))?
            .map_err(|text| anyhow!("{text}"))?;

        // ---- Wiedergabe ----------------------------------------------
        let ziel = lautsprecher.unwrap_or("");
        let (hoer_tx, zu_hoeren_channel) = mpsc::channel::<Vec<u8>>();
        let (start_wtx, start_wrx) = mpsc::channel::<Result<String, String>>();
        let ziel_geraet = ziel.to_string();
        {
            let laeuft = laeuft.clone();
            let steuerung = steuerung.clone();
            let gehoert_ausgabe = gehoert_aufnahme.clone();
            std::thread::Builder::new()
                .name("sprechwerk-wiedergabe".into())
                .spawn(move || {
                    let mut senke = match geraet::wiedergabe_oeffnen(&ziel_geraet) {
                        Ok(w) => w,
                        Err(fehler) => {
                            let _ = start_wtx.send(Err(format!("Wiedergabe: {fehler}")));
                            return;
                        }
                    };
                    let _ = start_wtx.send(Ok(senke.name()));
                    let mut empfaenger = Empfänger::neu();
                    let mut takt = Instant::now();
                    while laeuft.load(Ordering::Relaxed) {
                        while let Ok(d) = zu_hoeren_channel.try_recv() {
                            // Taub: nichts annehmen. So kommt gar nichts erst in
                            // den Puffer, statt dass es nur stumm geschaltet wird.
                            if !steuerung.ist_taub() {
                                empfaenger.nimm(&d);
                            }
                        }
                        let block = if steuerung.ist_taub() {
                            vec![0i16; pcm::RAHMEN_SAMPLES]
                        } else {
                            empfaenger.naechster_block()
                        };
                        if senke.schreibe(&pcm::i16_nach_bytes(&block)).is_err() {
                            break;
                        }
                        // Für die Echounterdrückung merken, was gerade
                        // hinausging (als Gleitkomma, wie AEC3 es braucht).
                        if let Ok(mut puffer) = gehoert_ausgabe.lock() {
                            puffer.push_back(pcm::nach_f32(&block));
                            while puffer.len() > 100 {
                                puffer.pop_front();
                            }
                        }
                        // Fester Takt: 20 ms je Block, Drift wird ausgeglichen.
                        takt += TAKT;
                        let jetzt = Instant::now();
                        if takt > jetzt {
                            std::thread::sleep(takt - jetzt);
                        } else {
                            takt = jetzt;
                        }
                    }
                })
                .context("Wiedergabethread")?;
        }
        let name_wiedergabe = start_wrx
            .recv()
            .map_err(|_| anyhow!("Wiedergabethread ist nicht gestartet"))?
            .map_err(|text| anyhow!("{text}"))?;

        Ok(Self {
            steuerung,
            zu_senden,
            zu_hoeren: hoer_tx,
            pegel,
            laeuft,
            einstellungen: stell_tx,
            gehoert: gehoert_aufnahme,
            name_aufnahme,
            name_wiedergabe,
        })
    }

    /// Die Steuerung (Stumm, Push-to-Talk) — darf überall benutzt werden.
    pub fn steuerung(&self) -> Arc<Steuerung> {
        self.steuerung.clone()
    }

    /// Neue Einstellungen für die Sendeseite.
    pub fn einstellen(&self, einstellungen: Sendeeinstellungen) {
        let _ = self.einstellungen.send(einstellungen);
    }

    /// Nächstes fertiges Datagramm (oder `None`).
    pub fn naechstes_datagramm(&self) -> Option<Vec<u8>> {
        self.zu_senden.try_recv().ok()
    }

    /// Ein Datagramm aus dem Netz zur Wiedergabe geben.
    pub fn hoeren(&self, datagramm: Vec<u8>) {
        let _ = self.zu_hoeren.send(datagramm);
    }

    /// Wie viele Aufnahmeblöcke verarbeitet wurden (Fehlersuche/Betrieb).
    pub fn bloecke() -> u64 {
        BLOECKE.load(Ordering::Relaxed)
    }

    /// Mikrofonpegel 0–100.
    pub fn pegel(&self) -> u8 {
        self.pegel.load(Ordering::Relaxed)
    }

    /// Welche Geräte benutzt werden (für Meldungen).
    pub fn geraete(&self) -> (String, String) {
        let lesbar = |s: &str| {
            if s.is_empty() {
                "Vorgabe des Systems".to_string()
            } else {
                s.to_string()
            }
        };
        (lesbar(&self.name_aufnahme), lesbar(&self.name_wiedergabe))
    }

    /// Beendet beide Richtungen.
    pub fn stoppen(self) {
        self.beenden();
    }

    /// Beendet beide Richtungen. Die Threads merken es beim nächsten Block
    /// (spätestens nach 20 ms) und schließen dabei ihre Geräte.
    fn beenden(&self) {
        self.laeuft.store(false, Ordering::Relaxed);
    }
}

impl Drop for Pfad {
    fn drop(&mut self) {
        self.beenden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stumm_verbietet_senden() {
        let s = Steuerung::neu();
        assert!(s.senden_erlaubt(), "offenes Mikrofon sendet");
        s.stumm_setzen(true);
        assert!(!s.senden_erlaubt());
        s.stumm_setzen(false);
        assert!(s.senden_erlaubt());
    }

    #[test]
    fn push_to_talk_verlangt_die_taste() {
        let s = Steuerung::neu();
        s.push_to_talk_setzen(true);
        assert!(!s.senden_erlaubt(), "ohne Taste nichts");
        s.taste_setzen(true);
        assert!(s.senden_erlaubt(), "mit Taste senden");
        s.taste_setzen(false);
        assert!(!s.senden_erlaubt());
        // Stumm schlägt die Taste.
        s.taste_setzen(true);
        s.stumm_setzen(true);
        assert!(!s.senden_erlaubt(), "stumm hat Vorrang");
    }

    #[test]
    fn monitor_als_mikrofon_wird_abgelehnt() {
        let ergebnis = Pfad::starten(
            Some("alsa_output.kopfhoerer.monitor"),
            None,
            Sendeeinstellungen::default(),
        );
        let fehler = ergebnis.err().expect("muss abgelehnt werden");
        assert!(fehler.to_string().contains("Monitor"), "{fehler}");
    }
}

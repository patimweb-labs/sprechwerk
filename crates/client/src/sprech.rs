// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der Sprechbetrieb des Fensters: ein Sprachpfad, viele Zugriffe.
//!
//! **Zwei Besitzer, ein Pfad.** Der Verbindungsthread schickt und empfängt
//! Datagramme, die Oberfläche schaltet stumm, drückt Push-to-Talk und liest den
//! Pegel. Deshalb liegt der Pfad hinter einer Sperre, und die Zeitgeber der
//! Aufnahme laufen **nicht** hier, sondern in `sprechwerk_audio`.
//!
//! Was hier steht, ist nur Verwaltung: Starten, Stoppen, Umstellen, Durchreichen.
//! Alles Inhaltliche (Rauschunterdrückung, Gate, Vorlauf, Codec, Mischen) liegt
//! in `sprechwerk_audio` und ist dort geprüft.

use sprechwerk_audio::{Pfad, Sendeeinstellungen, Steuerung};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Der Sprechbetrieb (lebt, solange das Fenster lebt).
#[derive(Default)]
pub struct Sprechbetrieb {
    pfad: Mutex<Option<Pfad>>,
    /// Steuerung auch ohne laufenden Pfad verfügbar (Stumm-Schalter).
    steuerung: Arc<Steuerung>,
    /// Pegel für die Oberfläche, auch wenn der Pfad gerade startet.
    pegel: AtomicU8,
    /// Läuft der Pfad?
    laeuft: AtomicBool,
    /// Geräte, mit denen der laufende Pfad gestartet wurde (roh, wie in den
    /// Einstellungen). Nur so lässt sich ein **Gerätewechsel** von einer
    /// Schwelle-Änderung unterscheiden.
    geraete: Mutex<(String, String)>,
}

impl Sprechbetrieb {
    pub fn neu() -> Self {
        Self {
            pfad: Mutex::new(None),
            steuerung: Arc::new(Steuerung::neu()),
            pegel: AtomicU8::new(0),
            laeuft: AtomicBool::new(false),
            geraete: Mutex::new((String::new(), String::new())),
        }
    }

    /// Startet Aufnahme und Wiedergabe mit den übergebenen Geräten.
    pub fn starten(
        &self,
        mikrofon: Option<String>,
        lautsprecher: Option<String>,
        einstellungen: Sendeeinstellungen,
    ) -> Result<(), String> {
        self.stoppen();
        let pfad = Pfad::starten(
            mikrofon.as_deref().filter(|s| !s.is_empty()),
            lautsprecher.as_deref().filter(|s| !s.is_empty()),
            einstellungen,
        )
        .map_err(|fehler| format!("{fehler:#}"))?;
        // Die Steuerung des Pfads ist die unsere: Stumm und Taste wirken sofort.
        if let Ok(mut p) = self.pfad.lock() {
            *p = Some(pfad);
        }
        if let Ok(mut g) = self.geraete.lock() {
            *g = (
                mikrofon.unwrap_or_default(),
                lautsprecher.unwrap_or_default(),
            );
        }
        self.laeuft.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Beendet den Pfad (Geräte zu, Kindprozesse zu).
    pub fn stoppen(&self) {
        if let Ok(mut p) = self.pfad.lock() {
            if let Some(pfad) = p.take() {
                pfad.stoppen();
            }
        }
        self.laeuft.store(false, Ordering::Relaxed);
        self.pegel.store(0, Ordering::Relaxed);
    }

    /// Läuft der Pfad?
    pub fn laeuft(&self) -> bool {
        self.laeuft.load(Ordering::Relaxed)
    }

    /// Neue Einstellungen an den laufenden Pfad (Schwelle, Gate, Vorlauf).
    pub fn einstellen(&self, einstellungen: Sendeeinstellungen) {
        if let Ok(p) = self.pfad.lock() {
            if let Some(pfad) = p.as_ref() {
                pfad.einstellen(einstellungen);
            }
        }
    }

    /// Übernimmt geänderte Einstellungen.
    ///
    /// Schwelle, Rauschunterdrückung und Noisegate wirken im laufenden Pfad.
    /// Ein **anderes Gerät** braucht einen Neustart — und nur dann wird
    /// neu gestartet, damit ein Regler nicht den Ton abreißen lässt.
    pub fn wende_an(
        &self,
        mikrofon: Option<String>,
        lautsprecher: Option<String>,
        einstellungen: Sendeeinstellungen,
    ) -> Result<(), String> {
        self.einstellen(einstellungen.clone());
        if !self.laeuft() {
            return Ok(());
        }
        let gewuenscht = (
            mikrofon.clone().unwrap_or_default(),
            lautsprecher.clone().unwrap_or_default(),
        );
        let aktuell = self.geraete.lock().map(|g| g.clone()).unwrap_or_default();
        if aktuell == gewuenscht {
            return Ok(());
        }
        self.starten(mikrofon, lautsprecher, einstellungen)
    }

    /// Stummschalten des eigenen Mikrofons.
    pub fn stumm_setzen(&self, an: bool) {
        self.steuerung.stumm_setzen(an);
    }

    /// Eigener Ton aus („taub").
    pub fn taub_setzen(&self, an: bool) {
        self.steuerung.taub_setzen(an);
    }

    /// Ist der eigene Ton aus?
    pub fn ist_taub(&self) -> bool {
        self.steuerung.ist_taub()
    }

    /// Ist das Mikrofon stumm?
    pub fn ist_stumm(&self) -> bool {
        self.steuerung.ist_stumm()
    }

    /// Push-to-Talk: Taste gedrückt oder losgelassen.
    pub fn taste_setzen(&self, an: bool) {
        self.steuerung.taste_setzen(an);
    }

    /// Push-to-Talk ein-/ausschalten (offenes Mikrofon als Gegenstück).
    pub fn push_to_talk_setzen(&self, an: bool) {
        self.steuerung.push_to_talk_setzen(an);
    }

    /// Mikrofonpegel 0–100 (frisch vom Pfad).
    pub fn pegel(&self) -> u8 {
        if let Ok(p) = self.pfad.lock() {
            if let Some(pfad) = p.as_ref() {
                let wert = pfad.pegel();
                self.pegel.store(wert, Ordering::Relaxed);
                return wert;
            }
        }
        self.pegel.load(Ordering::Relaxed)
    }

    /// Nächstes Datagramm für das Netz (vom Verbindungsthread abgeholt).
    pub fn naechstes_datagramm(&self) -> Option<Vec<u8>> {
        self.pfad
            .lock()
            .ok()
            .and_then(|p| p.as_ref().and_then(|pfad| pfad.naechstes_datagramm()))
    }

    /// Ein Datagramm aus dem Netz zur Wiedergabe.
    pub fn hoeren(&self, datagramm: Vec<u8>) {
        if let Ok(p) = self.pfad.lock() {
            if let Some(pfad) = p.as_ref() {
                pfad.hoeren(datagramm);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frischer_betrieb_laeuft_nicht_und_ist_nicht_stumm() {
        let b = Sprechbetrieb::neu();
        assert!(!b.laeuft());
        assert!(!b.ist_stumm());
        assert_eq!(b.pegel(), 0);
        assert!(b.naechstes_datagramm().is_none());
    }

    #[test]
    fn stumm_und_taste_gehen_auch_ohne_pfad() {
        let b = Sprechbetrieb::neu();
        b.stumm_setzen(true);
        assert!(b.ist_stumm());
        b.stumm_setzen(false);
        assert!(!b.ist_stumm());
        // Taste drücken ohne Pfad darf nicht panisch werden.
        b.taste_setzen(true);
        b.taste_setzen(false);
        b.hoeren(vec![1, 2, 3]);
        assert!(b.naechstes_datagramm().is_none());
    }

    #[test]
    fn stoppen_ist_wiederholbar() {
        let b = Sprechbetrieb::neu();
        b.stoppen();
        b.stoppen();
        assert!(!b.laeuft());
    }

    #[test]
    fn monitor_als_mikrofon_wird_abgelehnt() {
        let b = Sprechbetrieb::neu();
        let fehler = b
            .starten(
                Some("alsa_output.kopfhoerer.monitor".to_string()),
                None,
                Sendeeinstellungen::default(),
            )
            .expect_err("Monitor muss abgelehnt werden");
        assert!(fehler.contains("Monitor"), "{fehler}");
        assert!(!b.laeuft(), "nach dem Fehler läuft nichts");
    }

    #[test]
    fn ohne_geraet_wird_ein_fehler_gemeldet_statt_zu_panisch() {
        // Es gibt in dieser Umgebung kein Aufnahmegerät (CI/Container) oder
        // eines — beides darf nicht abstürzen.
        let b = Sprechbetrieb::neu();
        let ergebnis = b.starten(None, None, Sendeeinstellungen::default());
        match ergebnis {
            Ok(()) => b.stoppen(),
            Err(text) => assert!(!text.is_empty()),
        }
    }
}

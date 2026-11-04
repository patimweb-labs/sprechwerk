// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Soundkarte und Codec.
//!
//! **Schnitt:** dieses Crate holt Ton vom Gerät, kodiert mit Opus und gibt ihn
//! an den Aufrufer; in der Gegenrichtung dekodiert es und schreibt ihn auf das
//! Ausgabegerät. Was **wann** gesendet wird (Push-to-Talk, Vorlauf,
//! Jitter-Puffer, Schwelle) entscheidet `sprechwerk_adapters::sprechpfad` —
//! dort liegt die Logik und dort ist sie geprüft.
//!
//! **Bauvoraussetzungen** (siehe `docs/AUDIO.md`): ALSA-Entwicklungspaket,
//! `pkg-config`, `cmake` (für das gebündelte Opus). Ohne sie baut dieses Crate
//! nicht — deshalb steht es nicht im Arbeitsbereich.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Opus-Einstellungen. Mono, 20 ms Rahmen — wie im Protokoll festgelegt.
pub const ABTASTRATE: u32 = 48_000;
pub const KANAELE: u16 = 1;
pub const RAHMEN_MS: u32 = 20;
pub const BITRATE: i32 = 32_000;

/// Ein Gerät, wie es die Oberfläche anbietet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geraet {
    pub name: String,
    pub ist_vorgabe: bool,
}

/// Verfügbare Aufnahmegeräte.
pub fn aufnahmegeraete() -> Result<Vec<Geraet>> {
    let host = cpal::default_host();
    let vorgabe = host.default_input_device().and_then(|d| d.name().ok());
    let mut liste = Vec::new();
    for geraet in host.input_devices().context("Aufnahmegeräte")? {
        if let Ok(name) = geraet.name() {
            liste.push(Geraet {
                ist_vorgabe: Some(&name) == vorgabe.as_ref(),
                name,
            });
        }
    }
    Ok(liste)
}

/// Verfügbare Wiedergabegeräte.
pub fn wiedergabegeraete() -> Result<Vec<Geraet>> {
    let host = cpal::default_host();
    let vorgabe = host.default_output_device().and_then(|d| d.name().ok());
    let mut liste = Vec::new();
    for geraet in host.output_devices().context("Wiedergabegeräte")? {
        if let Ok(name) = geraet.name() {
            liste.push(Geraet {
                ist_vorgabe: Some(&name) == vorgabe.as_ref(),
                name,
            });
        }
    }
    Ok(liste)
}

/// Ein laufender Audiopfad.
pub struct Audiopfad {
    aufnahme: cpal::Stream,
    wiedergabe: cpal::Stream,
    /// Fertig kodierte Rahmen (20 ms) für den Netzteil.
    pub gesendet: Receiver<Vec<u8>>,
    /// Rahmen, die abgespielt werden sollen.
    pub zu_spielen: Sender<Vec<u8>>,
    /// Zuletzt gemessener Pegel 0–100 (für die Sprechschwelle).
    pub pegel: Arc<Mutex<u8>>,
}

impl Audiopfad {
    /// Startet Aufnahme und Wiedergabe.
    pub fn starten(mikrofon: Option<&str>, lautsprecher: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();

        // ---- Aufnahme ------------------------------------------------
        let eingang = match mikrofon.filter(|n| !n.is_empty()) {
            Some(name) => host
                .input_devices()?
                .find(|g| g.name().map(|n| n == name).unwrap_or(false))
                .with_context(|| format!("Mikrofon „{name}" nicht gefunden"))?,
            None => host.default_input_device().context("kein Mikrofon")?,
        };
        let eingerichtet = eingang.default_input_config()?;
        let eingang_rate = eingerichtet.sample_rate().0;
        let eingang_kanaele = eingerichtet.channels() as usize;

        let mut kodierer = audiopus::coder::Encoder::new(
            ABTASTRATE,
            audiopus::Channels::Mono,
            audiopus::Application::Voip,
        )?;
        kodierer.set_bitrate(audiopus::Bitrate::BitsPerSecond(BITRATE))?;

        let (sender, gesendet) = std::sync::mpsc::channel::<Vec<u8>>();
        let pegel = Arc::new(Mutex::new(0u8));
        let pegel_schreib = pegel.clone();

        // Ein Opus-Rahmen ist 20 ms; bei 48 kHz sind das 960 Abtastwerte.
        const RAHMEN_SAMPLES: usize = (ABTASTRATE as usize / 1000) * RAHMEN_MS as usize;
        let mut sammler: Vec<f32> = Vec::with_capacity(RAHMEN_SAMPLES);
        let mut zwischen: Vec<f32> = Vec::new();

        let aufnahme = eingang.build_input_stream(
            &eingerichtet.config(),
            move |daten: &[f32], _| {
                // Auf Mono heruntermischen und auf 48 kHz bringen (einfache
                // lineare Umrechnung; bei 48 kHz ist es ein Durchreichen).
                zwischen.clear();
                for rahmen in daten.chunks(eingang_kanaele) {
                    zwischen.push(rahmen.iter().sum::<f32>() / eingang_kanaele as f32);
                }
                let umgerechnet = umrechnen(&zwischen, eingang_rate, ABTASTRATE);
                sammler.extend_from_slice(&umgerechnet);

                // Pegel für die Sprechschwelle (einfacher Spitzenwert).
                if let Some(spitze) = sammler.last() {
                    let wert = (spitze.abs() * 100.0).clamp(0.0, 100.0) as u8;
                    if let Ok(mut p) = pegel_schreib.lock() {
                        *p = (*p * 3 + wert) / 4; // geglättet
                    }
                }

                while sammler.len() >= RAHMEN_SAMPLES {
                    let block: Vec<f32> = sammler.drain(..RAHMEN_SAMPLES).collect();
                    let pcm: Vec<i16> = block
                        .iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();
                    let mut ziel = vec![0u8; 4000];
                    if let Ok(len) = kodierer.encode(&pcm, &mut ziel) {
                        ziel.truncate(len);
                        let _ = sender.send(ziel);
                    }
                }
            },
            |fehler| eprintln!("Aufnahme: {fehler}"),
            None,
        )?;
        aufnahme.play()?;

        // ---- Wiedergabe ----------------------------------------------
        let ausgang = match lautsprecher.filter(|n| !n.is_empty()) {
            Some(name) => host
                .output_devices()?
                .find(|g| g.name().map(|n| n == name).unwrap_or(false))
                .with_context(|| format!("Wiedergabe „{name}" nicht gefunden"))?,
            None => host.default_output_device().context("keine Wiedergabe")?,
        };
        let aus_config = ausgang.default_output_config()?;
        let (zu_spielen, empfangen) = std::sync::mpsc::channel::<Vec<u8>>();

        let mut dekodierer = audiopus::coder::Decoder::new(ABTASTRATE, audiopus::Channels::Mono)?;
        let mut warteschlange: Vec<f32> = Vec::new();
        let mut pcm_puffer = vec![0i16; 5760];

        let wiedergabe = ausgang.build_output_stream(
            &aus_config.config(),
            move |ziel: &mut [f32], _| {
                // Alles abholen, was in der Zwischenzeit kam, dekodieren und
                // in die Warteschlange legen.
                while let Ok(rahmen) = empfangen.try_recv() {
                    if let Ok(len) = dekodierer.decode(&rahmen, &mut pcm_puffer, false) {
                        warteschlange.extend(
                            pcm_puffer[..len].iter().map(|s| *s as f32 / i16::MAX as f32),
                        );
                    }
                }
                for (i, platz) in ziel.iter_mut().enumerate() {
                    *platz = warteschlange.get(i).copied().unwrap_or(0.0);
                }
                // Verbrauchtes entfernen; bei Unterlauf bleibt Stille.
                let verbraucht = ziel.len().min(warteschlange.len());
                warteschlange.drain(..verbraucht);
            },
            |fehler| eprintln!("Wiedergabe: {fehler}"),
            None,
        )?;
        wiedergabe.play()?;

        Ok(Self {
            aufnahme,
            wiedergabe,
            gesendet,
            zu_spielen,
            pegel,
        })
    }

    /// Aktueller Pegel 0–100.
    pub fn pegel(&self) -> u8 {
        self.pegel.lock().map(|p| *p).unwrap_or(0)
    }

    /// Beendet beide Ströme.
    pub fn stoppen(self) {
        let _ = self.aufnahme.pause();
        let _ = self.wiedergabe.pause();
    }
}

/// Wandelt eine Abtastrate in eine andere (linear, für Sprachsignale genügend).
fn umrechnen(eingang: &[f32], von: u32, nach: u32) -> Vec<f32> {
    if von == nach {
        return eingang.to_vec();
    }
    let verhaeltnis = nach as f64 / von as f64;
    let laenge = (eingang.len() as f64 * verhaeltnis).round() as usize;
    let mut aus = Vec::with_capacity(laenge);
    for i in 0..laenge {
        let pos = i as f64 / verhaeltnis;
        let index = pos.floor() as usize;
        let rest = (pos - index as f64) as f32;
        let a = eingang.get(index).copied().unwrap_or(0.0);
        let b = eingang.get(index + 1).copied().unwrap_or(a);
        aus.push(a + (b - a) * rest);
    }
    aus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gleiche_rate_wird_durchgereicht() {
        let daten = vec![0.1, 0.2, 0.3];
        assert_eq!(umrechnen(&daten, 48_000, 48_000), daten);
    }

    #[test]
    fn verdoppelte_rate_verdoppelt_die_laenge() {
        let daten = vec![0.0, 1.0, 0.0, 1.0];
        let aus = umrechnen(&daten, 24_000, 48_000);
        assert_eq!(aus.len(), 8);
        assert!((aus[1] - 0.5).abs() < 0.01, "Zwischenwert wird eingefügt");
    }
}

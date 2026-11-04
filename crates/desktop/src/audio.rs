// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Geräte und Ton-Test über die Systemwerkzeuge von PipeWire/PulseAudio.
//!
//! **Warum nicht `cpal`/`libopus`?** Das wäre die richtige Wahl für den
//! Audiopfad — braucht aber Entwicklungspakete (`alsa-lib-devel`, `cmake`) und
//! damit einen Bau, der hier nicht läuft. Für **Geräteliste und Test** genügen
//! die mitgelieferten Werkzeuge (`pactl`, `parec`, `paplay`): sie sprechen mit
//! demselben Server, kosten keine Abhängigkeit, und ein Test ist kein
//! Dauerbetrieb.
//!
//! Der eigentliche Sprachpfad kommt später über `crates/audio` (Opus); dieses
//! Modul ist ausdrücklich nur Auswahl und Prüfung.

use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Ein wählbares Gerät.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Geraet {
    /// Technischer Name (`alsa_input.…`, `alsa_output.…`).
    pub id: String,
    /// Lesbare Beschreibung (aus `pactl list …` bzw. der Kennung abgeleitet).
    pub name: String,
    /// Ist es die Vorgabe des Systems?
    pub vorgabe: bool,
}

/// Beide Listen für die Oberfläche.
#[derive(Debug, Clone, Serialize)]
pub struct Geraeteliste {
    pub mikrofon: Vec<Geraet>,
    pub lautsprecher: Vec<Geraet>,
}

/// Fehler, die der Oberfläche als Meldung angezeigt werden.
#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("Audio-Werkzeuge fehlen (pactl/parec/paplay) — ist PipeWire oder PulseAudio installiert?")]
    Werkzeug,
    #[error("Gerät nicht gefunden: {0}")]
    Geraet(String),
    #[error("Messung fehlgeschlagen: {0}")]
    Messung(String),
}

/// Sind die Werkzeuge vorhanden?
fn werkzeug(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Liest die Geräteliste von `pactl`.
fn liste(art: &str) -> Result<Vec<Geraet>, Fehler> {
    if !werkzeug("pactl") {
        return Err(Fehler::Werkzeug);
    }
    // `pactl list short <art>`: Nummer, Name, Treiber, Format, Zustand.
    let ausgabe = Command::new("pactl")
        .args(["list", "short", art])
        .output()
        .map_err(|_| Fehler::Werkzeug)?;
    if !ausgabe.status.success() {
        return Err(Fehler::Messung("pactl list".into()));
    }
    let text = String::from_utf8_lossy(&ausgabe.stdout);
    let mut geraete = Vec::new();
    for zeile in text.lines() {
        let felder: Vec<&str> = zeile.split('\t').collect();
        if felder.len() < 2 {
            continue;
        }
        let id = felder[1].trim().to_string();
        // Monitore sind keine Aufnahmegeräte für Sprache — sie zeigen, was aus
        // der Karte herauskommt.
        if art == "sources" && id.ends_with(".monitor") {
            continue;
        }
        geraete.push(Geraet {
            name: lesbar(&id),
            id,
            vorgabe: false,
        });
    }
    Ok(geraete)
}

/// Macht aus `alsa_input.usb-Sennheiser_…-00.mono-fallback` etwas Lesbares.
fn lesbar(id: &str) -> String {
    let ohne_praefix = id
        .strip_prefix("alsa_input.")
        .or_else(|| id.strip_prefix("alsa_output."))
        .unwrap_or(id);
    // Bis zum Hersteller-Teil: „usb-Sennheiser_Sennheiser_Profile_…" → „Sennheiser"
    let kern = ohne_praefix.split('.').next().unwrap_or(ohne_praefix);
    let teile: Vec<&str> = kern.split('-').collect();
    let name = if teile.len() > 1 && teile[0] == "usb" {
        teile.get(1).copied().unwrap_or(kern)
    } else {
        teile.first().copied().unwrap_or(kern)
    };
    name.replace('_', " ").trim().to_string()
}

/// Vorgabegerät laut `pactl get-default-…`.
fn vorgabe(art: &str) -> Option<String> {
    let ausgabe = Command::new("pactl").args(["get-default", art]).output().ok()?;
    if !ausgabe.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Aufnahmegeräte.
pub fn mikrofon_geraete() -> Result<Vec<Geraet>, Fehler> {
    let mut liste = liste("sources")?;
    let vorgabe = vorgabe("source");
    for g in liste.iter_mut() {
        g.vorgabe = vorgabe.as_deref() == Some(g.id.as_str());
    }
    Ok(liste)
}

/// Wiedergabegeräte.
pub fn lautsprecher_geraete() -> Result<Vec<Geraet>, Fehler> {
    let mut liste = liste("sinks")?;
    let vorgabe = vorgabe("sink");
    for g in liste.iter_mut() {
        g.vorgabe = vorgabe.as_deref() == Some(g.id.as_str());
    }
    Ok(liste)
}

/// Schreibt einen kurzen Prüfton (440 Hz, 0,7 s) als WAV in eine Datei.
fn testton_datei() -> Result<std::path::PathBuf, Fehler> {
    let pfad = std::env::temp_dir().join("sprechwerk-testton.wav");
    let rate = 48_000u32;
    let dauer = 0.7f32;
    let anzahl = (rate as f32 * dauer) as u32;
    let mut daten: Vec<u8> = Vec::with_capacity(anzahl as usize * 2);
    for i in 0..anzahl {
        let t = i as f32 / rate as f32;
        // Weiche Hüllkurve, damit es nicht knackt.
        let huelle = (1.0 - (t / dauer)).min(1.0).min(t * 20.0);
        let wert = (t * 440.0 * std::f32::consts::TAU).sin() * 0.25 * huelle;
        let pcm = (wert * i16::MAX as f32) as i16;
        daten.extend_from_slice(&pcm.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + daten.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + daten.len()) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(daten.len() as u32).to_le_bytes());
    wav.extend_from_slice(&daten);
    std::fs::write(&pfad, wav).map_err(|e| Fehler::Messung(format!("Testton schreiben: {e}")))?;
    Ok(pfad)
}

/// Spielt einen Prüfton auf dem gewählten Gerät.
pub fn testton(geraet: &str) -> Result<(), Fehler> {
    if !werkzeug("paplay") {
        return Err(Fehler::Werkzeug);
    }
    let datei = testton_datei()?;
    let mut befehl = Command::new("paplay");
    if !geraet.is_empty() {
        befehl.arg(format!("--device={geraet}"));
    }
    let status = befehl
        .arg(&datei)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| Fehler::Werkzeug)?;
    if !status.success() {
        return Err(Fehler::Geraet(geraet.to_string()));
    }
    Ok(())
}

/// Misst den Eingangspegel für `millisekunden` und liefert 0–100.
///
/// Aufgenommen wird rohes PCM (16 Bit, mono, 16 kHz) über `parec`; gerechnet
/// wird der Effektivwert. Das ist genau das, was eine Sprechschwelle braucht.
pub fn pegel(geraet: &str, millisekunden: u64) -> Result<u8, Fehler> {
    if !werkzeug("parec") {
        return Err(Fehler::Werkzeug);
    }
    let mut befehl = Command::new("parec");
    befehl.args([
        "--format=s16le",
        "--rate=16000",
        "--channels=1",
        "--raw",
    ]);
    if !geraet.is_empty() {
        befehl.arg(format!("--device={geraet}"));
    }
    let mut kind = befehl
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Fehler::Werkzeug)?;

    let mut puffer = [0u8; 4096];
    let mut summe: f64 = 0.0;
    let mut anzahl: u64 = 0;
    let frist = Instant::now() + Duration::from_millis(millisekunden);
    if let Some(mut aus) = kind.stdout.take() {
        while Instant::now() < frist {
            match aus.read(&mut puffer) {
                Ok(0) => break,
                Ok(n) => {
                    for paar in puffer[..n].chunks_exact(2) {
                        let wert = i16::from_le_bytes([paar[0], paar[1]]) as f64 / i16::MAX as f64;
                        summe += wert * wert;
                        anzahl += 1;
                    }
                }
                Err(_) => break,
            }
        }
    }
    let _ = kind.kill();
    let _ = kind.wait();
    if anzahl == 0 {
        return Err(Fehler::Messung("keine Daten vom Mikrofon".into()));
    }
    let rms = (summe / anzahl as f64).sqrt();
    // -60 dB bis 0 dB auf 0–100 abbilden (Sprache liegt selten über -6 dB).
    let db = 20.0 * rms.max(1e-6).log10();
    let anteil = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    Ok((anteil * 100.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lesbare_namen() {
        assert_eq!(
            lesbar("alsa_input.usb-Sennheiser_Sennheiser_Profile_00.mono-fallback"),
            "Sennheiser"
        );
        assert_eq!(lesbar("alsa_input.pci-0000_0f_00.6.analog-stereo"), "pci");
        assert_eq!(lesbar("alsa_output.hdmi-stereo"), "hdmi");
    }

    #[test]
    fn testton_ist_gueltiges_wav() {
        let pfad = testton_datei().expect("Testton");
        let daten = std::fs::read(&pfad).expect("lesen");
        assert_eq!(&daten[..4], b"RIFF");
        assert_eq!(&daten[8..12], b"WAVE");
        assert!(daten.len() > 44, "Kopfdaten plus Ton");
        // 48 kHz, 0,7 s, 16 Bit mono ≈ 67 200 Byte Nutzlast.
        assert!(daten.len() > 60_000 && daten.len() < 70_000, "{}", daten.len());
        let _ = std::fs::remove_file(pfad);
    }
}

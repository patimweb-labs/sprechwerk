//! Umrechnung und Dateien: i16 ↔ f32, Pegel, WAV.
//!
//! Kein Gerät, keine Zeit, kein Netz — nur Rechnerei, deshalb prüfbar.

use std::io::Write;
use std::path::Path;

/// Abtastrate des Sprachpfads (48 kHz, wie das Protokoll es festlegt).
pub const ABTASTRATE: u32 = 48_000;
/// Abtastwerte je 20-ms-Rahmen.
pub const RAHMEN_SAMPLES: usize = 960;
/// Bytes je 20-ms-Rahmen in 16-Bit-Mono.
pub const RAHMEN_BYTES: usize = RAHMEN_SAMPLES * 2;

/// Wandelt 16-Bit-Werte nach -1.0 … 1.0.
pub fn nach_f32(werte: &[i16]) -> Vec<f32> {
    werte.iter().map(|w| *w as f32 / 32768.0).collect()
}

/// Wandelt -1.0 … 1.0 nach 16-Bit (mit Begrenzung).
pub fn nach_i16(werte: &[f32]) -> Vec<i16> {
    werte
        .iter()
        .map(|w| (w.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

/// Liest rohe 16-Bit-Mono-Bytes als Werte (kleine Ende, wie `parec` liefert).
pub fn bytes_nach_i16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

/// Schreibt Werte als rohe 16-Bit-Mono-Bytes (für `pacat`).
pub fn i16_nach_bytes(werte: &[i16]) -> Vec<u8> {
    let mut aus = Vec::with_capacity(werte.len() * 2);
    for w in werte {
        aus.extend_from_slice(&w.to_le_bytes());
    }
    aus
}

/// Mischt einen Rahmen auf eine Summe (additiv, mit Begrenzung).
///
/// Mehrere Sprecher gleichzeitig müssen hörbar bleiben: addieren und erst am
/// Ende begrenzen wäre bei zwei lauten Stimmen Übersteuerung. Deshalb wird hier
/// mit einer weichen Begrenzung gearbeitet.
pub fn dazu_mischen(summe: &mut [i16], neu: &[i16]) {
    for (ziel, wert) in summe.iter_mut().zip(neu.iter()) {
        let gemischt = *ziel as f32 / 32768.0 + *wert as f32 / 32768.0;
        // Weiche Begrenzung (tanh) statt hartem Abschneiden — klingt weniger
        // schroff und verhindert Übersteuerung bei mehreren Sprechern.
        let begrenzt = gemischt.tanh();
        *ziel = (begrenzt * i16::MAX as f32) as i16;
    }
}

/// Schreibt eine WAV-Datei (16-Bit-Mono, 48 kHz).
pub fn wav_schreiben(pfad: impl AsRef<Path>, werte: &[i16]) -> std::io::Result<()> {
    let daten_bytes = (werte.len() * 2) as u32;
    let mut datei = std::fs::File::create(pfad)?;
    datei.write_all(b"RIFF")?;
    datei.write_all(&(36 + daten_bytes).to_le_bytes())?;
    datei.write_all(b"WAVEfmt ")?;
    datei.write_all(&16u32.to_le_bytes())?; // Blocklänge
    datei.write_all(&1u16.to_le_bytes())?; // PCM
    datei.write_all(&1u16.to_le_bytes())?; // Mono
    datei.write_all(&ABTASTRATE.to_le_bytes())?;
    datei.write_all(&(ABTASTRATE * 2).to_le_bytes())?; // Bytes je Sekunde
    datei.write_all(&2u16.to_le_bytes())?; // Blockausrichtung
    datei.write_all(&16u16.to_le_bytes())?; // Bits je Wert
    datei.write_all(b"data")?;
    datei.write_all(&daten_bytes.to_le_bytes())?;
    datei.write_all(&i16_nach_bytes(werte))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umrechnung_ist_umkehrbar() {
        let werte = vec![0i16, 1000, -1000, i16::MAX, i16::MIN + 1];
        let hin = nach_f32(&werte);
        let zurueck = nach_i16(&hin);
        for (a, b) in werte.iter().zip(zurueck.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} → {b}");
        }
    }

    #[test]
    fn bytes_sind_kleine_ende() {
        let werte = vec![1i16, 258];
        let bytes = i16_nach_bytes(&werte);
        assert_eq!(bytes, vec![1, 0, 2, 1]);
        assert_eq!(bytes_nach_i16(&bytes), werte);
    }

    #[test]
    fn mischen_begrenzt_statt_abzuschneiden() {
        let mut summe = vec![30000i16; 4];
        let neu = vec![30000i16; 4];
        dazu_mischen(&mut summe, &neu);
        // 0.92 + 0.92 = 1.84 → weich begrenzt auf rund 0.95 (Faktor 1.04),
        // nicht hart auf 1.0 abgeschnitten (das wäre 32767).
        assert!(
            summe[0] > 30_000,
            "lauter als eine Stimme allein: {}",
            summe[0]
        );
        assert!(
            summe[0] < 32_000,
            "weiche Begrenzung statt Anschlag: {}",
            summe[0]
        );
    }

    #[test]
    fn wav_hat_richtigen_kopf() {
        let pfad = std::env::temp_dir().join("sprechwerk-wav-test.wav");
        wav_schreiben(&pfad, &[0i16; 480]).unwrap();
        let inhalt = std::fs::read(&pfad).unwrap();
        assert_eq!(&inhalt[0..4], b"RIFF");
        assert_eq!(&inhalt[8..12], b"WAVE");
        assert_eq!(inhalt.len(), 44 + 960);
        assert_eq!(
            u32::from_le_bytes(inhalt[24..28].try_into().unwrap()),
            48_000
        );
        let _ = std::fs::remove_file(pfad);
    }
}

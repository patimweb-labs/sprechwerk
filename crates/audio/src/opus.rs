//! Opus über die Systembibliothek `libopus.so.0`.
//!
//! **Warum eigene Bindung?** Das Crate `audiopus` baut Opus aus Quellen mit
//! `cmake` — eine Werkzeugkette, die nicht überall vorhanden ist. `libopus` ist
//! dagegen auf jedem System mit PipeWire/PulseAudio installiert (es ist die
//! Referenzimplementierung, gegen die alles andere prüft). Der Linker bekommt
//! deshalb den **genauen Dateinamen** (`+verbatim`), damit kein
//! Entwicklungspaket mit `libopus.so`-Verknüpfung nötig ist.
//!
//! Mono, 48 000 Hz, 20-ms-Rahmen (960 Abtastwerte) — wie im Protokoll
//! festgelegt (`docs/AUDIO.md`).

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::os::raw::c_int;

/// Abtastrate des Sprachpfads.
pub const ABTASTRATE: u32 = 48_000;
/// Abtastwerte je 20-ms-Rahmen.
pub const RAHMEN_SAMPLES: usize = (ABTASTRATE as usize / 1000) * 20;
/// Angestrebte Bitrate.
pub const BITRATE: i32 = 32_000;

// Anwendungsprofile von Opus.
const OPUS_APPLICATION_VOIP: c_int = 2048;

// `opus_encoder_ctl`/`opus_decoder_ctl` sind variadisch; die Anfragenummern
// stehen in `opus_defines.h`.
const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;
const OPUS_SET_INBAND_FEC_REQUEST: c_int = 4012;
const OPUS_SET_PACKET_LOSS_PERC_REQUEST: c_int = 4014;
const OPUS_SET_DTX_REQUEST: c_int = 4016;
const OPUS_SET_SIGNAL_REQUEST: c_int = 4024;
const OPUS_SIGNAL_VOICE: c_int = 3001;

#[link(name = "libopus.so.0", kind = "dylib", modifiers = "+verbatim")]
extern "C" {
    fn opus_encoder_create(
        fs: i32,
        channels: c_int,
        application: c_int,
        error: *mut c_int,
    ) -> *mut c_void;
    fn opus_encoder_destroy(st: *mut c_void);
    fn opus_encode(
        st: *mut c_void,
        pcm: *const i16,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> c_int;
    fn opus_encoder_ctl(st: *mut c_void, request: c_int, ...) -> c_int;

    fn opus_decoder_create(fs: i32, channels: c_int, error: *mut c_int) -> *mut c_void;
    fn opus_decoder_destroy(st: *mut c_void);
    fn opus_decode(
        st: *mut c_void,
        data: *const u8,
        len: i32,
        pcm: *mut i16,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
}

/// Ein Opus-Kodierer (ein Sprecher).
pub struct Kodierer {
    st: *mut c_void,
}

// Der Kodierer wird nur von einem Thread benutzt; `Send` ist trotzdem nötig,
// damit er im Aufnahmethread leben darf.
unsafe impl Send for Kodierer {}

impl Kodierer {
    pub fn neu() -> Result<Self> {
        let mut fehler: c_int = 0;
        let st = unsafe {
            opus_encoder_create(
                ABTASTRATE as i32,
                1,
                OPUS_APPLICATION_VOIP,
                &mut fehler,
            )
        };
        if st.is_null() || fehler != 0 {
            return Err(anyhow!("opus_encoder_create: Fehler {fehler}"));
        }
        let this = Self { st };
        unsafe {
            opus_encoder_ctl(this.st, OPUS_SET_BITRATE_REQUEST, BITRATE);
            opus_encoder_ctl(this.st, OPUS_SET_COMPLEXITY_REQUEST, 6 as c_int);
            // Verlust ist im Netz normal: FEC und DTX helfen beiden Seiten.
            opus_encoder_ctl(this.st, OPUS_SET_INBAND_FEC_REQUEST, 1 as c_int);
            opus_encoder_ctl(this.st, OPUS_SET_PACKET_LOSS_PERC_REQUEST, 8 as c_int);
            opus_encoder_ctl(this.st, OPUS_SET_DTX_REQUEST, 0 as c_int);
            opus_encoder_ctl(this.st, OPUS_SET_SIGNAL_REQUEST, OPUS_SIGNAL_VOICE);
        }
        Ok(this)
    }

    /// Kodiert genau 960 Abtastwerte zu einem Rahmen.
    pub fn kodiere(&mut self, pcm: &[i16], ziel: &mut [u8]) -> Result<usize> {
        if pcm.len() != RAHMEN_SAMPLES {
            return Err(anyhow!(
                "Opus braucht {} Abtastwerte je Rahmen, bekommen {}",
                RAHMEN_SAMPLES,
                pcm.len()
            ));
        }
        let len = unsafe {
            opus_encode(
                self.st,
                pcm.as_ptr(),
                RAHMEN_SAMPLES as c_int,
                ziel.as_mut_ptr(),
                ziel.len() as i32,
            )
        };
        if len < 0 {
            return Err(anyhow!("opus_encode: Fehler {len}"));
        }
        Ok(len as usize)
    }
}

impl Drop for Kodierer {
    fn drop(&mut self) {
        unsafe { opus_encoder_destroy(self.st) }
    }
}

/// Ein Opus-Dekodierer — **je Sprecher einer**.
pub struct Dekodierer {
    st: *mut c_void,
}

unsafe impl Send for Dekodierer {}

impl Dekodierer {
    pub fn neu() -> Result<Self> {
        let mut fehler: c_int = 0;
        let st = unsafe { opus_decoder_create(ABTASTRATE as i32, 1, &mut fehler) };
        if st.is_null() || fehler != 0 {
            return Err(anyhow!("opus_decoder_create: Fehler {fehler}"));
        }
        Ok(Self { st })
    }

    /// Dekodiert einen Rahmen nach `pcm` (mindestens 960 Werte). Liefert die
    /// Anzahl der Abtastwerte.
    pub fn dekodiere(&mut self, rahmen: &[u8], pcm: &mut [i16]) -> Result<usize> {
        let len = unsafe {
            opus_decode(
                self.st,
                rahmen.as_ptr(),
                rahmen.len() as i32,
                pcm.as_mut_ptr(),
                RAHMEN_SAMPLES as c_int,
                0,
            )
        };
        if len < 0 {
            return Err(anyhow!("opus_decode: Fehler {len}"));
        }
        Ok(len as usize)
    }

    /// Verlorener Rahmen: Opus setzt ihn aus dem Vorherigen fort
    /// („packet loss concealment"). Besser als Stille — Stille knackt.
    pub fn verschollen(&mut self, pcm: &mut [i16]) -> Result<usize> {
        let len = unsafe {
            opus_decode(
                self.st,
                std::ptr::null(),
                0,
                pcm.as_mut_ptr(),
                RAHMEN_SAMPLES as c_int,
                0,
            )
        };
        if len < 0 {
            return Err(anyhow!("opus_decode (Verlust): Fehler {len}"));
        }
        Ok(len as usize)
    }
}

impl Drop for Dekodierer {
    fn drop(&mut self) {
        unsafe { opus_decoder_destroy(self.st) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sinus(anzahl: usize, hz: f32, amplitude: f32) -> Vec<i16> {
        (0..anzahl)
            .map(|i| {
                let wert =
                    amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / ABTASTRATE as f32).sin();
                (wert * i16::MAX as f32) as i16
            })
            .collect()
    }

    #[test]
    fn kodieren_und_dekodieren_erhaelt_den_ton() {
        let mut k = Kodierer::neu().expect("Kodierer");
        let mut d = Dekodierer::neu().expect("Dekodierer");
        let mut puffer = vec![0u8; 4000];
        let pcm = sinus(RAHMEN_SAMPLES, 440.0, 0.5);

        // Der erste Rahmen ist bei Opus immer etwas ungenau (Anlauf).
        for _ in 0..3 {
            let len = k.kodiere(&pcm, &mut puffer).expect("kodieren");
            assert!(len > 0 && len < 1200, "Rahmenlänge {len}");
            let mut zurueck = vec![0i16; RAHMEN_SAMPLES];
            let geholt = d.dekodiere(&puffer[..len], &mut zurueck).expect("dekodieren");
            assert_eq!(geholt, RAHMEN_SAMPLES);
        }

        // Energie muss ankommen (Großsignal, kein Bitgenauigkeitsvergleich).
        let mut zurueck = vec![0i16; RAHMEN_SAMPLES];
        let len = k.kodiere(&pcm, &mut puffer).unwrap();
        d.dekodiere(&puffer[..len], &mut zurueck).unwrap();
        let energie: f32 =
            zurueck.iter().map(|s| (*s as f32).powi(2)).sum::<f32>() / RAHMEN_SAMPLES as f32;
        assert!(energie.sqrt() > 1000.0, "Signal kommt zu leise an: {energie}");
    }

    #[test]
    fn rahmen_passt_in_ein_datagramm() {
        let mut k = Kodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        let pcm = sinus(RAHMEN_SAMPLES, 200.0, 0.9);
        for _ in 0..5 {
            let len = k.kodiere(&pcm, &mut puffer).unwrap();
            assert!(
                len + sprechwerk_protocol::SPRACHKOPF_LEN
                    <= sprechwerk_protocol::MAX_SPRACHRAHMEN,
                "Rahmen {len} Byte passt nicht ins Datagramm"
            );
        }
    }

    #[test]
    fn falsche_rahmenlaenge_wird_abgelehnt() {
        let mut k = Kodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        assert!(k.kodiere(&[0i16; 10], &mut puffer).is_err());
    }

    #[test]
    fn verlust_wird_ueberbrueckt() {
        let mut d = Dekodierer::neu().unwrap();
        let mut pcm = vec![0i16; RAHMEN_SAMPLES];
        // Ohne Vorgeschichte liefert die Verdeckung Stille, aber keinen Fehler.
        assert_eq!(d.verschollen(&mut pcm).unwrap(), RAHMEN_SAMPLES);
    }
}

//! Opus — zur **Laufzeit** geladen, nicht fest gelinkt.
//!
//! # Warum das so ist
//!
//! Festes Linken bindet die Binärdatei an eine Plattform: unter Linux heißt die
//! Bibliothek `libopus.so.0`, unter macOS `libopus.dylib`, unter Windows
//! `opus.dll`. Ein `#[link(name = …)]` kann immer nur eines davon. Deshalb wird
//! die Bibliothek beim ersten Gebrauch gesucht und ihre Funktionszeiger geholt
//! (`libloading`). Das hat drei Folgen, alle gewollt:
//!
//! 1. **Eine Binärdatei je Plattform, gleicher Quelltext.** Kein `cfg`-Geflecht
//!    in der Audioligik.
//! 2. **Die Bibliothek darf mitgeliefert werden.** Liegt `opus.dll` neben der
//!    Anwendung, wird sie gefunden — der Nutzer muss nichts installieren.
//! 3. **Fehlt sie, gibt es eine verständliche Meldung** statt eines
//!    Linker-Fehlers beim Bauen.
//!
//! Mono, 48 000 Hz, 20-ms-Rahmen (960 Abtastwerte) — wie im Protokoll
//! festgelegt (`docs/AUDIO.md`).

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::OnceLock;

/// Abtastrate des Sprachpfads.
pub const ABTASTRATE: u32 = 48_000;
/// Abtastwerte je 20-ms-Rahmen.
pub const RAHMEN_SAMPLES: usize = (ABTASTRATE as usize / 1000) * 20;
/// Angestrebte Bitrate.
pub const BITRATE: i32 = 32_000;

// Anwendungsprofile von Opus.
const OPUS_APPLICATION_VOIP: c_int = 2048;

// Anfragenummern aus `opus_defines.h`.
const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;
const OPUS_SET_INBAND_FEC_REQUEST: c_int = 4012;
const OPUS_SET_PACKET_LOSS_PERC_REQUEST: c_int = 4014;
const OPUS_SET_DTX_REQUEST: c_int = 4016;
const OPUS_SET_SIGNAL_REQUEST: c_int = 4024;
const OPUS_SIGNAL_VOICE: c_int = 3001;

type EncoderCreate = unsafe extern "C" fn(i32, c_int, c_int, *mut c_int) -> *mut c_void;
type EncoderDestroy = unsafe extern "C" fn(*mut c_void);
type Encode = unsafe extern "C" fn(*mut c_void, *const i16, c_int, *mut u8, i32) -> c_int;
type EncoderCtl = unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int;
type DecoderCreate = unsafe extern "C" fn(i32, c_int, *mut c_int) -> *mut c_void;
type DecoderDestroy = unsafe extern "C" fn(*mut c_void);
type Decode = unsafe extern "C" fn(*mut c_void, *const u8, i32, *mut i16, c_int, c_int) -> c_int;

/// Die geladene Bibliothek samt Funktionszeigern.
struct Bibliothek {
    /// Muss am Leben bleiben, solange Zeiger daraus benutzt werden.
    _griff: libloading::Library,
    encoder_create: EncoderCreate,
    encoder_destroy: EncoderDestroy,
    encode: Encode,
    encoder_ctl: EncoderCtl,
    decoder_create: DecoderCreate,
    decoder_destroy: DecoderDestroy,
    decode: Decode,
}

// Die Zeiger sind Funktionsadressen in einer Bibliothek, die während der ganzen
// Laufzeit geladen bleibt — der Zugriff ist damit nebenläufig unbedenklich.
unsafe impl Send for Bibliothek {}
unsafe impl Sync for Bibliothek {}

/// Dateinamen, unter denen Opus je Plattform gesucht wird (in dieser Reihenfolge).
const NAMEN: &[&str] = &[
    // Linux
    "libopus.so.0",
    "libopus.so",
    // macOS
    "libopus.0.dylib",
    "libopus.dylib",
    // Windows
    "opus.dll",
    "libopus-0.dll",
    "libopus.dll",
];

fn bibliothek() -> Result<&'static Bibliothek> {
    static GELADEN: OnceLock<Result<Bibliothek, String>> = OnceLock::new();
    let ergebnis = GELADEN.get_or_init(|| laden().map_err(|fehler| fehler.to_string()));
    match ergebnis {
        Ok(b) => Ok(b),
        Err(text) => Err(anyhow!("{text}")),
    }
}

/// Wo Opus gesucht wird: erst **neben dem Programm**, dann über den Namen.
///
/// Der bloße Name reicht nicht überall:
/// * **macOS** sucht `dlopen("libopus.0.dylib")` weder im App-Bundle
///   (`Sprechwerk.app/Contents/Frameworks`, wohin das Paket Opus legt) noch
///   unter Homebrew auf Apple Silicon (`/opt/homebrew/lib`). Liegt Homebrew
///   woanders, nennt `HOMEBREW_PREFIX` (gesetzt von `brew shellenv`) den Ort.
/// * **Windows** fände `opus.dll` neben der `.exe` zwar auch über den Namen —
///   der volle Pfad schließt aber aus, dass eine fremde `opus.dll` aus dem
///   aktuellen Verzeichnis zuerst geladen wird.
///
/// Rein (keine Dateisystemzugriffe), damit die Reihenfolge prüfbar ist.
fn kandidaten(
    programm_ordner: Option<&std::path::Path>,
    homebrew: Option<&std::path::Path>,
) -> Vec<std::ffi::OsString> {
    let mut liste = Vec::new();
    if let Some(ordner) = programm_ordner {
        for name in ["opus.dll", "libopus.0.dylib", "libopus.so.0"] {
            liste.push(ordner.join(name).into_os_string());
        }
        // macOS-App: Contents/MacOS/<programm> → Contents/Frameworks/
        liste.push(
            ordner
                .join("../Frameworks/libopus.0.dylib")
                .into_os_string(),
        );
    }
    liste.extend(NAMEN.iter().map(std::ffi::OsString::from));
    // Homebrew: eigenes Präfix, dann Apple Silicon und Intel.
    if let Some(praefix) = homebrew {
        liste.push(praefix.join("lib/libopus.0.dylib").into_os_string());
    }
    for pfad in [
        "/opt/homebrew/lib/libopus.0.dylib",
        "/usr/local/lib/libopus.0.dylib",
    ] {
        liste.push(pfad.into());
    }
    liste
}

fn laden() -> Result<Bibliothek> {
    let mut letzter = String::new();
    let programm = std::env::current_exe().ok();
    let ordner = programm.as_deref().and_then(std::path::Path::parent);
    let homebrew = std::env::var_os("HOMEBREW_PREFIX").map(std::path::PathBuf::from);
    for name in kandidaten(ordner, homebrew.as_deref()) {
        // Nicht vorhandene Pfade gar nicht erst versuchen: sonst nennt die
        // Fehlermeldung am Ende einen Pfad, den es nie gab, statt des Namens.
        if std::path::Path::new(&name).is_absolute() && !std::path::Path::new(&name).exists() {
            continue;
        }
        let name = name.as_os_str();
        // Sicherheitsanforderung: `Library::new` ist unsicher, weil beliebiger
        // Code im Prozess landen kann. Hier werden nur feste Namen und Pfade
        // neben dem eigenen Programm geladen — keine Eingabe von außen.
        match unsafe { libloading::Library::new(name) } {
            Ok(griff) => {
                // Sicherheitsanforderung: Die Signaturen unten sind die der
                // veröffentlichten Opus-API (`opus.h`), unverändert.
                unsafe {
                    let encoder_create = *griff.get(b"opus_encoder_create\0")?;
                    let encoder_destroy = *griff.get(b"opus_encoder_destroy\0")?;
                    let encode = *griff.get(b"opus_encode\0")?;
                    let encoder_ctl = *griff.get(b"opus_encoder_ctl\0")?;
                    let decoder_create = *griff.get(b"opus_decoder_create\0")?;
                    let decoder_destroy = *griff.get(b"opus_decoder_destroy\0")?;
                    let decode = *griff.get(b"opus_decode\0")?;
                    return Ok(Bibliothek {
                        _griff: griff,
                        encoder_create,
                        encoder_destroy,
                        encode,
                        encoder_ctl,
                        decoder_create,
                        decoder_destroy,
                        decode,
                    });
                }
            }
            Err(fehler) => letzter = format!("{}: {fehler}", name.to_string_lossy()),
        }
    }
    Err(anyhow!(
        "Opus-Bibliothek nicht gefunden (zuletzt: {letzter}). \
         Installiere libopus, oder lege sie neben die Anwendung \
         (Linux: libopus.so.0, macOS: libopus.dylib, Windows: opus.dll)."
    ))
}

/// Ist Opus verfügbar? (Für eine klare Meldung im Fenster.)
pub fn verfuegbar() -> Result<()> {
    bibliothek().map(|_| ())
}

/// Ein Opus-Kodierer (ein Sprecher).
pub struct Kodierer {
    st: *mut c_void,
}

unsafe impl Send for Kodierer {}

impl Kodierer {
    pub fn neu() -> Result<Self> {
        let b = bibliothek()?;
        let mut fehler: c_int = 0;
        let st =
            unsafe { (b.encoder_create)(ABTASTRATE as i32, 1, OPUS_APPLICATION_VOIP, &mut fehler) };
        if st.is_null() || fehler != 0 {
            return Err(anyhow!("opus_encoder_create: Fehler {fehler}"));
        }
        let this = Self { st };
        unsafe {
            (b.encoder_ctl)(this.st, OPUS_SET_BITRATE_REQUEST, BITRATE);
            (b.encoder_ctl)(this.st, OPUS_SET_COMPLEXITY_REQUEST, 6 as c_int);
            // Verlust ist im Netz normal: FEC hilft dem Empfänger, den
            // verlorenen Rahmen aus dem nächsten mitzurechnen.
            (b.encoder_ctl)(this.st, OPUS_SET_INBAND_FEC_REQUEST, 1 as c_int);
            (b.encoder_ctl)(this.st, OPUS_SET_PACKET_LOSS_PERC_REQUEST, 8 as c_int);
            (b.encoder_ctl)(this.st, OPUS_SET_DTX_REQUEST, 0 as c_int);
            (b.encoder_ctl)(this.st, OPUS_SET_SIGNAL_REQUEST, OPUS_SIGNAL_VOICE);
        }
        Ok(this)
    }

    /// Kodiert genau 960 Abtastwerte zu einem Rahmen.
    pub fn kodiere(&mut self, pcm: &[i16], ziel: &mut [u8]) -> Result<usize> {
        let b = bibliothek()?;
        if pcm.len() != RAHMEN_SAMPLES {
            return Err(anyhow!(
                "Opus braucht {RAHMEN_SAMPLES} Abtastwerte je Rahmen, bekommen {}",
                pcm.len()
            ));
        }
        let len = unsafe {
            (b.encode)(
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
        if let Ok(b) = bibliothek() {
            unsafe { (b.encoder_destroy)(self.st) }
        }
    }
}

/// Ein Opus-Dekodierer — **je Sprecher einer**.
pub struct Dekodierer {
    st: *mut c_void,
}

unsafe impl Send for Dekodierer {}

impl Dekodierer {
    pub fn neu() -> Result<Self> {
        let b = bibliothek()?;
        let mut fehler: c_int = 0;
        let st = unsafe { (b.decoder_create)(ABTASTRATE as i32, 1, &mut fehler) };
        if st.is_null() || fehler != 0 {
            return Err(anyhow!("opus_decoder_create: Fehler {fehler}"));
        }
        Ok(Self { st })
    }

    /// Dekodiert einen Rahmen nach `pcm` (mindestens 960 Werte).
    pub fn dekodiere(&mut self, rahmen: &[u8], pcm: &mut [i16]) -> Result<usize> {
        let b = bibliothek()?;
        let len = unsafe {
            (b.decode)(
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
        let b = bibliothek()?;
        let len = unsafe {
            (b.decode)(
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

    /// Dekodiert **mit** Fehlerkorrektur aus dem nächsten Rahmen (Opus-FEC).
    ///
    /// Wird benutzt, wenn ein Rahmen fehlt und der folgende schon da ist: dann
    /// steckt im folgenden die grobe Fassung des fehlenden.
    pub fn mit_fec(&mut self, folgender: &[u8], pcm: &mut [i16]) -> Result<usize> {
        let b = bibliothek()?;
        let len = unsafe {
            (b.decode)(
                self.st,
                folgender.as_ptr(),
                folgender.len() as i32,
                pcm.as_mut_ptr(),
                RAHMEN_SAMPLES as c_int,
                1,
            )
        };
        if len < 0 {
            return Err(anyhow!("opus_decode (FEC): Fehler {len}"));
        }
        Ok(len as usize)
    }
}

impl Drop for Dekodierer {
    fn drop(&mut self) {
        if let Ok(b) = bibliothek() {
            unsafe { (b.decoder_destroy)(self.st) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_neben_dem_programm_kommt_vor_dem_systemweiten() {
        let ordner = std::path::Path::new("/Programme/Sprechwerk.app/Contents/MacOS");
        let liste = kandidaten(Some(ordner), None);
        let stelle = |gesucht: &str| {
            liste
                .iter()
                .position(|k| k.to_string_lossy() == gesucht)
                .unwrap_or_else(|| panic!("{gesucht} fehlt in {liste:?}"))
        };
        // Im App-Bundle (macOS) und neben der .exe (Windows) …
        let bundle =
            stelle("/Programme/Sprechwerk.app/Contents/MacOS/../Frameworks/libopus.0.dylib");
        let neben = stelle("/Programme/Sprechwerk.app/Contents/MacOS/opus.dll");
        // … vor dem bloßen Namen, den das System auflöst.
        let name = stelle("libopus.so.0");
        assert!(bundle < name && neben < name);
        // Homebrew auf Apple Silicon kennt dlopen nicht von selbst.
        stelle("/opt/homebrew/lib/libopus.0.dylib");
    }

    #[test]
    fn eigenes_homebrew_praefix_kommt_vor_den_festen_pfaden() {
        let liste = kandidaten(None, Some(std::path::Path::new("/Users/a/brew")));
        let stelle = |gesucht: &str| liste.iter().position(|k| k.to_string_lossy() == gesucht);
        let eigen = stelle("/Users/a/brew/lib/libopus.0.dylib").expect("eigenes Präfix fehlt");
        let fest = stelle("/opt/homebrew/lib/libopus.0.dylib").expect("fester Pfad fehlt");
        assert!(eigen < fest);
    }

    #[test]
    fn ohne_programmpfad_bleiben_die_namen() {
        let liste = kandidaten(None, None);
        assert_eq!(
            liste.first().map(|k| k.to_string_lossy().into_owned()),
            Some("libopus.so.0".into())
        );
    }

    fn sinus(anzahl: usize, hz: f32, amplitude: f32) -> Vec<i16> {
        (0..anzahl)
            .map(|i| {
                let wert = amplitude
                    * (2.0 * std::f32::consts::PI * hz * i as f32 / ABTASTRATE as f32).sin();
                (wert * i16::MAX as f32) as i16
            })
            .collect()
    }

    #[test]
    fn bibliothek_wird_gefunden() {
        // Auf dieser Maschine ist Opus installiert; fehlt sie anderswo, sagt
        // die Meldung, wonach zu suchen ist.
        if let Err(fehler) = verfuegbar() {
            assert!(fehler.to_string().contains("opus.dll"), "{fehler}");
        }
    }

    #[test]
    fn kodieren_und_dekodieren_erhaelt_den_ton() {
        let mut k = Kodierer::neu().expect("Kodierer");
        let mut d = Dekodierer::neu().expect("Dekodierer");
        let mut puffer = vec![0u8; 4000];
        let pcm = sinus(RAHMEN_SAMPLES, 440.0, 0.5);

        for _ in 0..3 {
            let len = k.kodiere(&pcm, &mut puffer).expect("kodieren");
            assert!(len > 0 && len < 1200, "Rahmenlänge {len}");
            let mut zurueck = vec![0i16; RAHMEN_SAMPLES];
            let geholt = d
                .dekodiere(&puffer[..len], &mut zurueck)
                .expect("dekodieren");
            assert_eq!(geholt, RAHMEN_SAMPLES);
        }

        let mut zurueck = vec![0i16; RAHMEN_SAMPLES];
        let len = k.kodiere(&pcm, &mut puffer).unwrap();
        d.dekodiere(&puffer[..len], &mut zurueck).unwrap();
        let energie: f32 =
            zurueck.iter().map(|s| (*s as f32).powi(2)).sum::<f32>() / RAHMEN_SAMPLES as f32;
        assert!(
            energie.sqrt() > 1000.0,
            "Signal kommt zu leise an: {energie}"
        );
    }

    #[test]
    fn rahmen_passt_in_ein_datagramm() {
        let mut k = Kodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        let pcm = sinus(RAHMEN_SAMPLES, 200.0, 0.9);
        for _ in 0..5 {
            let len = k.kodiere(&pcm, &mut puffer).unwrap();
            assert!(
                len + sprechwerk_protocol::SPRACHKOPF_LEN <= sprechwerk_protocol::MAX_SPRACHRAHMEN,
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
        assert_eq!(d.verschollen(&mut pcm).unwrap(), RAHMEN_SAMPLES);
    }

    /// Fehlerkorrektur: ein Rahmen fehlt, der nächste trägt ihn grob mit.
    #[test]
    fn fec_liefert_ton_statt_stille() {
        let mut k = Kodierer::neu().unwrap();
        let mut d = Dekodierer::neu().unwrap();
        let mut puffer = vec![0u8; 4000];
        let pcm = sinus(RAHMEN_SAMPLES, 500.0, 0.6);

        // Ein paar Rahmen Vorgeschichte, damit FEC etwas zum Anknüpfen hat.
        let mut letzter = Vec::new();
        for _ in 0..5 {
            let len = k.kodiere(&pcm, &mut puffer).unwrap();
            letzter = puffer[..len].to_vec();
            let mut zurueck = vec![0i16; RAHMEN_SAMPLES];
            d.dekodiere(&letzter, &mut zurueck).unwrap();
        }

        let mut ersatz = vec![0i16; RAHMEN_SAMPLES];
        let geholt = d.mit_fec(&letzter, &mut ersatz).unwrap();
        assert_eq!(geholt, RAHMEN_SAMPLES);
        let energie: f32 =
            ersatz.iter().map(|s| (*s as f32).powi(2)).sum::<f32>() / RAHMEN_SAMPLES as f32;
        assert!(
            energie.sqrt() > 500.0,
            "FEC liefert fast Stille ({energie}) — dann ist sie nutzlos"
        );
    }
}

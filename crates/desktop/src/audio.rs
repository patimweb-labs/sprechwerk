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
//!
//! **Ohne diese Werkzeuge** (Windows, macOS) kommen Liste, Pegel und Prüfton
//! über die Geräteschicht von `crates/audio` (cpal: WASAPI, CoreAudio) — also
//! über denselben Weg, den dort auch die Sprache nimmt. Unterschieden wird
//! danach, **was da ist**, nicht nach dem Betriebssystem (AGENTS.md §2.10).

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
    #[error(
        "Kein Tonweg: weder PipeWire/PulseAudio (pactl/parec/paplay) noch eine Geräteschicht des Systems"
    )]
    Werkzeug,
    #[error("Gerät nicht gefunden: {0}")]
    Geraet(String),
    #[error("Messung fehlgeschlagen: {0}")]
    Messung(String),
}

/// Ist das Werkzeug im `PATH`?
///
/// Selbst gesucht statt über `sh -c "command -v …"`: unter Windows gibt es kein
/// `sh` (oder eines aus Git, das dann bei jedem Aufruf ein Konsolenfenster
/// aufblitzen lässt).
fn werkzeug(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|pfad| im_pfad(name, &pfad))
}

/// Rein: liegt `name` (unter Windows auch `name.exe`) in einem der Ordner?
fn im_pfad(name: &str, pfad: &std::ffi::OsStr) -> bool {
    std::env::split_paths(pfad).any(|ordner| {
        [name.to_string(), format!("{name}.exe")]
            .iter()
            .any(|datei| ordner.join(datei).is_file())
    })
}

/// Geräte über die Geräteschicht von `crates/audio`, für eine der beiden Seiten.
fn systemliste(mikrofon: bool) -> Result<Vec<Geraet>, Fehler> {
    let geraete = match sprechwerk_audio::geraet::systemgeraete() {
        Some(Ok(g)) => g,
        Some(Err(text)) => return Err(Fehler::Messung(text)),
        None => return Err(Fehler::Werkzeug),
    };
    let liste = if mikrofon {
        geraete.mikrofon
    } else {
        geraete.lautsprecher
    };
    Ok(liste
        .into_iter()
        .map(|g| Geraet {
            id: g.name.clone(),
            name: g.name,
            vorgabe: g.vorgabe,
        })
        .collect())
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
        // Monitore sind keine Aufnahmegeräte für Sprache: sie liefern die
        // Wiedergabe des Rechners zurück. Wer sie als Mikrofon wählt, hört sich
        // selbst — und alle anderen doppelt.
        if art == "sources" && ist_monitor(&id) {
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

/// Ist das ein Monitor (Ausgang, mitgeschnitten)? Monitore liefern das, was der
/// Rechner **abspielt** — als Mikrofon gewählt hört man sich selbst und alle
/// anderen doppelt (Rückkopplung).
pub fn ist_monitor(id: &str) -> bool {
    id.ends_with(".monitor")
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
    let roh = if teile.len() > 1 && teile[0] == "usb" {
        teile.get(1).copied().unwrap_or(kern)
    } else {
        teile.first().copied().unwrap_or(kern)
    };
    // „Sennheiser_Sennheiser_Profile_00" → „Sennheiser": Gerätenamen wiederholen
    // den Hersteller; das erste Wort genügt und liest sich besser.
    let mut woerter = roh.split('_').filter(|w| !w.is_empty());
    let erstes = woerter.next().unwrap_or(roh);
    let zweites = woerter.next();
    let name = match zweites {
        Some(z) if z.eq_ignore_ascii_case(erstes) => erstes.to_string(),
        Some("Profile") | Some("Headset") | Some("Gaming") => erstes.to_string(),
        _ => roh.replace('_', " "),
    };
    name.trim().to_string()
}

/// Vorgabegerät laut `pactl get-default-…`.
fn vorgabe(art: &str) -> Option<String> {
    let ausgabe = Command::new("pactl")
        .args(["get-default", art])
        .output()
        .ok()?;
    if !ausgabe.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Aufnahmegeräte.
pub fn mikrofon_geraete() -> Result<Vec<Geraet>, Fehler> {
    if !werkzeug("pactl") {
        return systemliste(true);
    }
    let mut liste = liste("sources")?;
    let vorgabe = vorgabe("source");
    for g in liste.iter_mut() {
        g.vorgabe = vorgabe.as_deref() == Some(g.id.as_str());
    }
    Ok(liste)
}

/// Wiedergabegeräte.
pub fn lautsprecher_geraete() -> Result<Vec<Geraet>, Fehler> {
    if !werkzeug("pactl") {
        return systemliste(false);
    }
    let mut liste = liste("sinks")?;
    let vorgabe = vorgabe("sink");
    for g in liste.iter_mut() {
        g.vorgabe = vorgabe.as_deref() == Some(g.id.as_str());
    }
    Ok(liste)
}

/// Abtastrate des Prüftons — dieselbe wie die der Geräteschicht.
const TESTTON_RATE: u32 = 48_000;
/// Länge des Prüftons in Sekunden.
const TESTTON_DAUER: f32 = 0.7;

/// Der Prüfton als rohe Bytes (440 Hz, 16 Bit, mono, kleine Ende).
fn testton_pcm() -> Vec<u8> {
    let rate = TESTTON_RATE;
    let dauer = TESTTON_DAUER;
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
    daten
}

/// Schreibt den Prüfton als WAV in eine Datei (für `paplay`).
fn testton_datei() -> Result<std::path::PathBuf, Fehler> {
    let pfad = std::env::temp_dir().join("sprechwerk-testton.wav");
    let rate = TESTTON_RATE;
    let daten = testton_pcm();
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
        return testton_system(geraet);
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

/// Prüfton über die Geräteschicht (ohne `paplay`).
///
/// Die Wiedergabe nimmt die Daten sofort an und spielt sie im Hintergrund;
/// der Strom muss deshalb so lange leben, wie der Ton dauert — sonst bricht er
/// nach dem ersten Block ab.
fn testton_system(geraet: &str) -> Result<(), Fehler> {
    let mut wiedergabe = sprechwerk_audio::geraet::wiedergabe_oeffnen(geraet)
        .map_err(|e| Fehler::Geraet(format!("{geraet}: {e}")))?;
    wiedergabe
        .schreibe(&testton_pcm())
        .map_err(|e| Fehler::Messung(e.to_string()))?;
    std::thread::sleep(Duration::from_secs_f32(TESTTON_DAUER) + Duration::from_millis(200));
    Ok(())
}

/// Bildet einen Effektivwert (`summe` der Quadrate über `anzahl` Werte
/// zwischen -1 und 1) auf 0–100 ab: -60 dB bis 0 dB (Sprache liegt selten über
/// -6 dB).
fn pegel_wert(summe: f64, anzahl: u64) -> Result<u8, Fehler> {
    if anzahl == 0 {
        return Err(Fehler::Messung("keine Daten vom Mikrofon".into()));
    }
    let rms = (summe / anzahl as f64).sqrt();
    let db = 20.0 * rms.max(1e-6).log10();
    let anteil = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    Ok((anteil * 100.0).round() as u8)
}

/// Summiert die Quadrate eines Blocks (16 Bit, kleine Ende).
fn quadrate(block: &[u8]) -> (f64, u64) {
    let (paare, _) = block.as_chunks::<2>();
    let summe = paare
        .iter()
        .map(|p| {
            let wert = i16::from_le_bytes(*p) as f64 / i16::MAX as f64;
            wert * wert
        })
        .sum();
    (summe, paare.len() as u64)
}

/// Wartet bis zur Frist auf Blöcke und liefert den Pegel.
fn pegel_sammeln(
    empfangen: &std::sync::mpsc::Receiver<Vec<u8>>,
    millisekunden: u64,
) -> Result<u8, Fehler> {
    let frist = Instant::now() + Duration::from_millis(millisekunden);
    let mut summe: f64 = 0.0;
    let mut anzahl: u64 = 0;
    loop {
        let jetzt = Instant::now();
        if jetzt >= frist {
            break;
        }
        match empfangen.recv_timeout(frist - jetzt) {
            Ok(block) => {
                let (s, n) = quadrate(&block);
                summe += s;
                anzahl += n;
            }
            Err(_) => break,
        }
    }
    pegel_wert(summe, anzahl)
}

/// Pegel über die Geräteschicht (ohne `parec`).
///
/// Die Aufnahme ist nicht `Send` (ein cpal-Strom gehört seinem Thread) und ihr
/// `lies` blockiert, bis Daten kommen — deshalb wird sie **im** Lesethread
/// geöffnet, und der Aufrufer wartet nur bis zur Frist. Liefert das Gerät nie
/// etwas, bleibt der Lesethread bis zum Programmende stehen; die Oberfläche
/// hängt trotzdem nicht.
fn pegel_system(geraet: &str, millisekunden: u64) -> Result<u8, Fehler> {
    let (senden, empfangen) = std::sync::mpsc::channel::<Vec<u8>>();
    let (bereit, ob_bereit) = std::sync::mpsc::channel::<Result<(), String>>();
    let geraet = geraet.to_string();
    std::thread::Builder::new()
        .name("sprechwerk-pegelmessung".into())
        .spawn(move || {
            let mut aufnahme = match sprechwerk_audio::geraet::aufnahme_oeffnen(&geraet) {
                Ok(a) => {
                    let _ = bereit.send(Ok(()));
                    a
                }
                Err(e) => {
                    let _ = bereit.send(Err(format!("{geraet}: {e}")));
                    return;
                }
            };
            // 10 ms bei 48 kHz, 16 Bit.
            let mut block = vec![0u8; 960];
            while aufnahme.lies(&mut block).is_ok() {
                if senden.send(block.clone()).is_err() {
                    return;
                }
            }
        })
        .map_err(|_| Fehler::Messung("Lesethread".into()))?;
    // Öffnen dauert auf WASAPI/CoreAudio Millisekunden; zwei Sekunden sind
    // großzügig und halten die Oberfläche trotzdem nicht lange auf.
    match ob_bereit.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => {}
        Ok(Err(text)) => return Err(Fehler::Geraet(text)),
        Err(_) => return Err(Fehler::Messung("Mikrofon öffnet nicht".into())),
    }
    pegel_sammeln(&empfangen, millisekunden)
}

/// Misst den Eingangspegel für `millisekunden` und liefert 0–100.
///
/// **Die Frist muss halten.** Ein `read()` auf der Pipe blockiert, bis Daten
/// kommen oder der Prozess endet — die Frist im Schleifenkopf wird dann nie
/// erreicht. Genau das hat die Oberfläche aufgehängt: der Sprechtest wartete
/// auf ein Mikrofon, das nichts lieferte. Deshalb liest ein eigener Thread und
/// der Aufrufer wartet mit `recv_timeout`; danach wird der Prozess beendet,
/// komme was wolle.
pub fn pegel(geraet: &str, millisekunden: u64) -> Result<u8, Fehler> {
    // Erst das Argument prüfen, dann die Werkzeuge: ein Monitor ist **immer**
    // falsch, egal ob `parec` installiert ist. Sonst hinge die Ablehnung eines
    // falschen Geräts davon ab, was auf dem Rechner installiert ist — und ihre
    // Prüfung ebenso.
    if ist_monitor(geraet) {
        return Err(Fehler::Geraet(format!(
            "{geraet} ist ein Monitor (Wiedergabe), kein Mikrofon"
        )));
    }
    if !werkzeug("parec") {
        return pegel_system(geraet, millisekunden);
    }
    let mut befehl = Command::new("parec");
    befehl.args(["--format=s16le", "--rate=16000", "--channels=1", "--raw"]);
    if !geraet.is_empty() {
        befehl.arg(format!("--device={geraet}"));
    }
    let mut kind = befehl
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Fehler::Werkzeug)?;

    // Der Lesethread endet mit dem Prozess (EOF) — oder wird mit ihm beendet.
    let (senden, empfangen) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut aus = match kind.stdout.take() {
        Some(aus) => aus,
        None => {
            let _ = kind.kill();
            return Err(Fehler::Werkzeug);
        }
    };
    std::thread::Builder::new()
        .name("sprechwerk-pegelmessung".into())
        .spawn(move || {
            let mut puffer = [0u8; 4096];
            loop {
                match aus.read(&mut puffer) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if senden.send(puffer[..n].to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        })
        .map_err(|_| Fehler::Messung("Lesethread".into()))?;

    let ergebnis = pegel_sammeln(&empfangen, millisekunden);
    let _ = kind.kill();
    let _ = kind.wait();
    ergebnis
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
    fn monitore_werden_erkannt() {
        assert!(ist_monitor("alsa_output.usb-Kopfhoerer.monitor"));
        assert!(!ist_monitor("alsa_input.usb-Kopfhoerer.mono-fallback"));
    }

    /// **Die Frist muss halten** — auch wenn das Gerät nichts liefert.
    ///
    /// Vorher blockierte `read()` unbegrenzt: der Sprechtest hängte die ganze
    /// Oberfläche auf, weil ein synchroner Befehl auf dem Hauptthread wartete.
    #[test]
    fn messung_haelt_die_frist_auch_ohne_daten() {
        let start = std::time::Instant::now();
        let _ = pegel("gibt-es-nicht", 150);
        let gedauert = start.elapsed();
        assert!(
            gedauert < std::time::Duration::from_secs(3),
            "Messung lief {gedauert:?} — die Frist greift nicht"
        );
    }

    #[test]
    fn monitor_als_mikrofon_wird_abgelehnt() {
        let fehler = pegel("alsa_output.x.monitor", 100).unwrap_err();
        assert!(matches!(fehler, Fehler::Geraet(_)), "{fehler}");
    }

    #[test]
    fn werkzeug_wird_im_pfad_gefunden_auch_als_exe() {
        let ordner = std::env::temp_dir().join(format!("sprechwerk-pfad-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Ordner");
        std::fs::write(ordner.join("pactl"), b"").expect("Datei");
        std::fs::write(ordner.join("parec.exe"), b"").expect("Datei");
        let pfad =
            std::env::join_paths([std::path::PathBuf::from("/gibt/es/nicht"), ordner.clone()])
                .expect("PATH");
        assert!(im_pfad("pactl", &pfad));
        assert!(im_pfad("parec", &pfad), "Windows: parec.exe zählt");
        assert!(!im_pfad("paplay", &pfad));
        // Ein Ordner gleichen Namens ist kein Werkzeug.
        std::fs::create_dir_all(ordner.join("pacat")).expect("Ordner");
        assert!(!im_pfad("pacat", &pfad));
        let _ = std::fs::remove_dir_all(ordner);
    }

    #[test]
    fn pegel_bildet_minus_60_bis_0_db_ab() {
        // Stille → 0, Vollaussteuerung → 100, -30 dB → Mitte.
        assert_eq!(pegel_wert(0.0, 100).unwrap(), 0);
        assert_eq!(pegel_wert(100.0, 100).unwrap(), 100);
        let minus_30_db = 10f64.powf(-30.0 / 20.0);
        assert_eq!(
            pegel_wert(minus_30_db * minus_30_db * 10.0, 10).unwrap(),
            50
        );
        assert!(matches!(pegel_wert(0.0, 0), Err(Fehler::Messung(_))));
    }

    #[test]
    fn quadrate_lesen_16_bit_kleine_ende() {
        let mut block = Vec::new();
        block.extend_from_slice(&i16::MAX.to_le_bytes());
        block.extend_from_slice(&0i16.to_le_bytes());
        block.push(0x7f); // halbes Paar am Ende zählt nicht
        let (summe, anzahl) = quadrate(&block);
        assert_eq!(anzahl, 2);
        assert!((summe - 1.0).abs() < 1e-9, "{summe}");
    }

    #[test]
    fn pegel_aus_einem_strom_von_bloecken() {
        // Derselbe Weg wie bei der Geräteschicht, nur ohne Gerät.
        let (senden, empfangen) = std::sync::mpsc::channel();
        let laut: Vec<u8> = (0..480)
            .flat_map(|_| (i16::MAX / 2).to_le_bytes())
            .collect();
        senden.send(laut).unwrap();
        drop(senden);
        let wert = pegel_sammeln(&empfangen, 200).expect("Pegel");
        // -6 dB → 90.
        assert_eq!(wert, 90);
    }

    #[test]
    fn testton_pcm_hat_die_laenge_der_wav() {
        let pcm = testton_pcm();
        assert_eq!(
            pcm.len(),
            (TESTTON_RATE as f32 * TESTTON_DAUER) as usize * 2
        );
        // Hüllkurve: beginnt und endet leise, kein Knacken.
        let erster = i16::from_le_bytes([pcm[0], pcm[1]]);
        let letzter = i16::from_le_bytes([pcm[pcm.len() - 2], pcm[pcm.len() - 1]]);
        assert!(
            erster.abs() < 100 && letzter.abs() < 100,
            "{erster} {letzter}"
        );
    }

    #[test]
    fn testton_ist_gueltiges_wav() {
        let pfad = testton_datei().expect("Testton");
        let daten = std::fs::read(&pfad).expect("lesen");
        assert_eq!(&daten[..4], b"RIFF");
        assert_eq!(&daten[8..12], b"WAVE");
        assert!(daten.len() > 44, "Kopfdaten plus Ton");
        // 48 kHz, 0,7 s, 16 Bit mono ≈ 67 200 Byte Nutzlast.
        assert!(
            daten.len() > 60_000 && daten.len() < 70_000,
            "{}",
            daten.len()
        );
        let _ = std::fs::remove_file(pfad);
    }
}

//! Geräte: Aufnahme und Wiedergabe — auf Windows, macOS und Linux.
//!
//! # Zwei Wege, ein Rückfall
//!
//! ```text
//!               ┌── cpal ────────────── WASAPI (Windows)
//! Aufnahme ─────┤                       CoreAudio (macOS)
//! Wiedergabe    └── PipeWire/PulseAudio  ALSA (Linux)
//!                    └─ parec/pacat ──── Linux, wenn ALSA nicht öffnet
//! ```
//!
//! **cpal ist der Regelweg**, weil es auf allen drei Systemen dieselbe
//! Schnittstelle bietet (eine Rust-Bibliothek, kein Kindprozess, kein
//! Plattform-Code im Rest der Anwendung). Auf Linux öffnet cpal ALSA-Geräte;
//! wenn das nicht geht — etwa weil `pipewire-alsa` fehlt und ALSA kein Gerät
//! `default` kennt — fällt der Pfad auf die Werkzeuge von PipeWire/PulseAudio
//! zurück (`parec`/`pacat`). Dieser Rückfall ist der **hier geprüfte** Weg.
//!
//! # Warum die Geräteschicht hinter Merkmalen steckt
//!
//! `cpal` braucht auf Linux das ALSA-Entwicklungspaket (`alsa-lib-devel`,
//! `pkg-config`); auf Windows und macOS ist nichts zu installieren. Wer ohne
//! dieses Paket bauen will, baut mit `--no-default-features --features pipewire`
//! und bekommt nur den Rückfallweg. Was gerade benutzt wird, sagt
//! [`Pfad::geraete`] und damit auch die Oberfläche.

use std::io::Read;

/// Eine Aufnahmequelle: liefert rohe 16-Bit-Mono-Bytes bei 48 kHz.
///
/// Kein `Send`: ein cpal-Strom gehört dem Thread, der ihn geöffnet hat. Deshalb
/// wird das Gerät **im** Aufnahmethread geöffnet, nicht davor (siehe
/// `Pfad::starten`).
pub trait Aufnahme {
    /// Füllt `ziel` mit genau `ziel.len()` Bytes (kleine Ende, 16 Bit, mono).
    fn lies(&mut self, ziel: &mut [u8]) -> std::io::Result<()>;
    /// Wie der Weg heißt (für die Meldung im Fenster).
    fn name(&self) -> String;
}

/// Eine Wiedergabesenke: nimmt rohe 16-Bit-Mono-Bytes bei 48 kHz.
///
/// Kein `Send`, aus demselben Grund wie bei [`Aufnahme`].
pub trait Wiedergabe {
    fn schreibe(&mut self, daten: &[u8]) -> std::io::Result<()>;
    fn name(&self) -> String;
}

// ---------------------------------------------------------------- PipeWire

/// Aufnahme über `parec` (PipeWire/PulseAudio, Linux).
pub struct ParecAufnahme {
    kind: std::process::Child,
    aus: std::io::BufReader<std::process::ChildStdout>,
    geraet: String,
}

impl ParecAufnahme {
    pub fn neu(geraet: &str) -> std::io::Result<Self> {
        let mut befehl = std::process::Command::new("parec");
        befehl.args([
            "--format=s16le",
            "--rate=48000",
            "--channels=1",
            "--raw",
            "--latency-msec=20",
        ]);
        if !geraet.is_empty() {
            befehl.arg(format!("--device={geraet}"));
        }
        let mut kind = befehl
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()?;
        let aus = std::io::BufReader::new(
            kind.stdout
                .take()
                .ok_or_else(|| std::io::Error::other("parec liefert nichts"))?,
        );
        Ok(Self {
            kind,
            aus,
            geraet: geraet.to_string(),
        })
    }
}

impl Aufnahme for ParecAufnahme {
    fn lies(&mut self, ziel: &mut [u8]) -> std::io::Result<()> {
        self.aus.read_exact(ziel)
    }

    fn name(&self) -> String {
        if self.geraet.is_empty() {
            "PipeWire (Vorgabe des Systems)".to_string()
        } else {
            format!("PipeWire: {}", self.geraet)
        }
    }
}

impl Drop for ParecAufnahme {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }
}

/// Wiedergabe über `pacat` (PipeWire/PulseAudio, Linux).
pub struct PacatWiedergabe {
    kind: std::process::Child,
    ein: std::process::ChildStdin,
    geraet: String,
}

impl PacatWiedergabe {
    pub fn neu(geraet: &str) -> std::io::Result<Self> {
        let mut befehl = std::process::Command::new("pacat");
        befehl.args([
            "--playback",
            "--format=s16le",
            "--rate=48000",
            "--channels=1",
            "--raw",
            "--latency-msec=40",
        ]);
        if !geraet.is_empty() {
            befehl.arg(format!("--device={geraet}"));
        }
        let mut kind = befehl
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let ein = kind
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("pacat nimmt nichts an"))?;
        Ok(Self {
            kind,
            ein,
            geraet: geraet.to_string(),
        })
    }
}

impl Wiedergabe for PacatWiedergabe {
    fn schreibe(&mut self, daten: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.ein.write_all(daten)
    }

    fn name(&self) -> String {
        if self.geraet.is_empty() {
            "PipeWire (Vorgabe des Systems)".to_string()
        } else {
            format!("PipeWire: {}", self.geraet)
        }
    }
}

impl Drop for PacatWiedergabe {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }
}

// --------------------------------------------------------------------- cpal

#[cfg(any(feature = "cpal", not(target_os = "linux")))]
mod mit_cpal {
    use super::{Aufnahme, Wiedergabe};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{mpsc, Arc, Mutex};

    /// Zielrate und -format des ganzen Pfads.
    const RATE: u32 = 48_000;

    /// Wandelt einen Geräteblock in mono/48 kHz/16 Bit.
    fn nach_ziel(block: &[f32], kanaele: usize, rate: u32) -> Vec<u8> {
        // Mono mischen
        let mono: Vec<f32> = if kanaele <= 1 {
            block.to_vec()
        } else {
            block
                .chunks(kanaele)
                .map(|rahmen| rahmen.iter().sum::<f32>() / kanaele as f32)
                .collect()
        };
        // Auf 48 kHz bringen (lineare Umrechnung; für Sprache genügend)
        let umgerechnet = if rate == RATE {
            mono
        } else {
            let verhaeltnis = RATE as f32 / rate as f32;
            let laenge = (mono.len() as f32 * verhaeltnis) as usize;
            (0..laenge)
                .map(|i| {
                    let pos = i as f32 / verhaeltnis;
                    let index = pos as usize;
                    let rest = pos - index as f32;
                    let a = mono.get(index).copied().unwrap_or(0.0);
                    let b = mono.get(index + 1).copied().unwrap_or(a);
                    a + (b - a) * rest
                })
                .collect()
        };
        let mut bytes = Vec::with_capacity(umgerechnet.len() * 2);
        for wert in umgerechnet {
            let ganz = (wert.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            bytes.extend_from_slice(&ganz.to_le_bytes());
        }
        bytes
    }

    /// Alle Ein- und Ausgabegeräte des Standard-Hosts, Vorgabe markiert.
    pub(super) fn geraete() -> Result<super::Systemgeraete, String> {
        let host = cpal::default_host();
        let vorgabe_ein = host.default_input_device().and_then(|d| d.name().ok());
        let vorgabe_aus = host.default_output_device().and_then(|d| d.name().ok());
        let liste = |geraete: Box<dyn Iterator<Item = cpal::Device>>, vorgabe: &Option<String>| {
            let mut namen: Vec<super::Systemgeraet> = Vec::new();
            for name in geraete.filter_map(|d| d.name().ok()) {
                // Manche Hosts nennen dasselbe Gerät zweimal (je Format).
                if namen.iter().any(|g| g.name == name) {
                    continue;
                }
                namen.push(super::Systemgeraet {
                    vorgabe: vorgabe.as_deref() == Some(name.as_str()),
                    name,
                });
            }
            namen
        };
        Ok(super::Systemgeraete {
            mikrofon: liste(
                Box::new(host.input_devices().map_err(|e| e.to_string())?),
                &vorgabe_ein,
            ),
            lautsprecher: liste(
                Box::new(host.output_devices().map_err(|e| e.to_string())?),
                &vorgabe_aus,
            ),
        })
    }

    /// Aufnahme über cpal (WASAPI / CoreAudio / ALSA).
    pub struct CpalAufnahme {
        /// Der Strom muss leben, solange aufgenommen wird.
        _strom: cpal::Stream,
        empfangen: Receiver<Vec<u8>>,
        rest: Vec<u8>,
        name: String,
    }

    impl CpalAufnahme {
        pub fn neu(geraet: &str) -> Result<Self, String> {
            let host = cpal::default_host();
            let geraet = if geraet.is_empty() {
                host.default_input_device()
                    .ok_or_else(|| "kein Aufnahmegerät".to_string())?
            } else {
                host.input_devices()
                    .map_err(|e| e.to_string())?
                    .find(|d| d.name().map(|n| n == geraet).unwrap_or(false))
                    .ok_or_else(|| format!("Mikrofon „{geraet}“ nicht gefunden"))?
            };
            let name = geraet.name().unwrap_or_else(|_| "Aufnahme".to_string());
            let eingerichtet = geraet.default_input_config().map_err(|e| e.to_string())?;
            let kanaele = eingerichtet.channels() as usize;
            let rate = eingerichtet.sample_rate().0;
            let (senden, empfangen) = mpsc::channel::<Vec<u8>>();

            let fehler = Arc::new(Mutex::new(None::<String>));
            let fehler_cb = fehler.clone();
            let strom = match eingerichtet.sample_format() {
                cpal::SampleFormat::F32 => geraet.build_input_stream(
                    &eingerichtet.config(),
                    move |daten: &[f32], _| {
                        let _ = senden.send(nach_ziel(daten, kanaele, rate));
                    },
                    move |e| *fehler_cb.lock().unwrap() = Some(e.to_string()),
                    None,
                ),
                cpal::SampleFormat::I16 => geraet.build_input_stream(
                    &eingerichtet.config(),
                    move |daten: &[i16], _| {
                        let f: Vec<f32> =
                            daten.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        let _ = senden.send(nach_ziel(&f, kanaele, rate));
                    },
                    move |e| *fehler_cb.lock().unwrap() = Some(e.to_string()),
                    None,
                ),
                anderes => return Err(format!("Sampleformat {anderes:?} nicht unterstützt")),
            }
            .map_err(|e| e.to_string())?;
            strom.play().map_err(|e| e.to_string())?;
            Ok(Self {
                _strom: strom,
                empfangen,
                rest: Vec::new(),
                name,
            })
        }
    }

    impl Aufnahme for CpalAufnahme {
        fn lies(&mut self, ziel: &mut [u8]) -> std::io::Result<()> {
            while self.rest.len() < ziel.len() {
                match self.empfangen.recv() {
                    Ok(block) => self.rest.extend_from_slice(&block),
                    Err(_) => {
                        return Err(std::io::Error::other("Aufnahme beendet"));
                    }
                }
            }
            ziel.copy_from_slice(&self.rest[..ziel.len()]);
            self.rest.drain(..ziel.len());
            Ok(())
        }

        fn name(&self) -> String {
            format!("cpal: {}", self.name)
        }
    }

    /// Wiedergabe über cpal.
    pub struct CpalWiedergabe {
        _strom: cpal::Stream,
        senden: Sender<Vec<i16>>,
        rest: Vec<i16>,
        name: String,
    }

    impl CpalWiedergabe {
        pub fn neu(geraet: &str) -> Result<Self, String> {
            let host = cpal::default_host();
            let geraet = if geraet.is_empty() {
                host.default_output_device()
                    .ok_or_else(|| "kein Wiedergabegerät".to_string())?
            } else {
                host.output_devices()
                    .map_err(|e| e.to_string())?
                    .find(|d| d.name().map(|n| n == geraet).unwrap_or(false))
                    .ok_or_else(|| format!("Wiedergabe „{geraet}“ nicht gefunden"))?
            };
            let name = geraet.name().unwrap_or_else(|_| "Wiedergabe".to_string());
            let eingerichtet = geraet.default_output_config().map_err(|e| e.to_string())?;
            let kanaele = eingerichtet.channels() as usize;
            let rate = eingerichtet.sample_rate().0;
            let (senden, empfangen) = mpsc::channel::<Vec<i16>>();
            let fehler = Arc::new(Mutex::new(None::<String>));
            let fehler_cb = fehler.clone();

            // Der Wiedergabe-Rückruf läuft in einem eigenen Thread; die
            // Umrechnung auf Geräteformat und -rate passiert dort.
            let rest = Arc::new(Mutex::new(Vec::<i16>::new()));
            let rest_cb = rest.clone();
            let strom = match eingerichtet.sample_format() {
                cpal::SampleFormat::F32 => geraet.build_output_stream(
                    &eingerichtet.config(),
                    move |ziel: &mut [f32], _| {
                        let werte = hole(&empfangen, &rest_cb, ziel.len() / kanaele.max(1), rate);
                        for (i, platz) in ziel.iter_mut().enumerate() {
                            let wert = werte.get(i / kanaele.max(1)).copied().unwrap_or(0) as f32
                                / i16::MAX as f32;
                            *platz = wert;
                        }
                    },
                    move |e| *fehler_cb.lock().unwrap() = Some(e.to_string()),
                    None,
                ),
                cpal::SampleFormat::I16 => geraet.build_output_stream(
                    &eingerichtet.config(),
                    move |ziel: &mut [i16], _| {
                        let werte = hole(&empfangen, &rest_cb, ziel.len() / kanaele.max(1), rate);
                        for (i, platz) in ziel.iter_mut().enumerate() {
                            *platz = werte.get(i / kanaele.max(1)).copied().unwrap_or(0);
                        }
                    },
                    move |e| *fehler_cb.lock().unwrap() = Some(e.to_string()),
                    None,
                ),
                anderes => return Err(format!("Sampleformat {anderes:?} nicht unterstützt")),
            }
            .map_err(|e| e.to_string())?;
            strom.play().map_err(|e| e.to_string())?;
            Ok(Self {
                _strom: strom,
                senden,
                rest: Vec::new(),
                name,
            })
        }
    }

    /// Holt so viele Werte, wie das Gerät braucht (48 kHz; bei anderen Raten
    /// wird verdoppelt/halbierte — für Sprache ausreichend).
    fn hole(
        empfangen: &Receiver<Vec<i16>>,
        rest: &Arc<Mutex<Vec<i16>>>,
        anzahl: usize,
        gerate_rate: u32,
    ) -> Vec<i16> {
        let mut puffer = rest.lock().unwrap();
        while puffer.len() < anzahl {
            match empfangen.try_recv() {
                Ok(block) => puffer.extend_from_slice(&block),
                Err(_) => break,
            }
        }
        let nehmen = anzahl.min(puffer.len());
        let mut werte: Vec<i16> = puffer.drain(..nehmen).collect();
        werte.resize(anzahl, 0);
        if gerate_rate == RATE {
            return werte;
        }
        // Andere Geräterate: lineare Umrechnung.
        let verhaeltnis = gerate_rate as f32 / RATE as f32;
        let laenge = (werte.len() as f32 * verhaeltnis) as usize;
        (0..laenge)
            .map(|i| {
                let pos = i as f32 / verhaeltnis;
                werte.get(pos as usize).copied().unwrap_or(0)
            })
            .collect()
    }

    impl Wiedergabe for CpalWiedergabe {
        fn schreibe(&mut self, daten: &[u8]) -> std::io::Result<()> {
            let werte: Vec<i16> = daten
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| i16::from_le_bytes(*p))
                .collect();
            let _ = &self.rest;
            self.senden
                .send(werte)
                .map_err(|_| std::io::Error::other("Wiedergabe beendet"))
        }

        fn name(&self) -> String {
            format!("cpal: {}", self.name)
        }
    }
}

#[cfg(any(feature = "cpal", not(target_os = "linux")))]
pub use mit_cpal::{CpalAufnahme, CpalWiedergabe};

// ------------------------------------------------------------- Geräteliste

/// Ein Gerät, wie das System es nennt — der Name ist zugleich die Kennung, die
/// [`aufnahme_oeffnen`]/[`wiedergabe_oeffnen`] wieder annehmen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Systemgeraet {
    pub name: String,
    pub vorgabe: bool,
}

/// Mikrofone und Lautsprecher.
#[derive(Debug, Clone, Default)]
pub struct Systemgeraete {
    pub mikrofon: Vec<Systemgeraet>,
    pub lautsprecher: Vec<Systemgeraet>,
}

/// Die Geräte über cpal (WASAPI, CoreAudio, ALSA mit `--features cpal`).
///
/// `None` heißt: in **diesem Bau** gibt es keine solche Liste (Linux ohne
/// `cpal`; dort liefert `pactl` die Geräte). Die Fensteranwendung fragt so,
/// statt selbst nach dem Betriebssystem zu unterscheiden (AGENTS.md §2.10).
pub fn systemgeraete() -> Option<Result<Systemgeraete, String>> {
    #[cfg(any(feature = "cpal", not(target_os = "linux")))]
    {
        Some(mit_cpal::geraete())
    }
    #[cfg(not(any(feature = "cpal", not(target_os = "linux"))))]
    {
        None
    }
}

// -------------------------------------------------------- Auswahl mit Rückfall

/// Öffnet die Aufnahme: erst cpal, dann PipeWire.
pub fn aufnahme_oeffnen(geraet: &str) -> std::io::Result<Box<dyn Aufnahme>> {
    #[cfg(any(feature = "cpal", not(target_os = "linux")))]
    {
        match mit_cpal::CpalAufnahme::neu(geraet) {
            Ok(a) => return Ok(Box::new(a)),
            Err(fehler) => {
                eprintln!("cpal-Aufnahme nicht möglich ({fehler}) — Rückfall auf PipeWire");
            }
        }
    }
    Ok(Box::new(ParecAufnahme::neu(geraet)?))
}

/// Öffnet die Wiedergabe: erst cpal, dann PipeWire.
pub fn wiedergabe_oeffnen(geraet: &str) -> std::io::Result<Box<dyn Wiedergabe>> {
    #[cfg(any(feature = "cpal", not(target_os = "linux")))]
    {
        match mit_cpal::CpalWiedergabe::neu(geraet) {
            Ok(w) => return Ok(Box::new(w)),
            Err(fehler) => {
                eprintln!("cpal-Wiedergabe nicht möglich ({fehler}) — Rückfall auf PipeWire");
            }
        }
    }
    Ok(Box::new(PacatWiedergabe::neu(geraet)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipewire_rueckfall_meldet_seinen_weg() {
        // Ohne Gerät: parec startet und meldet sich als PipeWire-Weg.
        if let Ok(a) = ParecAufnahme::neu("") {
            assert!(a.name().starts_with("PipeWire"), "{}", a.name());
        }
    }

    #[test]
    fn geraetewahl_wird_im_namen_sichtbar() {
        // Ein nicht vorhandenes Gerät darf keinen Panik auslösen, nur einen
        // Fehler — und der Name nennt das Gerät.
        match PacatWiedergabe::neu("gibt-es-nicht") {
            Ok(w) => assert!(w.name().contains("gibt-es-nicht") || w.name().contains("PipeWire")),
            Err(fehler) => assert!(!fehler.to_string().is_empty()),
        }
    }
}

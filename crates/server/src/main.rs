//! sprechwerk — Server.
//!
//! Ein Prozess, ein Container, ein QUIC-Port (**Festlegung SPEC §2/§7**).
//! Startablauf: Konfiguration lesen, Datenbank öffnen, Zustand laden,
//! Sicherung planen, lauschen.

mod kern;
mod konfig;
mod netz;
mod tls;

use anyhow::Context;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Unterbefehl `beispiel` gibt eine Konfigurationsdatei aus.
const BEISPIEL: &str = "beispiel";

const HILFE: &str = r#"sprechwerk — Server

  sprechwerk [konfigurationsdatei]   Server starten (Vorgabe: sprechwerk.toml)
  sprechwerk beispiel                Beispielkonfiguration ausgeben
  sprechwerk export <datei>          Zustand als Text schreiben
  sprechwerk import <datei>          Zustand aus Text einspielen
  sprechwerk sicherung               Sofortsicherung anlegen

Konfiguration über Datei und Umgebungsvariablen (SPRECHWERK_*), siehe `beispiel`.
"#;

fn konfig_laden() -> anyhow::Result<konfig::Konfig> {
    let pfad = std::env::var("SPRECHWERK_KONFIG").unwrap_or_else(|_| "sprechwerk.toml".to_string());
    Ok(konfig::Konfig::laden(&pfad).with_context(|| format!("Konfiguration {pfad}"))?)
}

/// Lädt den Zustand für Betriebswerkzeuge. Eine Datenbank ohne Räume (frisch
/// angelegt) bekommt den Empfangsraum, damit Export und Sicherung nie einen
/// unbrauchbaren Zustand schreiben.
fn zustand_laden(konfig: &konfig::Konfig) -> anyhow::Result<sprechwerk_domain::State> {
    let store = sprechwerk_store::Store::oeffnen(konfig.datenbank())?;
    let standard_rolle = konfig.standard_rolle();
    let name = konfig.server.name.clone();
    let mut state = store
        .laden(|| sprechwerk_domain::State::neu(name.clone(), standard_rolle, kern::jetzt_ms()))?;
    if state.raeume.is_empty() {
        state = sprechwerk_domain::State::neu(name, standard_rolle, kern::jetzt_ms());
    }
    Ok(state)
}

fn betrieb_export(ziel: &str) -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = zustand_laden(&konfig)?;
    sprechwerk_store::Store::exportieren(&state, ziel)?;
    println!("Zustand geschrieben: {ziel}");
    Ok(())
}

fn betrieb_import(quelle: &str) -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = sprechwerk_store::Store::importieren(quelle)?;
    let mut store = sprechwerk_store::Store::oeffnen(konfig.datenbank())?;
    // Vor dem Überschreiben sichern: ein Import ist der gefährlichste Befehl.
    let sicherung = konfig
        .sicherung
        .verzeichnis
        .join(format!("vor-import-{}.json", kern::jetzt_ms()));
    if let Ok(alt) = store.laden(|| sprechwerk_domain::State::neu("leer", state.standard_rolle, 0))
    {
        let _ = sprechwerk_store::Store::exportieren(&alt, &sicherung);
    }
    store.speichern(&state)?;
    println!("Zustand eingespielt: {quelle}");
    println!("Bisheriger Zustand gesichert: {}", sicherung.display());
    Ok(())
}

fn betrieb_sicherung() -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = zustand_laden(&konfig)?;
    let pfad = sprechwerk_store::Store::sichern(
        &state,
        &konfig.sicherung.verzeichnis,
        konfig.sicherung.behalten,
        kern::jetzt_ms(),
    )?;
    println!("Sicherung: {}", pfad.display());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let erster = args.next();

    if erster.as_deref() == Some(BEISPIEL) {
        print!("{}", konfig::Konfig::beispiel());
        return Ok(());
    }

    // Betriebswerkzeuge (**Festlegung SPEC §2**: der Text ist die Wahrheit,
    // darum muss man ihn jederzeit herausgeben und zurücklesen können).
    match erster.as_deref() {
        Some("export") => {
            let ziel = args.next().unwrap_or_else(|| "export.json".to_string());
            return betrieb_export(&ziel);
        }
        Some("import") => {
            let quelle = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("Aufruf: sprechwerk import <datei>"))?;
            return betrieb_import(&quelle);
        }
        Some("sicherung") => return betrieb_sicherung(),
        Some("hilfe") | Some("--hilfe") | Some("-h") => {
            print!("{}", HILFE);
            return Ok(());
        }
        _ => {}
    }

    let konfig_pfad = std::env::var("SPRECHWERK_KONFIG")
        .ok()
        .or(erster)
        .unwrap_or_else(|| "sprechwerk.toml".to_string());

    // Protokollierung: Text, filterbar über RUST_LOG.
    let filter = tracing_subscriber::EnvFilter::try_from_env("SPRECHWERK_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("sprechwerk_server=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let konfig = konfig::Konfig::laden(&konfig_pfad)
        .with_context(|| format!("Konfiguration {konfig_pfad}"))?;
    tracing::info!(
        name = %konfig.server.name,
        port = konfig.server.port,
        daten = %konfig.server.daten.display(),
        sicherung = konfig.sicherung.aktiv,
        "sprechwerk startet"
    );

    let laufzeit = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    laufzeit.block_on(async move {
        let kern = Arc::new(Mutex::new(kern::Kern::neu(konfig.clone())?));

        // Sicherung nach Zeitplan (**Festlegung SPEC §2**).
        if konfig.sicherung.aktiv {
            let kern_sicherung = kern.clone();
            let intervall = Duration::from_secs(konfig.sicherung.intervall_minuten.max(1) * 60);
            tokio::spawn(async move {
                let mut takt = tokio::time::interval(intervall);
                takt.tick().await; // erster Takt verstreicht sofort
                loop {
                    takt.tick().await;
                    let (state, verzeichnis, behalten) = {
                        let k = kern_sicherung.lock().expect("Sperre");
                        (
                            k.state.clone(),
                            k.konfig.sicherung.verzeichnis.clone(),
                            k.konfig.sicherung.behalten,
                        )
                    };
                    match sprechwerk_store::Store::sichern(
                        &state,
                        &verzeichnis,
                        behalten,
                        kern::jetzt_ms(),
                    ) {
                        Ok(pfad) => {
                            tracing::info!(datei = %pfad.display(), "Sicherung geschrieben")
                        }
                        Err(fehler) => {
                            tracing::error!(fehler = %fehler, "Sicherung fehlgeschlagen")
                        }
                    }
                }
            });
        }

        // Aufräumen: Aufbewahrungsfrist für Nachrichten.
        if konfig.server.aufbewahrung_tage > 0 {
            let kern_aufraeumen = kern.clone();
            tokio::spawn(async move {
                let mut takt = tokio::time::interval(Duration::from_secs(3_600));
                loop {
                    takt.tick().await;
                    let mut k = kern_aufraeumen.lock().expect("Sperre");
                    let grenze =
                        kern::jetzt_ms() - (k.konfig.server.aufbewahrung_tage as i64) * 86_400_000;
                    let vorher = k.state.nachrichten.len();
                    k.state.nachrichten.retain(|n| n.zeit >= grenze);
                    let entfernt = vorher - k.state.nachrichten.len();
                    if entfernt > 0 {
                        tracing::info!(entfernt, "Nachrichten nach Aufbewahrungsfrist entfernt");
                        let _ = k.speichern();
                    }
                }
            });
        }

        // Messung: Zählerstand im Takt protokollieren, wenn gewünscht.
        if std::env::var("SPRECHWERK_MESSUNG").is_ok() {
            let kern_messung = kern.clone();
            tokio::spawn(async move {
                let mut takt = tokio::time::interval(Duration::from_secs(10));
                let (mut vor_rahmen, mut vor_verteilt, mut vor_verworfen) = (0u64, 0u64, 0u64);
                loop {
                    takt.tick().await;
                    let z = {
                        let k = kern_messung.lock().expect("Sperre");
                        k.zaehler()
                    };
                    tracing::info!(
                        rahmen = z.sprachrahmen - vor_rahmen,
                        verteilt = z.verteilt - vor_verteilt,
                        verloren = z.verworfen - vor_verworfen,
                        abgelehnt = z.abgelehnt,
                        verbundene = z.verbundene,
                        "Sprachrahmen je 10 s"
                    );
                    vor_rahmen = z.sprachrahmen;
                    vor_verteilt = z.verteilt;
                    vor_verworfen = z.verworfen;
                }
            });
        }

        // Signalbehandlung: bei SIGTERM/Ctrl-C noch einmal speichern.
        let kern_signal = kern.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("beende, Zustand wird gespeichert");
            if let Ok(mut k) = kern_signal.lock() {
                let _ = k.speichern();
            }
            std::process::exit(0);
        });

        netz::lauschen(kern).await
    })
}

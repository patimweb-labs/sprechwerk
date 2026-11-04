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

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let erster = args.next();

    if erster.as_deref() == Some(BEISPIEL) {
        print!("{}", konfig::Konfig::beispiel());
        return Ok(());
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
                        Ok(pfad) => tracing::info!(datei = %pfad.display(), "Sicherung geschrieben"),
                        Err(fehler) => tracing::error!(fehler = %fehler, "Sicherung fehlgeschlagen"),
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
                    let grenze = kern::jetzt_ms() - (k.konfig.server.aufbewahrung_tage as i64) * 86_400_000;
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

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der laufende Dienst: Netz, Verwaltung, Lebenszeichen, Sicherung,
//! Aufräumen — für Konsole und Fenster derselbe Ablauf.

use crate::kern::{self, Kern};
use crate::konfig::Konfig;
use crate::mitschrift::Mitschrift;
use crate::verwaltung::{self, Verwaltung};
use crate::{gesundheit, netz, tls};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Richtet die Protokollierung ein (einmal je Prozess) und gibt die
/// Mitschrift zurück, die die Verwaltung zeigt — dieselben Zeilen wie auf der
/// Konsole bzw. in `docker logs`, keine zweite Quelle.
///
/// Der Vorgabefilter muss den **Modulpfad** treffen: `sprechwerk` deckt
/// `sprechwerk_server::…` ab (Präfix), gefunden 2026-11-13 mit
/// `scripts/lastprobe.sh`, als der Server in `docker logs` stumm war.
pub fn protokoll_einrichten() -> Mitschrift {
    let filter = tracing_subscriber::EnvFilter::try_from_env("SPRECHWERK_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("sprechwerk=info"));
    let mitschrift = Mitschrift::neu();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(mitschrift.schreiber())
        .try_init();
    mitschrift
}

/// Startet den Dienst. `bereit` bekommt die Verwaltung, sobald sie steht
/// (für das Server-Fenster); danach läuft das Netz, bis die Aufgabe endet.
pub async fn laufen(
    konfig: Konfig,
    mitschrift: Mitschrift,
    bereit: impl FnOnce(Verwaltung) + Send,
) -> anyhow::Result<()> {
    tracing::info!(
        name = %konfig.server.name,
        port = konfig.server.port,
        daten = %konfig.server.daten.display(),
        sicherung = konfig.sicherung.aktiv,
        "sprechwerk startet"
    );
    for eintrag in konfig.veraltet() {
        tracing::warn!(
            eintrag,
            "veraltet und ohne Wirkung (Verwaltung im Browser gibt es nicht mehr)"
        );
    }
    let kern = Arc::new(Mutex::new(Kern::neu(konfig.clone())?));
    let zert = tls::laden_oder_erzeugen(&konfig.server.daten)?;
    let v = Verwaltung {
        kern: kern.clone(),
        mitschrift,
        fingerabdruck: zert.fingerabdruck.clone().into(),
        gestartet: kern::jetzt_ms(),
    };

    // Steuerkanal für die Konsole. Scheitert er (Port belegt), läuft der
    // Sprachdienst trotzdem — er ist Beiwerk.
    if konfig.verwaltung.aktiv {
        match verwaltung::schluessel_schreiben(&konfig.server.daten) {
            Ok(schluessel) => {
                let v = v.clone();
                let port = konfig.verwaltung.port;
                tokio::spawn(async move {
                    if let Err(fehler) = verwaltung::bedienen(v, port, schluessel).await {
                        tracing::error!(port, fehler = %fehler, "Steuerkanal nicht gestartet");
                    }
                });
                tracing::info!(
                    port = konfig.verwaltung.port,
                    "Verwaltung bereit: `sprechwerk stand`, `sprechwerk rolle <wer> <rolle>`"
                );
            }
            Err(fehler) => {
                tracing::error!(fehler = %fehler, "Verwaltungsschlüssel nicht schreibbar")
            }
        }
    }

    // Lebenszeichen für den Container-Healthcheck (siehe `gesundheit.rs`).
    // Sofort einmal schreiben: sonst gilt der Dienst in den ersten Sekunden
    // als krank.
    {
        let daten = konfig.server.daten.clone();
        let _ = gesundheit::schreibe(&daten, kern::jetzt_ms());
        tokio::spawn(async move {
            let mut takt = tokio::time::interval(gesundheit::TAKT);
            loop {
                takt.tick().await;
                if let Err(fehler) = gesundheit::schreibe(&daten, kern::jetzt_ms()) {
                    tracing::warn!(fehler = %fehler, "Lebenszeichen nicht schreibbar");
                }
            }
        });
    }

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
                let z = kern_messung.lock().expect("Sperre").zaehler();
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

    bereit(v);
    netz::lauschen(kern, zert).await
}

/// Speichert den Zustand (beim Beenden).
pub fn speichern(v: &Verwaltung) {
    if let Ok(mut k) = v.kern.lock() {
        let _ = k.speichern();
    }
}

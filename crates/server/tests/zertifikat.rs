// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Zertifikatsprüfung am echten Server: anheften, wiederfinden, ablehnen.
//!
//! ADR 0002 §3 sagt: selbst signiertes Zertifikat, Fingerabdruck beim ersten
//! Verbinden merken, danach muss er passen. Bis 2026-11-14 nahm der Client
//! **jedes** Zertifikat an — die Zusage stand in der Anleitung, nicht im Code.
//! Diese Prüfung hält den jetzigen Stand fest, und zwar am echten TLS-Pfad:
//!
//! 1. Der erste Kontakt liefert den Fingerabdruck zurück — und zwar genau den
//!    des Zertifikats, das der Server benutzt (Vergleich mit der Datei).
//! 2. Ein angehefteter, passender Wert wird angenommen.
//! 3. Ein anderer Wert bricht **im Handschlag** ab: der Server sieht keine
//!    Anmeldung, kein Beweis, kein Passwort — und der Admin-Platz bleibt frei.

use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_domain::Role;
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Ein laufender Server auf einem freien Port (wie in `verschluesselt.rs`).
struct Testserver {
    kind: Child,
    verzeichnis: std::path::PathBuf,
}

impl Drop for Testserver {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
        let _ = std::fs::remove_dir_all(&self.verzeichnis);
    }
}

fn freier_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn starte_server() -> (Testserver, SocketAddr) {
    for _ in 0..8 {
        if let Some(ergebnis) = versuche_server().await {
            return ergebnis;
        }
    }
    panic!("kein Server startbar");
}

async fn versuche_server() -> Option<(Testserver, SocketAddr)> {
    let verzeichnis = std::env::temp_dir().join(format!(
        "sprechwerk-zert-{}-{}",
        std::process::id(),
        freier_port()
    ));
    std::fs::create_dir_all(&verzeichnis).unwrap();
    let port = freier_port();
    let mut kind = Command::new(env!("CARGO_BIN_EXE_sprechwerk"))
        .env("SPRECHWERK_DATEN", &verzeichnis)
        .env("SPRECHWERK_PORT", port.to_string())
        .env("SPRECHWERK_BIND", "127.0.0.1")
        .env("SPRECHWERK_NAME", "Zertifikatstest")
        .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Server startet");

    // Auf „lauscht auf" warten; danach die Ausgabe weiterlesen, sonst stirbt
    // der Server am nächsten `println!` (Broken pipe).
    {
        use std::io::BufRead;
        let stdout = kind.stdout.take().expect("stdout");
        let mut leser = std::io::BufReader::new(stdout);
        let mut zeile = String::new();
        for _ in 0..200 {
            zeile.clear();
            if leser.read_line(&mut zeile).unwrap_or(0) == 0 {
                break;
            }
            if zeile.contains("lauscht auf") {
                break;
            }
        }
        std::thread::spawn(move || {
            let mut rest = String::new();
            while leser.read_line(&mut rest).unwrap_or(0) > 0 {
                rest.clear();
            }
        });
    }
    for _ in 0..50 {
        if std::net::UdpSocket::bind(("127.0.0.1", port)).is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if let Ok(Some(_)) = kind.try_wait() {
        let _ = std::fs::remove_dir_all(&verzeichnis);
        return None;
    }
    let adresse: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    Some((Testserver { kind, verzeichnis }, adresse))
}

/// Der Fingerabdruck, den der Server laut seiner Datei hat.
fn fingerabdruck_aus_datei(server: &Testserver) -> String {
    let der = std::fs::read(server.verzeichnis.join("zertifikat.der"))
        .expect("Server legt sein Zertifikat im Datenverzeichnis ab");
    sprechwerk_protocol::zertifikat_fingerabdruck(&der)
}

#[tokio::test]
async fn erster_kontakt_liefert_den_fingerabdruck_des_servers() {
    let (server, adresse) = starte_server().await;
    let verbindung = verbinde_mit(adresse, None).await;

    let gesehen = verbindung.zertifikat.clone();
    assert_eq!(
        gesehen.len(),
        sprechwerk_protocol::FINGERABDRUCK_LEN,
        "ein Fingerabdruck ist 32 Byte hexadezimal: {gesehen}"
    );
    assert_eq!(
        gesehen,
        fingerabdruck_aus_datei(&server),
        "Client und Server müssen dasselbe rechnen — sonst prüft der Client \
         etwas anderes, als der Server anzeigt"
    );
}

#[tokio::test]
async fn angehefteter_fingerabdruck_wird_angenommen() {
    let (server, adresse) = starte_server().await;
    let passend = fingerabdruck_aus_datei(&server);

    // Mit Doppelpunkten und Großbuchstaben, wie man ihn abtippt.
    let getippt = sprechwerk_protocol::gruppiere(&passend).to_uppercase();
    let verbindung = verbinde_mit(adresse, Some(&getippt)).await;
    assert_eq!(verbindung.zertifikat, passend);
}

#[tokio::test]
async fn fremdes_zertifikat_bricht_ab_bevor_die_anmeldung_geht() {
    let (server, adresse) = starte_server().await;
    let richtig = fingerabdruck_aus_datei(&server);
    // Ein Angreifer mit eigenem Zertifikat: irgendein anderer, gültiger Wert.
    let fremd = sprechwerk_protocol::zertifikat_fingerabdruck(b"boeser-server");
    assert_ne!(richtig, fremd);

    let schluessel = Schluessel::neu();
    let versuch = Verbindung::oeffnen_mit(
        adresse,
        &schluessel,
        "Opfer",
        Some("streng-geheim"),
        Some(&fremd),
    )
    .await;
    let fehler = match versuch {
        Ok(_) => panic!("ein fremdes Zertifikat darf nicht durchgehen"),
        Err(fehler) => fehler,
    };
    let text = format!("{fehler:#}");
    assert!(
        text.contains("anderes Zertifikat"),
        "der Grund muss in der Meldung stehen, nicht nur „TLS-Fehler“: {text}"
    );

    // **Der Beweis, dass der Abbruch früh kommt:** der erste Client eines
    // Servers wird Administrator. Wäre die Anmeldung trotz Ablehnung
    // durchgegangen, wäre der Platz jetzt weg — oder der Server hätte den
    // Beweis gesehen. Also: der nächste Client ist Administrator.
    let verbindung = verbinde_mit(adresse, Some(&richtig)).await;
    assert_eq!(
        verbindung.rolle,
        Role::Administrator,
        "der abgelehnte Handschlag darf den Administratorplatz nicht verbrauchen"
    );
}

/// Verbindet mit Nachfassen (der Server ist manchmal erst Millisekunden nach
/// seiner Meldung ansprechbar) und prüft gegen `erwartet`.
async fn verbinde_mit(adresse: SocketAddr, erwartet: Option<&str>) -> Verbindung {
    let schluessel = Schluessel::neu();
    let mut letzter = String::new();
    for _ in 0..12 {
        match tokio::time::timeout(
            Duration::from_secs(3),
            Verbindung::oeffnen_mit(adresse, &schluessel, "Ada", None, erwartet),
        )
        .await
        {
            Ok(Ok(v)) => return v,
            Ok(Err(fehler)) => letzter = format!("{fehler:#}"),
            Err(_) => letzter = "Zeitüberschreitung".to_string(),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("Verbindung zu {adresse} nicht möglich: {letzter}");
}

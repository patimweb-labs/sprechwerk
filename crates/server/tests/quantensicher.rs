// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Quantensicherheit am Draht: bietet jede Seite die hybride Gruppe wirklich an?
//!
//! ADR 0010 verlangt `X25519MLKEM768` — hybrid, also klassisch **und**
//! quantensicher in einem Handschlag. Ein Eintrag in einer Liste beweist nichts:
//! diese Prüfung zwingt beide Seiten auf **ausschließlich** die hybride Gruppe.
//! Kommt die Verbindung zustande, wurde sie benutzt; gäbe es sie nicht, meldet
//! `rustls` „keine gemeinsame Gruppe".
//!
//! Zwei Richtungen, weil zwei Konfigurationen im Spiel sind:
//!
//! 1. echter Server ← PQ-only-Client: der Server bietet PQ an.
//! 2. echter Client (aus `crates/adapters`) → PQ-only-Server: der Client bietet
//!    PQ an. Geprüft wird bis zum ersten Rahmen, nicht nur der Handschlag.

use anyhow::anyhow;
use quinn::crypto::rustls::QuicServerConfig;
use rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768;
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::{Fehlercode, ServerFrame, PROTOKOLL_VERSION};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------- Hilfsmittel

fn freier_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Nimmt jedes Zertifikat an — hier wird die **Gruppe** geprüft, nicht der
/// Server. (Die Zertifikatsprüfung hat ihre eigene Prüfung: `zertifikat.rs`.)
#[derive(Debug)]
struct Egal;

impl rustls::client::danger::ServerCertVerifier for Egal {
    fn verify_server_cert(
        &self,
        _e: &rustls::pki_types::CertificateDer<'_>,
        _i: &[rustls::pki_types::CertificateDer<'_>],
        _n: &rustls::pki_types::ServerName<'_>,
        _o: &[u8],
        _t: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &rustls::pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &verfahren())
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &rustls::pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &verfahren())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        verfahren().supported_schemes()
    }
}

fn verfahren() -> rustls::crypto::WebPkiSupportedAlgorithms {
    rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms
}

/// Ein Anbieter, der **nur** die hybride Gruppe anbietet.
fn nur_hybrid() -> rustls::crypto::CryptoProvider {
    let mut anbieter = rustls::crypto::aws_lc_rs::default_provider();
    anbieter.kx_groups = vec![X25519MLKEM768];
    anbieter
}

fn selbstsigniert() -> (
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
) {
    let erzeugt =
        rcgen::generate_simple_self_signed(vec!["sprechwerk".into()]).expect("Zertifikat");
    (
        vec![rustls::pki_types::CertificateDer::from(
            erzeugt.cert.der().to_vec(),
        )],
        rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
            erzeugt.key_pair.serialize_der(),
        )),
    )
}

// -------------------------------------------------- 1. Der echte Server kann PQ

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

async fn starte_server() -> Option<(Testserver, SocketAddr)> {
    let verzeichnis = std::env::temp_dir().join(format!(
        "sprechwerk-pq-{}-{}",
        std::process::id(),
        freier_port()
    ));
    std::fs::create_dir_all(&verzeichnis).unwrap();
    let port = freier_port();
    let mut kind = Command::new(env!("CARGO_BIN_EXE_sprechwerk"))
        .env("SPRECHWERK_DATEN", &verzeichnis)
        .env("SPRECHWERK_PORT", port.to_string())
        .env("SPRECHWERK_BIND", "127.0.0.1")
        .env("SPRECHWERK_NAME", "Quantentest")
        .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
        // Keine Verwaltung (Port 7444, Browser) für Testserver.
        .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Server startet");
    {
        use std::io::BufRead;
        let stdout = kind.stdout.take().expect("stdout");
        let mut leser = std::io::BufReader::new(stdout);
        for _ in 0..200 {
            let mut zeile = String::new();
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

/// Ein Client, der **nur** die hybride Gruppe anbietet.
fn pq_only_client() -> quinn::ClientConfig {
    let anbieter = Arc::new(nur_hybrid());
    let mut tls = rustls::ClientConfig::builder_with_provider(anbieter)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Egal))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"sprechwerk/1".to_vec()];
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(tls).expect("QUIC");
    quinn::ClientConfig::new(Arc::new(quic))
}

#[tokio::test]
async fn der_server_bietet_die_hybride_gruppe_an() {
    let (_server, adresse) = starte_server().await.expect("Server startbar");

    let mut endpunkt = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpunkt.set_default_client_config(pq_only_client());
    let aufbau = endpunkt.connect(adresse, "sprechwerk").expect("Anlauf");

    // Mit ausschließlich PQ auf der Clientseite: kommt die Verbindung zustande,
    // hat der Server die hybride Gruppe angeboten. Sonst: „keine gemeinsame
    // Gruppe" und der Test scheitert hier.
    let conn = tokio::time::timeout(Duration::from_secs(10), aufbau)
        .await
        .expect("kein Zeitlimit")
        .expect("Server muss X25519MLKEM768 anbieten");

    // Der Server öffnet den Steuerstrom und schickt die Beweisaufforderung —
    // das beweist, dass nicht nur der Handschlag, sondern auch der Aufbau steht.
    let mut recv = tokio::time::timeout(Duration::from_secs(10), conn.accept_bi())
        .await
        .expect("kein Zeitlimit")
        .expect("Steuerstrom");
    let mut puffer = Vec::new();
    let rahmen = lies_rahmen(&mut recv.1, &mut puffer).await;
    assert!(
        matches!(rahmen, ServerFrame::Anforderung { .. }),
        "erwartet: Beweisaufforderung, bekommen: {rahmen:?}"
    );
    conn.close(0u32.into(), b"fertig");
}

// ----------------------------- 2. Der echte Client kann PQ (bis zum Rahmen)

/// Ein Server, der **nur** die hybride Gruppe anbietet und gerade genug
/// Sprechwerk spricht, damit der echte Client bis zur Ablehnung kommt.
fn pq_only_server() -> (
    quinn::Endpoint,
    Vec<rustls::pki_types::CertificateDer<'static>>,
) {
    let (kette, schluessel) = selbstsigniert();
    let anbieter = Arc::new(nur_hybrid());
    let mut tls = rustls::ServerConfig::builder_with_provider(anbieter)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_no_client_auth()
        .with_single_cert(kette.clone(), schluessel)
        .expect("Zertifikat");
    tls.alpn_protocols = vec![b"sprechwerk/1".to_vec()];
    let quic = QuicServerConfig::try_from(tls).expect("QUIC");
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(quic));
    config.transport_config(Arc::new(quinn::TransportConfig::default()));
    let endpunkt = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    (endpunkt, kette)
}

async fn lies_rahmen(recv: &mut quinn::RecvStream, puffer: &mut Vec<u8>) -> ServerFrame {
    loop {
        if let Some((rahmen, verbraucht)) =
            sprechwerk_protocol::lese_rahmen::<ServerFrame>(puffer).expect("Rahmen lesbar")
        {
            puffer.drain(..verbraucht);
            return rahmen;
        }
        let mut block = [0u8; 8192];
        let n = tokio::io::AsyncReadExt::read(recv, &mut block)
            .await
            .expect("lesen");
        if n == 0 {
            panic!("Client hat den Strom geschlossen, bevor ein Rahmen kam");
        }
        puffer.extend_from_slice(&block[..n]);
    }
}

#[tokio::test]
async fn der_client_bietet_die_hybride_gruppe_an() {
    let (endpunkt, _kette) = pq_only_server();
    let adresse = endpunkt.local_addr().unwrap();

    tokio::spawn(async move {
        if let Some(ankunft) = endpunkt.accept().await {
            // Handschlag abschließen, dann Sprechwerk-Rahmen sprechen.
            let Ok(conn) = ankunft.await else { return };
            // **Der Server öffnet den Steuerstrom** (ein Strom wird erst mit den
            // ersten Bytes sichtbar) — der Client nimmt ihn an.
            let Ok((mut send, mut recv)) = conn.open_bi().await else {
                return;
            };
            // 1. Beweisaufforderung
            if schreibe(
                &mut send,
                &ServerFrame::Anforderung {
                    version: PROTOKOLL_VERSION,
                    nonce: "00".repeat(32),
                },
            )
            .await
            .is_err()
            {
                return;
            }
            // 2. Anmeldung abwarten (Inhalt egal — es geht um den Pfad).
            let mut block = [0u8; 8192];
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                tokio::io::AsyncReadExt::read(&mut recv, &mut block),
            )
            .await;
            // 3. Ablehnen: der Client muss bis hierher gekommen sein.
            let _ = schreibe(
                &mut send,
                &ServerFrame::Fehler {
                    code: Fehlercode::AnmeldungAbgelehnt,
                    text: "Absicht: Handschlag hat geklappt".to_string(),
                },
            )
            .await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });

    let ergebnis = tokio::time::timeout(
        Duration::from_secs(15),
        Verbindung::oeffnen(adresse, &Schluessel::neu(), "Ada", None),
    )
    .await
    .expect("kein Zeitlimit");

    let fehler = match ergebnis {
        Ok(_) => panic!("dieser Server hätte ablehnen müssen"),
        Err(fehler) => format!("{fehler:#}"),
    };
    // **Der Beweis:** der Client kam bis zur Anmeldung. Wäre die hybride Gruppe
    // nicht angeboten worden, stünde hier ein Handschlagfehler („no common
    // key exchange"), und die Ablehnung des Servers käme nie an.
    //
    // Geprüft wird der **Text des Servers**: seit 2026-11-19 gibt der Client
    // den Grund der Ablehnung unverändert weiter („Passwort falsch" statt
    // „Anmeldung abgelehnt: …"), damit beim Nutzer ankommt, woran es lag.
    assert!(
        fehler.contains("Absicht: Handschlag hat geklappt"),
        "der Client muss den Handschlag mit X25519MLKEM768 schaffen, sonst käme \
         die Ablehnung nicht an: {fehler}"
    );
}

async fn schreibe(send: &mut quinn::SendStream, rahmen: &ServerFrame) -> anyhow::Result<()> {
    let bytes = sprechwerk_protocol::schreibe_rahmen(rahmen).map_err(|e| anyhow!("{e}"))?;
    tokio::io::AsyncWriteExt::write_all(send, &bytes).await?;
    tokio::io::AsyncWriteExt::flush(send).await?;
    Ok(())
}

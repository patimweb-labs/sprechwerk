//! Latenzmessung: wie lange braucht ein Sprachrahmen durch den Server?
//!
//! Diese Prüfung läuft **nicht** bei jedem `cargo test` mit (sie braucht Zeit
//! und belastet die CPU). Aufruf:
//!
//! ```text
//! cargo test --release --test latenz -- --ignored --nocapture
//! ```
//!
//! Gemessen wird, was der Nutzer merkt: von „Client A schickt" bis „Client B
//! hat den Rahmen". Das schließt die Aufgabenplanung beider Clients ein — also
//! mehr als der reine Serverdurchlauf, und damit die ehrlichere Zahl.

use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::{ClientFrame, ServerFrame, Sprachkopf, SPRACHKOPF_LEN};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Testserver {
    kind: Child,
    adresse: SocketAddr,
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

fn starten(extra: &[(&str, &str)]) -> Testserver {
    let port = freier_port();
    let verzeichnis =
        std::env::temp_dir().join(format!("sprechwerk-latenz-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&verzeichnis).unwrap();
    let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk"));
    befehl
        .env("SPRECHWERK_DATEN", &verzeichnis)
        .env("SPRECHWERK_PORT", port.to_string())
        .env("SPRECHWERK_BIND", "127.0.0.1")
        .env("SPRECHWERK_NAME", "Latenztest")
        .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
        // Keine Verwaltung (Port 7444, Browser) für Testserver.
        .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in extra {
        befehl.env(k, v);
    }
    let mut kind = befehl.spawn().expect("Server startet");
    use std::io::BufRead;
    let stdout = kind.stdout.take().unwrap();
    let mut leser = std::io::BufReader::new(stdout);
    let mut zeile = String::new();
    while leser.read_line(&mut zeile).unwrap_or(0) > 0 {
        if zeile.contains("lauscht auf") {
            break;
        }
        zeile.clear();
    }
    std::thread::spawn(move || {
        let mut rest = String::new();
        let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
    });
    Testserver {
        kind,
        adresse: format!("127.0.0.1:{port}").parse().unwrap(),
        verzeichnis,
    }
}

async fn warte_auf<F>(verbindung: &mut Verbindung, bedingung: F) -> ServerFrame
where
    F: Fn(&ServerFrame) -> bool,
{
    let frist = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let rest = frist.saturating_duration_since(tokio::time::Instant::now());
        assert!(!rest.is_zero(), "Zeitüberschreitung");
        if let Ok(Ok(Some(frame))) = tokio::time::timeout(rest, verbindung.empfang()).await {
            if bedingung(&frame) {
                return frame;
            }
        }
    }
}

fn prozent(werte: &[u128], p: f64) -> u128 {
    let mut sortiert = werte.to_vec();
    sortiert.sort_unstable();
    let index = ((sortiert.len() as f64 - 1.0) * p).round() as usize;
    sortiert[index]
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Latenzmessung: nur mit --ignored --nocapture aufrufen"]
async fn sprachlatenz_durch_den_server() {
    let server = starten(&[]);

    let mut ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .unwrap();
    let mut bosse = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bosse", None)
        .await
        .unwrap();

    let raum = ada.raeume.first().unwrap().id;
    ada.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    bosse.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Betreten { .. })).await;

    // Aufwärmen (Socket, Aufgaben, Zwischenspeicher).
    for folge in 0..50u16 {
        let d = Sprachkopf::neu(folge).schreiben(&[0u8; 80]);
        ada.sprache_senden(bytes::Bytes::from(d)).unwrap();
        let _ = tokio::time::timeout(Duration::from_millis(200), bosse.sprache_empfangen()).await;
    }

    const N: u16 = 1000;
    let mut zeiten = Vec::with_capacity(N as usize);
    for folge in 0..N {
        let daten = Sprachkopf::neu(folge).schreiben(&[0u8; 80]);
        let start = Instant::now();
        ada.sprache_senden(bytes::Bytes::from(daten)).unwrap();
        let empfangen = tokio::time::timeout(Duration::from_secs(2), bosse.sprache_empfangen())
            .await
            .expect("Rahmen kommt an")
            .expect("kein Fehler");
        let dauer = start.elapsed().as_micros();
        // Kopfgröße aus dem Vertrag: sie ist schon einmal gewachsen
        // (Sprecher kam dazu), und dieser Test soll das mitgehen.
        assert_eq!(empfangen.len(), SPRACHKOPF_LEN + 80);
        zeiten.push(dauer);
    }

    let p50 = prozent(&zeiten, 0.50);
    let p95 = prozent(&zeiten, 0.95);
    let p99 = prozent(&zeiten, 0.99);
    let max = *zeiten.iter().max().unwrap();
    let min = *zeiten.iter().min().unwrap();
    let mittel: u128 = zeiten.iter().sum::<u128>() / zeiten.len() as u128;

    println!("\nSprachrahmen durch den Server (A → Server → B), {N} Messungen:");
    println!("  min  {min:>6} µs");
    println!("  p50  {p50:>6} µs");
    println!("  p95  {p95:>6} µs");
    println!("  p99  {p99:>6} µs");
    println!("  max  {max:>6} µs");
    println!("  mittel {mittel:>4} µs");
    println!("  (Opus-20-ms-Rahmen: ein Rahmen alle 20 000 µs)\n");

    // Abnahmekriterium: der Server darf die Sprache nicht hörbar verzögern.
    assert!(
        p99 < 2_000,
        "p99 {p99} µs überschreitet 2 ms — der Server verzögert hörbar"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Beitrittsmessung: nur mit --ignored --nocapture aufrufen"]
async fn beitrittszeit_bis_zum_ersten_rahmen() {
    let server = starten(&[]);
    let mut zeiten = Vec::new();
    for i in 0..20 {
        let name = format!("K{i}");
        let start = Instant::now();
        let v = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), &name, None)
            .await
            .unwrap();
        zeiten.push(start.elapsed().as_micros());
        v.schliessen();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let p50 = prozent(&zeiten, 0.50);
    let p95 = prozent(&zeiten, 0.95);
    println!("\nBeitritt (QUIC + Anmeldung + Willkommen), 20 Versuche:");
    println!("  p50  {p50:>6} µs");
    println!("  p95  {p95:>6} µs");
    println!("  max  {:>6} µs\n", *zeiten.iter().max().unwrap());
    assert!(p95 < 500_000, "p95 {p95} µs — Beitritt dauert über 500 ms");
}

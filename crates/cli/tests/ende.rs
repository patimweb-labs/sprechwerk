//! Der Referenzclient als echter Prozess: Abschied und Verlauf.
//!
//! **Gefunden 2026-11-19 (Simulation):**
//! * Nach `/ende` blieb der Client im `stand` als „in Empfang" stehen — der
//!   Prozess endete, bevor das `CONNECTION_CLOSE` hinausging.
//! * Der Client meldete „N alte Nachrichten" und gab keine einzige aus: der
//!   Verlauf kam mit dem Betreten, der Serverschlüssel erst danach.
//!
//! Beides ist nur mit einem **Prozess** prüfbar (das Prozessende ist der
//! Fehler), deshalb läuft der Server hier im Hintergrundfaden und der Client
//! als eigenes Programm (`CARGO_BIN_EXE_sprechwerk-cli`).

use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_server::{dienst, konfig::Konfig};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// So lange darf eine Prüfung auf eine Meldung warten.
const FRIST: Duration = Duration::from_secs(15);
/// So lange darf es dauern, bis ein Abschied ankommt. Ohne den Fix waren es
/// die 30 s Leerlauf-Timeout des Servers.
const ABSCHIED: Duration = Duration::from_secs(10);

fn freier_udp_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn verzeichnis(name: &str) -> PathBuf {
    let pfad = std::env::temp_dir().join(format!(
        "sprechwerk-cli-{}-{}-{}",
        std::process::id(),
        name,
        freier_udp_port()
    ));
    std::fs::create_dir_all(&pfad).unwrap();
    pfad
}

/// Ein echter Dienst im Hintergrundfaden (eigene Laufzeit, echter UDP-Port).
struct Pruefserver {
    adresse: SocketAddr,
    verzeichnis: PathBuf,
}

impl Pruefserver {
    fn starten(name: &str) -> Self {
        let verzeichnis = verzeichnis(name);
        let port = freier_udp_port();
        let mut konfig = Konfig::default();
        konfig.server.name = "Prüfwerk".to_string();
        konfig.server.bind = "127.0.0.1".to_string();
        konfig.server.port = port;
        konfig.server.daten = verzeichnis.clone();
        konfig.sicherung.aktiv = false;
        // Keine Verwaltung: kein zweiter Port, keine Schlüsseldatei.
        konfig.verwaltung.aktiv = false;

        std::thread::spawn(move || {
            let Ok(laufzeit) = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            laufzeit.block_on(async move {
                let mitschrift = dienst::protokoll_einrichten();
                if let Err(fehler) = dienst::laufen(konfig, mitschrift, |_| {}).await {
                    eprintln!("Prüfserver: {fehler}");
                }
            });
        });

        // Bereit, sobald der UDP-Port belegt ist.
        let mut bereit = false;
        for _ in 0..200 {
            if std::net::UdpSocket::bind(("127.0.0.1", port)).is_err() {
                bereit = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(bereit, "Prüfserver lauscht nicht auf Port {port}");
        Self {
            adresse: format!("127.0.0.1:{port}").parse().unwrap(),
            verzeichnis,
        }
    }
}

impl Drop for Pruefserver {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.verzeichnis);
    }
}

/// Der Referenzclient als Kindprozess, Zeile für Zeile mitlesbar.
struct Cli {
    kind: Child,
    eingabe: ChildStdin,
    zeilen: Receiver<String>,
    /// Alles, was bisher gelesen wurde — die Reihenfolge der Zeilen ist Teil
    /// der Prüfung (der Verlauf kommt nach dem Schlüssel).
    gesehen: String,
}

impl Cli {
    fn starten(server: &Pruefserver, name: &str, schluessel: &Path, extra: &[&str]) -> Self {
        let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk-cli"));
        befehl
            .arg("--server")
            .arg(server.adresse.to_string())
            .arg("--name")
            .arg(name)
            .arg("--schluessel")
            .arg(schluessel)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut kind = befehl.spawn().expect("sprechwerk-cli startet");
        let eingabe = kind.stdin.take().expect("stdin");
        let ausgabe = kind.stdout.take().expect("stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for zeile in std::io::BufReader::new(ausgabe)
                .lines()
                .map_while(Result::ok)
            {
                if tx.send(zeile).is_err() {
                    break;
                }
            }
        });
        Self {
            kind,
            eingabe,
            zeilen: rx,
            gesehen: String::new(),
        }
    }

    /// Schickt eine Zeile an den Client (Text oder `/befehl`).
    fn schreibe(&mut self, text: &str) {
        writeln!(self.eingabe, "{text}").expect("Eingabe");
        let _ = self.eingabe.flush();
    }

    /// Liest mit, bis `muster` in einer Zeile vorkommt; liefert **alles**
    /// bisher Gelesene.
    fn warte_auf(&mut self, muster: &str, frist: Duration) -> String {
        let ende = Instant::now() + frist;
        loop {
            let rest = ende.saturating_duration_since(Instant::now());
            assert!(
                !rest.is_zero(),
                "„{muster}\" kam nicht innerhalb von {frist:?}; gelesen:\n{}",
                self.gesehen
            );
            match self.zeilen.recv_timeout(rest) {
                Ok(zeile) => {
                    self.gesehen.push_str(&zeile);
                    self.gesehen.push('\n');
                    if zeile.contains(muster) {
                        return self.gesehen.clone();
                    }
                }
                Err(_) => panic!("„{muster}\" kam nicht; gelesen:\n{}", self.gesehen),
            }
        }
    }

    /// Wartet auf das Ende des Prozesses.
    fn warte_auf_ende(&mut self) {
        let status = self.kind.wait().expect("Prozessende");
        assert!(status.success(), "der Client endete mit {status}");
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        // Schon beendet? Dann ist `kill` ein wirkungsloser Fehlschlag.
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }
}

fn anwesend(v: &Verbindung, raum: sprechwerk_domain::RoomId) -> usize {
    v.raeume
        .iter()
        .find(|r| r.id == raum)
        .map(|r| r.anwesend)
        .unwrap_or(0)
}

/// `/ende` muss beim Server ankommen: der Client darf nicht als Geist
/// zurückbleiben.
#[tokio::test]
async fn ende_raeumt_die_anwesenheit_auf() {
    let server = Pruefserver::starten("ende");
    let mut zeuge = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Zeuge", None)
        .await
        .expect("Zeuge meldet sich an");
    let raum = zeuge.raum.expect("Zeuge sitzt in einem Raum");
    assert_eq!(anwesend(&zeuge, raum), 1, "erst ist nur der Zeuge da");

    let mut cli = Cli::starten(
        &server,
        "Ada",
        &server.verzeichnis.join("k_Ada"),
        &["--raum", "1"],
    );
    cli.warte_auf("Tippe Text", FRIST);

    // Warten, bis der Zeuge Ada sieht — sonst prüften wir einen Abschied, der
    // nie ein Kommen war.
    let frist = Instant::now() + FRIST;
    while anwesend(&zeuge, raum) != 2 {
        let rest = frist.saturating_duration_since(Instant::now());
        assert!(!rest.is_zero(), "Ada ist nicht anwesend geworden");
        if tokio::time::timeout(rest, zeuge.empfang()).await.is_err() {
            panic!("Ada ist nicht anwesend geworden: {:?}", zeuge.raeume);
        }
    }

    cli.schreibe("/ende");
    cli.warte_auf_ende();

    // Jetzt muss der Abschied beim Server angekommen sein.
    let frist = Instant::now() + ABSCHIED;
    loop {
        if anwesend(&zeuge, raum) == 1 {
            break;
        }
        let rest = frist.saturating_duration_since(Instant::now());
        assert!(
            !rest.is_zero(),
            "Ada bleibt nach /ende anwesend — das CLOSE-Paket ging verloren: {:?}",
            zeuge.raeume
        );
        let _ = tokio::time::timeout(rest, zeuge.empfang()).await;
    }
}

/// Der Verlauf wird ausgegeben, sobald der Serverschlüssel da ist.
#[tokio::test]
async fn verlauf_wird_nach_dem_schluessel_ausgegeben() {
    let server = Pruefserver::starten("verlauf");

    // Ada ist zuerst da: sie legt den Serverschlüssel an und bleibt online.
    let mut ada = Cli::starten(&server, "Ada", &server.verzeichnis.join("k_Ada"), &[]);
    ada.warte_auf("Tippe Text", FRIST);
    // Dem Anlegen des Schlüssels eine Runde Zeit lassen (ADR 0012).
    std::thread::sleep(Duration::from_millis(1_500));
    ada.schreibe("Guten Abend");
    // Die eigene Nachricht kommt zurück — erst dann steht sie im Verlauf.
    let gesehen = ada.warte_auf("Ada: Guten Abend", FRIST);
    if gesehen.contains("nicht gesendet") {
        // Der Schlüssel war noch nicht da: noch einmal.
        std::thread::sleep(Duration::from_millis(1_500));
        ada.schreibe("Guten Abend");
        ada.warte_auf("Ada: Guten Abend", FRIST);
    }

    // Bosse betritt denselben Raum: er bekommt den Verlauf mit dem Betreten
    // (Schlüssel fehlt zu dem Zeitpunkt noch) und muss ihn nachreichen.
    let mut bosse = Cli::starten(
        &server,
        "Bosse",
        &server.verzeichnis.join("k_Bosse"),
        &["--raum", "1"],
    );
    bosse.warte_auf("Tippe Text", FRIST);
    // Nach dem Schlüssel muss der Verlauf kommen — mit Inhalt, nicht nur als
    // Zahl („1 alte Nachrichten" stand schon da, die Zeile fehlte).
    let ausgabe = bosse.warte_auf("Ada: Guten Abend", FRIST);
    assert!(
        ausgabe.contains("1 alte Nachrichten"),
        "das Betreten zählt den Verlauf:\n{ausgabe}"
    );
    // Und zwar erst, wenn der Schlüssel da ist: vorher steht dort „fehlt noch".
    assert!(
        !ausgabe.contains("(verschlüsselt — Schlüssel fehlt noch)"),
        "der Verlauf wurde mit einer Lücke ausgegeben:\n{ausgabe}"
    );
    bosse.schreibe("/ende");
    bosse.warte_auf_ende();
}

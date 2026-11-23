//! Das Spielnetz im Referenzclient: zwei Prozesse, ein Server, ein Echo.
//!
//! Was hier geprüft wird, ist der ganze Weg aus Sicht eines Spiels: Ada gibt
//! einen lokalen TCP-Port frei, Bosse tritt bei, bekommt eine **lokale**
//! Adresse genannt und schickt darüber ein Echo — das kommt unverändert
//! zurück. Nur mit zwei echten Prozessen ist das aussagekräftig: die
//! Freigabe hängt an der Sitzung (ADR 0016 §4), und beide Seiten müssen
//! getrennte Kennungen, Tunnel und lokale Ports haben.
//!
//! Aufbau wie in `ende.rs`: der Dienst läuft im Hintergrundfaden mit eigener
//! Laufzeit, die Clients sind echte Prozesse (`CARGO_BIN_EXE_sprechwerk-cli`).

use sprechwerk_server::{dienst, konfig::Konfig};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// So lange darf eine Prüfung auf eine Meldung warten.
const FRIST: Duration = Duration::from_secs(20);

fn freier_udp_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn verzeichnis(name: &str) -> PathBuf {
    let pfad = std::env::temp_dir().join(format!(
        "sprechwerk-cli-spielnetz-{}-{}-{}",
        std::process::id(),
        name,
        freier_udp_port()
    ));
    std::fs::create_dir_all(&pfad).unwrap();
    pfad
}

/// Ein echter Dienst im Hintergrundfaden — **mit** Spielnetz (ADR 0016 §3).
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
        konfig.verwaltung.aktiv = false;
        // Ohne diesen Schalter bietet der Server den ALPN nicht an, und der
        // Client könnte gar nicht erst beitreten (ADR 0016 §2).
        konfig.spielnetz.aktiv = true;

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
    /// Alles, was bisher gelesen wurde — für die Fehlermeldung.
    gesehen: String,
}

impl Cli {
    fn starten(server: &Pruefserver, name: &str, schluessel: &Path) -> Self {
        let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk-cli"));
        befehl
            .arg("--server")
            .arg(server.adresse.to_string())
            .arg("--name")
            .arg(name)
            .arg("--schluessel")
            .arg(schluessel)
            // Beide in denselben Raum: Freigaben gelten je Raum (ADR 0016 §6).
            .args(["--raum", "1", "--spielnetz"])
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
        // Schon gelesen zählt auch: die Reihenfolge der Zeilen ist nicht die
        // Reihenfolge der Prüfungen.
        if self.gesehen.contains(muster) {
            return self.gesehen.clone();
        }
        let ende = Instant::now() + frist;
        loop {
            let rest = ende.saturating_duration_since(Instant::now());
            assert!(
                !rest.is_zero(),
                "„{muster}\u{201c} kam nicht innerhalb von {frist:?}; gelesen:\n{}",
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
                Err(_) => panic!("„{muster}\u{201c} kam nicht; gelesen:\n{}", self.gesehen),
            }
        }
    }

    /// Die zuletzt genannte lokale Adresse aus dem Mitgelesenen.
    fn lokale_adresse(&self) -> SocketAddr {
        let zeile = self
            .gesehen
            .lines()
            .filter_map(|z| z.strip_prefix("lokale Adresse: "))
            .next_back()
            .unwrap_or_else(|| panic!("keine lokale Adresse; gelesen:\n{}", self.gesehen));
        zeile
            .trim()
            .parse()
            .unwrap_or_else(|fehler| panic!("„{zeile}\u{201c} ist keine Adresse: {fehler}"))
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
    }
}

/// Ein TCP-Echo auf `127.0.0.1:0` — der „Spielserver“ des Gastgebers.
fn tcp_echo() -> u16 {
    let zuhoerer = TcpListener::bind("127.0.0.1:0").expect("Echo");
    let port = zuhoerer.local_addr().expect("Adresse").port();
    std::thread::spawn(move || {
        for strom in zuhoerer.incoming() {
            let Ok(mut strom) = strom else { break };
            std::thread::spawn(move || {
                let mut block = [0u8; 4096];
                loop {
                    match strom.read(&mut block) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if strom.write_all(&block[..n]).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

/// Der ganze Weg: freigeben, beitreten, über die lokale Adresse spielen.
#[test]
fn echo_kommt_ueber_die_freigegebene_adresse_zurueck() {
    let server = Pruefserver::starten("echo");
    let port = tcp_echo();

    // Ada ist zuerst da: sie wird Administrator und darf freigeben.
    let mut ada = Cli::starten(&server, "Ada", &server.verzeichnis.join("k_Ada"));
    ada.warte_auf("Tippe Text", FRIST);
    ada.warte_auf("Spielnetz: verbunden", FRIST);
    ada.schreibe(&format!("/freigeben {port} tcp Echo"));
    ada.warte_auf("freigegeben: 1", FRIST);

    // Bosse kommt dazu und sieht die Freigabe, ohne danach zu fragen.
    let mut bosse = Cli::starten(&server, "Bosse", &server.verzeichnis.join("k_Bosse"));
    let gesehen = bosse.warte_auf("Echo", FRIST);
    assert!(
        gesehen.contains(&format!("{port}/tcp")),
        "die Freigabe steht mit Port und Art in der Liste:\n{gesehen}"
    );
    assert!(
        gesehen.contains("von Ada"),
        "der Gastgeber wird mit Namen genannt:\n{gesehen}"
    );

    bosse.schreibe("/beitreten 1");
    bosse.warte_auf("lokale Adresse: ", FRIST);
    let lokal = bosse.lokale_adresse();
    assert!(
        lokal.ip().is_loopback(),
        "der lokale Port ist nur auf der Schleife offen: {lokal}"
    );

    // Jetzt „spielt“ der Test: schicken, alles zurückbekommen.
    let mut strom = TcpStream::connect(lokal).expect("Verbindung zum lokalen Port");
    strom
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("Frist");
    strom.write_all(b"hallo sprechwerk").expect("schreiben");
    let mut zurueck = vec![0u8; b"hallo sprechwerk".len()];
    strom.read_exact(&mut zurueck).expect("lesen");
    assert_eq!(zurueck, b"hallo sprechwerk", "das Echo kommt unverändert");

    // Und ein zweites Mal, damit es nicht „einmalig“ funktioniert.
    strom.write_all(b"noch einmal").expect("schreiben");
    let mut zurueck = vec![0u8; b"noch einmal".len()];
    strom.read_exact(&mut zurueck).expect("lesen");
    assert_eq!(zurueck, b"noch einmal");

    bosse.schreibe("/verlassen 1");
    bosse.warte_auf("verlassen: 1", FRIST);
    // Der lokale Port ist danach zu.
    let nachher = TcpStream::connect(lokal);
    assert!(
        nachher.is_err(),
        "nach dem Verlassen darf nichts mehr lauschen: {lokal}"
    );

    ada.schreibe("/ende");
    bosse.schreibe("/ende");
}

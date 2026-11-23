//! Der Client-Tunnel im Betrieb: lokale Ports, Ende zu Ende, echter Rundlauf.
//!
//! Jeder Test startet einen **echten** Serverprozess (wie `spielnetz.rs`) und
//! zwei Tunnel: Ada als Gastgeber, Bosse als Teilnehmer. Was geprüft wird, ist
//! genau das, was ein Spiel erlebt — ein Echo über `127.0.0.1`.
//!
//! # Warum die Hilfen hier noch einmal stehen
//!
//! `tests/common/` wäre die Alternative gewesen; die Hilfen sind aber klein und
//! eng an diesen Ablauf gebunden (Sitzung, Serverstart, Warten). Eine Kopie ist
//! hier billiger als eine gemeinsame Abstraktion über zwei Testbinarys.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_spielnetz::tunnel::ziel;
use sprechwerk_spielnetz::{AngebotId, Art, Lage, Tunnel};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch;

/// Ein laufender Server auf einem freien Port (Spielnetz an).
struct Testserver {
    kind: Child,
    adresse: SocketAddr,
    verzeichnis: PathBuf,
}

impl Drop for Testserver {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
        let _ = std::fs::remove_dir_all(&self.verzeichnis);
    }
}

fn freier_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

impl Testserver {
    /// Wie `spielnetz.rs`: mehrfach versuchen, weil zwischen „Port ist frei“ und
    /// dem Binden ein anderer Test denselben Port erwischen kann.
    fn neu(name: &str) -> Self {
        let mut letzter = String::new();
        for versuch in 0..5 {
            match Self::versuchen(name, versuch) {
                Ok(server) => return server,
                Err(text) => letzter = text,
            }
        }
        panic!("Server startet nicht:\n{letzter}");
    }

    fn versuchen(name: &str, versuch: u32) -> Result<Self, String> {
        let port = freier_port();
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-tunnel-{}-{}-{}-{versuch}",
            std::process::id(),
            name,
            port
        ));
        std::fs::create_dir_all(&verzeichnis).map_err(|f| f.to_string())?;
        let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk"));
        befehl
            .env("SPRECHWERK_DATEN", &verzeichnis)
            .env("SPRECHWERK_PORT", port.to_string())
            .env("SPRECHWERK_BIND", "127.0.0.1")
            .env("SPRECHWERK_NAME", "Tunneltest")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
            .env("SPRECHWERK_SPIELNETZ_AKTIV", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut kind = befehl.spawn().map_err(|f| f.to_string())?;

        // Auf „lauscht auf“ warten — keine Sonde: der erste Ankömmling wird
        // Administrator, eine Testverbindung würde diese Rolle verbrauchen.
        let stdout = kind.stdout.take().expect("stdout");
        let mut leser = std::io::BufReader::new(stdout);
        let mut gesamt = String::new();
        use std::io::BufRead;
        let mut zeile = String::new();
        let mut bereit = false;
        while leser.read_line(&mut zeile).unwrap_or(0) > 0 {
            gesamt.push_str(&zeile);
            if zeile.contains("lauscht auf") {
                bereit = true;
                break;
            }
            zeile.clear();
        }
        if !bereit {
            let _ = kind.kill();
            let _ = kind.wait();
            let mut fehler = String::new();
            if let Some(mut err) = kind.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut err, &mut fehler);
            }
            let _ = std::fs::remove_dir_all(&verzeichnis);
            return Err(format!("{gesamt}{fehler}"));
        }
        std::thread::spawn(move || {
            let mut rest = String::new();
            let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
        });

        Ok(Self {
            kind,
            adresse: format!("127.0.0.1:{port}").parse().expect("Adresse"),
            verzeichnis,
        })
    }
}

/// Ein Mitglied mit Sitzung **und** Spielnetz-Tunnel.
struct Mitglied {
    tunnel: Tunnel,
    lage: watch::Receiver<Lage>,
    kennung: String,
    /// Die Sitzung muss leben, solange die Freigabe läuft: ohne sie endet sie
    /// (`Grund::GastgeberWeg`, `Grund::Verlassen`).
    _sitzung: Verbindung,
}

impl Mitglied {
    async fn neu(server: &Testserver, name: &str) -> Self {
        let schluessel = Schluessel::neu();
        let sitzung = Verbindung::oeffnen(server.adresse, &schluessel, name, None)
            .await
            .unwrap_or_else(|fehler| panic!("{name}: Sitzung: {fehler}"));
        let kennung = schluessel.kennung();
        let (tunnel, lage) = Tunnel::starten(server.adresse, schluessel, &sitzung.zertifikat)
            .await
            .expect("Spielnetz anmelden");
        Self {
            tunnel,
            lage,
            kennung,
            _sitzung: sitzung,
        }
    }

    /// Wartet auf die erste Lage, die die Bedingung erfüllt (höchstens 5 s).
    async fn warte<F>(&mut self, was: &str, bedingung: F) -> Lage
    where
        F: Fn(&Lage) -> bool,
    {
        let frist = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if bedingung(&self.lage.borrow()) {
                return self.lage.borrow().clone();
            }
            let rest = frist.saturating_duration_since(tokio::time::Instant::now());
            assert!(!rest.is_zero(), "Zeitüberschreitung beim Warten auf {was}");
            if tokio::time::timeout(rest, self.lage.changed())
                .await
                .is_err()
            {
                panic!("Zeitüberschreitung beim Warten auf {was}");
            }
        }
    }

    /// Wartet, bis ein Angebot im Raum auftaucht, und gibt seine Nummer.
    async fn angebot_abwarten(&mut self, port: u16) -> AngebotId {
        let lage = self
            .warte(&format!("Angebot auf Port {port}"), |lage| {
                lage.angebote.iter().any(|a| a.port == port)
            })
            .await;
        lage.angebote
            .iter()
            .find(|a| a.port == port)
            .map(|a| a.id)
            .expect("Angebot")
    }
}

/// Ein TCP-Echo auf `127.0.0.1:0` — der „Spielserver“ des Gastgebers.
async fn tcp_echo() -> (u16, tokio::task::JoinHandle<()>) {
    let zuhoerer = TcpListener::bind("127.0.0.1:0").await.expect("Echo");
    let port = zuhoerer.local_addr().expect("Adresse").port();
    let aufgabe = tokio::spawn(async move {
        loop {
            let Ok((mut strom, _)) = zuhoerer.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let (mut lesen, mut schreiben) = strom.split();
                let _ = tokio::io::copy(&mut lesen, &mut schreiben).await;
            });
        }
    });
    (port, aufgabe)
}

/// Ein UDP-Echo auf `127.0.0.1:0`.
async fn udp_echo() -> (u16, tokio::task::JoinHandle<()>) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("Echo");
    let port = socket.local_addr().expect("Adresse").port();
    let aufgabe = tokio::spawn(async move {
        let mut block = vec![0u8; 65535];
        loop {
            let Ok((n, quelle)) = socket.recv_from(&mut block).await else {
                break;
            };
            let _ = socket.send_to(&block[..n], quelle).await;
        }
    });
    (port, aufgabe)
}

/// Zufall für die Nutzlast (kein neues Fremd-Crate: der Test braucht nur Streuung).
fn zufall(anzahl: usize, saat: u64) -> Vec<u8> {
    let mut x = saat | 1;
    (0..anzahl)
        .map(|_| {
            // SplitMix64 — reicht für „sieht aus wie Zufall“.
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as u8
        })
        .collect()
}

/// Eine TCP-Verbindung zum lokalen Port des Teilnehmers.
async fn verbinde(lokal: SocketAddr) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(lokal))
        .await
        .expect("Zeitüberschreitung beim Verbinden")
        .expect("Verbindung zum lokalen Port")
}

/// Ein Rundlauf über einen lokalen Port: schicken, alles zurückbekommen.
///
/// Geschrieben und gelesen wird **gleichzeitig**: 200 KiB in eine Richtung
/// füllen sonst den Empfangspuffer, und alles stande still. Und die
/// Schreibhälfte bleibt offen, bis alles zurück ist — der Server beendet mit
/// der ersten fertigen Richtung den ganzen Strom (`server::leite_strom`), ein
/// halbes Schließen trägt also nicht (siehe Moduldoku von `tunnel`).
async fn echo_rundlauf(lokal: SocketAddr, nutzlast: &[u8]) {
    let strom = verbinde(lokal).await;
    let (mut lesen, mut schreiben) = strom.into_split();
    let daten = nutzlast.to_vec();
    let erwartet = daten.len();

    let hin = async {
        schreiben.write_all(&daten).await.expect("schreiben");
        schreiben.flush().await.expect("flush");
        // `schreiben` lebt bis zum Ende dieses Blocks: erst danach fällt die
        // Schreibhälfte weg und der Strom geht zu.
    };
    let her = async {
        let mut zurueck = vec![0u8; erwartet];
        tokio::time::timeout(Duration::from_secs(20), lesen.read_exact(&mut zurueck))
            .await
            .expect("Zeitüberschreitung beim Lesen")
            .expect("lesen");
        zurueck
    };
    let ((), zurueck) = tokio::join!(hin, her);
    assert_eq!(zurueck, nutzlast, "alles muss unverändert zurückkommen");
}

// ------------------------------------------------------------------- TCP

/// Ada gibt einen TCP-Port frei, Bosse tritt bei und spielt 200 KiB hin und
/// zurück — zweimal gleichzeitig.
#[tokio::test]
async fn tcp_echo_kommt_ueber_den_tunnel_zurueck() {
    let server = Testserver::neu("tcp");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    let (port, _echo) = tcp_echo().await;
    let angebot = ada
        .tunnel
        .anbieten("Echo", port, Art::Tcp)
        .await
        .expect("anbieten");
    // Die eigene Freigabe steht in der eigenen Lage …
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;
    // … und Bosse sieht sie im Raum.
    assert_eq!(bosse.angebot_abwarten(port).await, angebot);

    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");
    assert!(lokal.ip().is_loopback(), "nur die Schleife: {lokal}");
    bosse
        .warte("Teilnahme", |lage| {
            lage.teilnahmen.iter().any(|t| t.angebot == angebot)
        })
        .await;

    // Zwei Verbindungen gleichzeitig, jede mit eigenem Inhalt.
    let eins = zufall(200 * 1024, 7);
    let zwei = zufall(200 * 1024, 11);
    tokio::join!(echo_rundlauf(lokal, &eins), echo_rundlauf(lokal, &zwei));

    // Und danach noch einmal, damit nichts „einmalig“ funktioniert.
    echo_rundlauf(lokal, &zufall(4096, 13)).await;

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

// ------------------------------------------------------------------- UDP

/// Dasselbe für UDP: 50 Datagramme à 1000 Byte je Socket, und jeder Socket
/// bekommt **seine** Antworten.
#[tokio::test]
async fn udp_echo_kommt_ueber_den_tunnel_zurueck() {
    let server = Testserver::neu("udp");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    let (port, _echo) = udp_echo().await;
    let angebot = ada
        .tunnel
        .anbieten("UDP-Echo", port, Art::Udp)
        .await
        .expect("anbieten");
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;
    assert_eq!(bosse.angebot_abwarten(port).await, angebot);

    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");
    assert!(lokal.ip().is_loopback(), "nur die Schleife: {lokal}");

    // Zwei lokale Sockets: jeder bekommt über seinen eigenen Kanal Antwort.
    let eins = UdpSocket::bind("127.0.0.1:0").await.expect("Socket");
    let zwei = UdpSocket::bind("127.0.0.1:0").await.expect("Socket");
    eins.connect(lokal).await.expect("connect");
    zwei.connect(lokal).await.expect("connect");

    // Beide gleichzeitig: senden und zwischendurch lesen. Der Empfangspuffer
    // eines Sockets ist klein (etwa acht Datagramme) — wer erst alles sendet
    // und dann liest, verliert genau die Pakete, die er zählen will.
    let (ergebnis_eins, ergebnis_zwei) =
        tokio::join!(sende_und_sammle(eins, 1, 50), sende_und_sammle(zwei, 2, 50));

    assert!(
        ergebnis_eins.antworten.len() >= 45,
        "mindestens 45 von 50 zurück (Socket 1), waren {}",
        ergebnis_eins.antworten.len()
    );
    assert!(
        ergebnis_zwei.antworten.len() >= 45,
        "mindestens 45 von 50 zurück (Socket 2), waren {}",
        ergebnis_zwei.antworten.len()
    );
    // Jeder Socket sieht seine eigenen Antworten, Byte für Byte.
    for antwort in &ergebnis_eins.antworten {
        assert_eq!(antwort.len(), 1000);
        assert_eq!(antwort[1], 1, "Socket 1 bekommt nur seine Antworten");
        assert!(
            ergebnis_eins.gesendet.iter().any(|s| s == antwort),
            "die Antwort war nie gesendet"
        );
    }
    for antwort in &ergebnis_zwei.antworten {
        assert_eq!(antwort.len(), 1000);
        assert_eq!(antwort[1], 2, "Socket 2 bekommt nur seine Antworten");
        assert!(ergebnis_zwei.gesendet.iter().any(|s| s == antwort));
    }

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

/// Was ein Socket gesendet hat und was zurückkam.
struct UdpRunde {
    gesendet: Vec<Vec<u8>>,
    antworten: Vec<Vec<u8>>,
}

/// Sendet `anzahl` Datagramme und sammelt die Antworten ein.
///
/// Zwischendurch wird gelesen: der Empfangspuffer eines UDP-Sockets ist klein,
/// und ein Datagramm, das dort nicht mehr hineinpasst, ist weg.
async fn sende_und_sammle(socket: UdpSocket, marke: u8, anzahl: usize) -> UdpRunde {
    let mut gesendet = Vec::new();
    let mut antworten = Vec::new();
    let mut block = vec![0u8; 65535];

    for nummer in 0..anzahl {
        let mut nutzlast = vec![nummer as u8; 1000];
        // Byte 0 ist die laufende Nummer, Byte 1 die Marke des Sockets —
        // daran erkennt der Test, wessen Antwort angekommen ist.
        nutzlast[0] = nummer as u8;
        nutzlast[1] = marke;
        socket.send(&nutzlast).await.expect("senden");
        gesendet.push(nutzlast);
        while let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_millis(1), socket.recv(&mut block)).await
        {
            antworten.push(block[..n].to_vec());
        }
    }

    // Den Rest einsammeln; was fünf Sekunden nicht kommt, kommt nicht mehr.
    while antworten.len() < anzahl {
        match tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut block)).await {
            Ok(Ok(n)) => antworten.push(block[..n].to_vec()),
            _ => break,
        }
    }

    UdpRunde {
        gesendet,
        antworten,
    }
}

// --------------------------------------------------------------- Verlassen

/// Nach dem Verlassen ist der lokale TCP-Port zu.
#[tokio::test]
async fn verlassen_schliesst_den_lokalen_port() {
    let server = Testserver::neu("verlassen");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    let (port, _echo) = tcp_echo().await;
    let angebot = ada
        .tunnel
        .anbieten("Echo", port, Art::Tcp)
        .await
        .expect("anbieten");
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;
    assert_eq!(bosse.angebot_abwarten(port).await, angebot);
    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");
    echo_rundlauf(lokal, b"vor dem Verlassen").await;

    bosse.tunnel.verlassen(angebot).await.expect("verlassen");
    bosse
        .warte("keine Teilnahme mehr", |lage| {
            !lage.teilnahmen.iter().any(|t| t.angebot == angebot)
        })
        .await;

    // Der Port ist zu: der Verbindungsversuch scheitert (nicht: hängt).
    let versuch = tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(lokal)).await;
    match versuch {
        Ok(Ok(_)) => panic!("der lokale Port {lokal} ist noch offen"),
        Ok(Err(_)) => {}
        Err(_) => panic!("der Verbindungsversuch hing statt zu scheitern"),
    }

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

// --------------------------------------------------------------- Entfernen

/// Ada wirft Bosse hinaus — seine offene Verbindung endet.
#[tokio::test]
async fn entfernen_beendet_die_offene_verbindung() {
    let server = Testserver::neu("entfernen");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    let (port, _echo) = tcp_echo().await;
    let angebot = ada
        .tunnel
        .anbieten("Echo", port, Art::Tcp)
        .await
        .expect("anbieten");
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;
    assert_eq!(bosse.angebot_abwarten(port).await, angebot);
    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");

    // Eine offene Verbindung: ein Byte hin, ein Byte zurück.
    let mut strom = verbinde(lokal).await;
    strom.write_all(b"x").await.expect("schreiben");
    strom.flush().await.expect("flush");
    let mut byte = [0u8; 1];
    tokio::time::timeout(Duration::from_secs(5), strom.read_exact(&mut byte))
        .await
        .expect("Zeitüberschreitung")
        .expect("lesen");
    assert_eq!(&byte, b"x");

    // Ada entfernt Bosse (Kennung aus seiner Sicht: die eigene).
    let kennung = bosse.kennung.clone();
    ada.tunnel
        .entfernen(angebot, &kennung)
        .await
        .expect("entfernen");

    // Die Verbindung endet: Lesen liefert 0 (zu) oder einen Fehler — aber
    // binnen zwei Sekunden.
    let mut block = [0u8; 16];
    let ergebnis = tokio::time::timeout(Duration::from_secs(2), strom.read(&mut block)).await;
    match ergebnis {
        Ok(Ok(0)) | Ok(Err(_)) => {}
        Ok(Ok(n)) => panic!("unerwartet {n} Byte nach dem Entfernen"),
        Err(_) => panic!("die Verbindung blieb nach dem Entfernen offen"),
    }

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

// -------------------------------------------------------------- Port belegt

/// Bosse hält den Wunschport selbst — der Tunnel nimmt einen anderen, und der
/// funktioniert.
#[tokio::test]
async fn belegter_wunschport_bekommt_einen_anderen() {
    let server = Testserver::neu("belegt");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    // Den Wunschport sofort belegen und behalten: freigeben und später neu
    // binden wäre ein Rennen mit parallel laufenden Tests.
    let besetzt = TcpListener::bind("127.0.0.1:0").await.expect("Wunschport");
    let freier = besetzt.local_addr().expect("Adresse").port();

    // Ada gibt ihn frei. Auf einem Rechner lauscht dort zunächst niemand —
    // Bosse belegt ihn gleich selbst; die Schleife ist geteilt, also erreicht
    // Adas Tunnel genau diesen Zuhörer.
    let angebot = ada
        .tunnel
        .anbieten("Belegt", freier, Art::Tcp)
        .await
        .expect("anbieten");
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;

    tokio::spawn(async move {
        loop {
            let Ok((mut strom, _)) = besetzt.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let (mut lesen, mut schreiben) = strom.split();
                let _ = tokio::io::copy(&mut lesen, &mut schreiben).await;
            });
        }
    });

    assert_eq!(bosse.angebot_abwarten(freier).await, angebot);
    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");
    assert_ne!(lokal.port(), freier, "der Wunschport ist belegt");
    assert!(lokal.ip().is_loopback(), "nur die Schleife: {lokal}");

    // Und der andere Port trägt wirklich.
    echo_rundlauf(lokal, &zufall(2048, 23)).await;

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

// ------------------------------------------------------------------- Rand

/// Der Gastgeber verbindet nur `127.0.0.1` — egal, welcher Port freigegeben ist.
#[tokio::test]
async fn der_gastgeber_verbindet_nur_localhost() {
    for port in [1u16, 25565, 65535] {
        let adresse = ziel(port);
        assert_eq!(adresse.port(), port);
        assert_eq!(adresse.ip().to_string(), "127.0.0.1");
        assert!(adresse.ip().is_loopback());
    }
}

// --------------------------------------------------------- halbes Schließen

/// Ein Dienst, der erst **alles** liest und dann antwortet (wie eine Anfrage
/// mit `shutdown(Write)`): der Tunnel darf die Antwortrichtung nicht abbrechen,
/// nur weil die Anfragerichtung zu Ende ist.
#[tokio::test]
async fn halbes_schliessen_laesst_die_antwort_durch() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let server = Testserver::neu("halb");
    let mut ada = Mitglied::neu(&server, "Ada").await;
    let mut bosse = Mitglied::neu(&server, "Bosse").await;

    let dienst = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = dienst.local_addr().expect("Adresse").port();
    let _dienst = tokio::spawn(async move {
        while let Ok((mut tcp, _)) = dienst.accept().await {
            tokio::spawn(async move {
                let mut alles = Vec::new();
                if tcp.read_to_end(&mut alles).await.is_ok() {
                    let _ = tcp
                        .write_all(format!("len={}", alles.len()).as_bytes())
                        .await;
                    let _ = tcp.shutdown().await;
                }
            });
        }
    });

    let angebot = ada
        .tunnel
        .anbieten("Anfrage", port, Art::Tcp)
        .await
        .expect("anbieten");
    ada.warte("eigene Freigabe", |lage| lage.eigene.contains(&angebot))
        .await;
    assert_eq!(bosse.angebot_abwarten(port).await, angebot);
    let lokal = bosse.tunnel.beitreten(angebot).await.expect("beitreten");

    let mut tcp = verbinde(lokal).await;
    tcp.write_all(&zufall(50_000, 3)).await.expect("schreiben");
    tcp.shutdown().await.expect("Schreibseite zu");
    let mut antwort = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut antwort))
        .await
        .expect("Antwort kommt")
        .expect("lesen");
    assert_eq!(antwort, b"len=50000");

    bosse.tunnel.beenden().await;
    ada.tunnel.beenden().await;
}

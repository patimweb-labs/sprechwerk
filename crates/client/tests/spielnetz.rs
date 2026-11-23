//! Das Spielnetz über die Fassade: Einstellung an, Sitzung steht, Modul läuft.
//!
//! Geprüft wird der Weg, den eine Oberfläche nimmt (P4): Einstellung
//! gespeichert → [`Aktion::Verbinden`] → der gemeldete [`Zustand`] trägt
//! `Spielnetz::Bereit`. Der Tunnel selbst wird in `crates/server` und
//! `crates/cli` Ende zu Ende geprüft; hier geht es um das Einklinken in den
//! Client-Kern — ohne dass `anwendung.rs` davon etwas weiß.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use sprechwerk_client::einstellungen::Einstellungen;
use sprechwerk_client::sitzung::{Aktion, Ergebnis, Meldung};
use sprechwerk_client::Client;
use sprechwerk_ipc::{Spielnetz, Zustand};
use sprechwerk_server::{dienst, konfig::Konfig};

/// So lange darf eine Meldung auf sich warten lassen.
const FRIST: Duration = Duration::from_secs(20);

fn freier_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn verzeichnis(name: &str) -> PathBuf {
    let pfad = std::env::temp_dir().join(format!(
        "sprechwerk-client-spielnetz-{}-{name}-{}",
        std::process::id(),
        freier_port()
    ));
    std::fs::create_dir_all(&pfad).unwrap();
    pfad
}

/// Ein echter Dienst im Hintergrundfaden — mit Spielnetz.
struct Pruefserver {
    adresse: SocketAddr,
}

impl Pruefserver {
    fn starten() -> Self {
        let verzeichnis = verzeichnis("server");
        let port = freier_port();
        let mut konfig = Konfig::default();
        konfig.server.name = "Prüfwerk".to_string();
        konfig.server.bind = "127.0.0.1".to_string();
        konfig.server.port = port;
        konfig.server.daten = verzeichnis.clone();
        konfig.sicherung.aktiv = false;
        konfig.verwaltung.aktiv = false;
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
        }
    }
}

/// Meldungen des Clients einsammeln.
struct Meldungen(Receiver<Meldung>);

impl Meldungen {
    /// Wartet auf das Ergebnis einer Aktion — und gibt die Zustände mit, die
    /// bis dahin gemeldet wurden (sie können vor der Antwort da sein).
    fn rueck(&self, frist: Duration) -> (String, Result<Ergebnis, String>, Vec<Zustand>) {
        let ende = Instant::now() + frist;
        let mut staende = Vec::new();
        loop {
            let rest = ende.saturating_duration_since(Instant::now());
            assert!(!rest.is_zero(), "keine Antwort innerhalb von {frist:?}");
            match self.0.recv_timeout(rest).expect("Meldung") {
                Meldung::Rueck { aktion, ergebnis } => {
                    return (aktion.to_string(), ergebnis, staende)
                }
                Meldung::Zustand(zustand) => staende.push(*zustand),
            }
        }
    }

    /// Wartet, bis ein Zustand die Bedingung erfüllt; `None` nach Frist.
    fn warte_auf_stand(
        &self,
        frist: Duration,
        bedingung: impl Fn(&Zustand) -> bool,
    ) -> Option<Zustand> {
        let ende = Instant::now() + frist;
        loop {
            let rest = ende.saturating_duration_since(Instant::now());
            if rest.is_zero() {
                return None;
            }
            match self.0.recv_timeout(rest) {
                Ok(Meldung::Zustand(zustand)) => {
                    if bedingung(&zustand) {
                        return Some(*zustand);
                    }
                }
                Ok(Meldung::Rueck { .. }) => {}
                // Frist um oder Leitung zu: kein Bereit in dieser Zeit.
                Err(_) => return None,
            }
        }
    }
}

/// Mit `--spielnetz`-Einstellung verbindet sich das Modul von selbst.
#[test]
fn verbinden_mit_eingeschaltetem_spielnetz_wird_bereit() {
    let server = Pruefserver::starten();
    let d = verzeichnis("client");

    // Die Einstellung steht **vor** dem Start in der Datei: so kommt sie im
    // Fenster an (dort liest `Client::starten` sie einmal).
    let e = Einstellungen {
        server: server.adresse.to_string(),
        spielnetz: true,
        ..Einstellungen::default()
    };
    e.speichern(&d.join("einstellungen.json"))
        .expect("Einstellungen");

    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let client = Client::starten(&d, &d.join("zwischen"), move |m| {
        let _ = tx.lock().unwrap().send(m);
    });
    let meldungen = Meldungen(rx);

    // Ohne Sitzung wartet das Modul — eingeschaltet, aber nichts offen.
    assert_eq!(client.zustand().spielnetz, Spielnetz::Wartet);

    client.ausfuehren(Aktion::Verbinden {
        server: server.adresse.to_string(),
        name: "Ada".into(),
        passwort: None,
    });
    let (aktion, ergebnis, gesehen) = meldungen.rueck(FRIST);
    assert_eq!(aktion, "verbinden");
    assert!(ergebnis.is_ok(), "verbinden: {ergebnis:?}");

    // Und jetzt verbindet sich das Spielnetz von allein; der Zustand sagt es.
    // Der Zustand kann schon vor der Antwort auf `Verbinden` gemeldet worden
    // sein, deshalb zählt beides.
    let bereit = |z: &&Zustand| matches!(z.spielnetz, Spielnetz::Bereit { .. });
    let zustand = gesehen
        .iter()
        .find(bereit)
        .cloned()
        .or_else(|| meldungen.warte_auf_stand(FRIST, |z| bereit(&z)))
        .unwrap_or_else(|| {
            panic!(
                "das Spielnetz wurde nicht bereit: {:?}",
                client.zustand().spielnetz
            )
        });
    let Spielnetz::Bereit { angebote, .. } = &zustand.spielnetz else {
        panic!("erwartet: bereit");
    };
    assert!(angebote.is_empty(), "niemand hat etwas freigegeben");
    assert!(
        zustand.verbindung.is_some(),
        "der Zustand bleibt der der Sitzung"
    );

    // `Client::zustand()` sagt dasselbe — die Oberfläche liest von dort.
    assert!(matches!(
        client.zustand().spielnetz,
        Spielnetz::Bereit { .. }
    ));

    let _ = std::fs::remove_dir_all(&d);
}

/// Ohne Einstellung bleibt das Modul aus — kein Port, keine zweite Verbindung.
#[test]
fn ohne_einstellung_bleibt_das_modul_aus() {
    let server = Pruefserver::starten();
    let d = verzeichnis("aus");

    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let client = Client::starten(&d, &d.join("zwischen"), move |m| {
        let _ = tx.lock().unwrap().send(m);
    });
    let meldungen = Meldungen(rx);
    assert_eq!(client.zustand().spielnetz, Spielnetz::Aus);

    client.ausfuehren(Aktion::Verbinden {
        server: server.adresse.to_string(),
        name: "Ada".into(),
        passwort: None,
    });
    let (_, ergebnis, _) = meldungen.rueck(FRIST);
    assert!(ergebnis.is_ok(), "verbinden: {ergebnis:?}");

    // Auch nach dem Verbinden bleibt es aus: die Einstellung entscheidet.
    assert_eq!(client.zustand().spielnetz, Spielnetz::Aus);

    let _ = std::fs::remove_dir_all(&d);
}

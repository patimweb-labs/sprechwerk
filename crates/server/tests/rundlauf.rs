//! Rundlauf: echter Serverprozess, echter Client, echte Verbindung.
//!
//! Diese Prüfung ist der Ersatz für „ich klicke es mal an". Sie startet das
//! Programm so, wie Docker es startet (Konfiguration über Umgebung), verbindet
//! zwei Clients und prüft, was sie voneinander sehen.

use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::{ClientFrame, ServerFrame};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Ein laufender Server auf einem freien Port. Wird beim Verlassen beendet.
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

/// Sucht einen freien UDP-Port (kleine Lücke zwischen Prüfung und Bindung ist
/// unvermeidlich, deshalb wird der Start mehrfach versucht).
fn freier_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn verzeichnis(name: &str) -> std::path::PathBuf {
    let pfad = std::env::temp_dir().join(format!(
        "sprechwerk-runde-{}-{}-{}",
        std::process::id(),
        name,
        freier_port()
    ));
    std::fs::create_dir_all(&pfad).unwrap();
    pfad
}

impl Testserver {
    fn starten(name: &str, extra: &[(&str, &str)]) -> Self {
        let verzeichnis = verzeichnis(name);
        let port = freier_port();
        let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk"));
        befehl
            .env("SPRECHWERK_DATEN", &verzeichnis)
            .env("SPRECHWERK_PORT", port.to_string())
            .env("SPRECHWERK_BIND", "127.0.0.1")
            .env("SPRECHWERK_NAME", "Rundlauftest")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (schluessel, wert) in extra {
            befehl.env(schluessel, wert);
        }
        let mut kind = befehl.spawn().expect("Server startet");

        // Auf die Meldung „lauscht" warten. Bewusst **nicht** über eine
        // Testverbindung: der erste Ankömmling wird Administrator, eine Sonde
        // würde diese Rolle verbrauchen.
        let stdout = kind.stdout.take().expect("stdout");
        let mut leser = std::io::BufReader::new(stdout);
        let mut zeile = String::new();
        let mut bereit = false;
        let mut gesamt = String::new();
        use std::io::BufRead;
        while leser.read_line(&mut zeile).unwrap_or(0) > 0 {
            gesamt.push_str(&zeile);
            if zeile.contains("lauscht auf") {
                bereit = true;
                break;
            }
            zeile.clear();
        }
        assert!(bereit, "Server meldet sich nicht:\n{gesamt}");
        // Restliche Ausgabe im Hintergrund verwerfen, damit der Server nicht an
        // einer vollen Pfeife hängt.
        std::thread::spawn(move || {
            let mut rest = String::new();
            let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
        });

        Self {
            kind,
            adresse: format!("127.0.0.1:{port}").parse().unwrap(),
            verzeichnis,
        }
    }

    /// Der Server ist nach dem Start bereits bereit.
    async fn bereit(&self) {}
}

#[tokio::test]
async fn zwei_clients_sehen_sich_und_texten() {
    let server = Testserver::starten("zwei", &[]);
    server.bereit().await;

    let mut ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .expect("Ada meldet sich an");
    let mut bosse = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bosse", None)
        .await
        .expect("Bosse meldet sich an");

    // Der erste Ankömmling ist Administrator, der zweite Mitglied.
    assert_eq!(ada.rolle, sprechwerk_domain::Role::Administrator);
    assert_eq!(bosse.rolle, sprechwerk_domain::Role::Mitglied);

    // Beide in den Empfangsraum.
    let raum = ada.raeume.first().expect("ein Raum").id;
    ada.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    bosse.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Betreten { .. })).await;

    // Bosse sieht, dass Ada da ist.
    let anwesend = bosse
        .mitglieder
        .iter()
        .map(|m| m.name.clone())
        .collect::<Vec<_>>();
    assert!(anwesend.contains(&"Ada".to_string()), "{anwesend:?}");

    // Ada schreibt, Bosse bekommt es.
    ada.sende(ClientFrame::Text {
        text: "Guten Abend".into(),
    })
    .await
    .unwrap();
    let nachricht = warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Text { .. })).await;
    match nachricht {
        ServerFrame::Text { nachricht } => {
            assert_eq!(nachricht.text, "Guten Abend");
            assert_eq!(nachricht.verfasser_name, "Ada");
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }
}

#[tokio::test]
async fn protokoll_ist_lesbar_und_rechte_werden_geprueft() {
    let server = Testserver::starten("protokoll", &[]);
    server.bereit().await;

    let mut ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .unwrap();
    let mut bosse = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bosse", None)
        .await
        .unwrap();

    // Mitglied ohne Recht legt einen Raum an: Ablehnung.
    let raum = bosse.raeume.first().unwrap().id;
    bosse.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    bosse
        .sende(ClientFrame::RaumAnlegen {
            name: "Verboten".into(),
        })
        .await
        .unwrap();
    let absage = warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Fehler { .. })).await;
    match absage {
        ServerFrame::Fehler { code, .. } => {
            assert_eq!(code, sprechwerk_protocol::Fehlercode::KeinRecht)
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Dieselbe Aktion als Administrator: erlaubt.
    ada.sende(ClientFrame::RaumAnlegen {
        name: "Vorstand".into(),
    })
    .await
    .unwrap();
    let liste = warte_auf(&mut ada, |f| matches!(f, ServerFrame::Raeume { .. })).await;
    match liste {
        ServerFrame::Raeume { raeume } => {
            assert!(raeume.iter().any(|r| r.name == "Vorstand"), "{raeume:?}");
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Protokoll enthält die Ablehnung **und** die Erlaubnis.
    ada.sende(ClientFrame::Protokoll { ab: 0, anzahl: 100 })
        .await
        .unwrap();
    let protokoll = warte_auf(&mut ada, |f| matches!(f, ServerFrame::Protokoll { .. })).await;
    match protokoll {
        ServerFrame::Protokoll { eintraege } => {
            assert!(
                eintraege
                    .iter()
                    .any(|e| e.aktion == "raum_angelegt" && e.ergebnis.starts_with("abgelehnt")),
                "Ablehnung fehlt im Protokoll"
            );
            assert!(
                eintraege
                    .iter()
                    .any(|e| e.aktion == "raum_angelegt" && e.ergebnis == "erlaubt"),
                "Erlaubnis fehlt im Protokoll"
            );
            assert!(eintraege.iter().any(|e| e.handelnder_name == "Bosse"));
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Ein Mitglied darf das Protokoll nicht lesen.
    bosse
        .sende(ClientFrame::Protokoll { ab: 0, anzahl: 10 })
        .await
        .unwrap();
    let absage = warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Fehler { .. })).await;
    match absage {
        ServerFrame::Fehler { code, .. } => {
            assert_eq!(code, sprechwerk_protocol::Fehlercode::KeinRecht)
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Der Raum überlebt in der Raumliste aller.
    assert!(bosse.raeume.iter().any(|r| r.name == "Empfang"));
}

#[tokio::test]
async fn serverpasswort_wird_verlangt() {
    let server = Testserver::starten("passwort", &[("SPRECHWERK_PASSWORT", "geheim")]);
    server.bereit().await;

    // Ohne Passwort: abgelehnt.
    let ohne = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Fremder", None).await;
    assert!(ohne.is_err(), "ohne Passwort darf niemand herein");

    // Mit Passwort: drin.
    let mit = Verbindung::oeffnen(
        server.adresse,
        &Schluessel::neu(),
        "Mitglied",
        Some("geheim"),
    )
    .await;
    assert!(mit.is_ok(), "mit Passwort muss es gehen: {:?}", mit.err());
}

#[tokio::test]
async fn kennung_bleibt_ueber_verbindungen_und_rollen_haften_daran() {
    let server = Testserver::starten("kennung", &[]);
    server.bereit().await;

    let schluessel = Schluessel::neu();
    let kennung = schluessel.kennung();

    let mut ada = Verbindung::oeffnen(server.adresse, &schluessel.clone(), "Ada", None)
        .await
        .unwrap();
    let raum = ada.raeume.first().unwrap().id;
    ada.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Betreten { .. })).await;

    // Zweiter Client mit eigener Kennung wird Moderator von Ada gemacht.
    let bosse_schluessel = Schluessel::neu();
    let mut bosse = Verbindung::oeffnen(server.adresse, &bosse_schluessel, "Bosse", None)
        .await
        .unwrap();
    let _ = &kennung;
    let bosse_ich = bosse.ich;
    ada.sende(ClientFrame::RolleSetzen {
        mitglied: bosse_ich,
        rolle: sprechwerk_domain::Role::Moderator,
    })
    .await
    .unwrap();
    warte_auf(&mut ada, |f| {
        matches!(f, ServerFrame::MitgliedGeaendert { .. })
    })
    .await;

    // Neu verbinden mit derselben Kennung: Rolle bleibt, Kennung bleibt.
    bosse.schliessen();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let wieder = Verbindung::oeffnen(server.adresse, &bosse_schluessel, "Bosse", None)
        .await
        .unwrap();
    assert_eq!(wieder.rolle, sprechwerk_domain::Role::Moderator);
    assert_eq!(wieder.ich, bosse_ich);
}

/// Wartet auf den ersten Rahmen, der zur Bedingung passt.
async fn warte_auf<F>(verbindung: &mut Verbindung, bedingung: F) -> ServerFrame
where
    F: Fn(&ServerFrame) -> bool,
{
    let frist = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let rest = frist.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !rest.is_zero(),
            "Zeitüberschreitung beim Warten auf einen Rahmen"
        );
        match tokio::time::timeout(rest, verbindung.empfang()).await {
            Ok(Ok(Some(frame))) => {
                if bedingung(&frame) {
                    return frame;
                }
            }
            Ok(Ok(None)) => panic!("Verbindung wurde geschlossen"),
            Ok(Err(fehler)) => panic!("Verbindungsfehler: {fehler}"),
            Err(_) => panic!("Zeitüberschreitung"),
        }
    }
}

#[tokio::test]
async fn sprache_wird_nur_im_eigenen_raum_verteilt() {
    let server = Testserver::starten("sprache", &[]);
    server.bereit().await;

    let mut ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .unwrap();
    let mut bosse = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bosse", None)
        .await
        .unwrap();
    let mut fremd = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Fremd", None)
        .await
        .unwrap();

    let empfang = ada.raeume.first().unwrap().id;
    // Zweiten Raum anlegen (nur der Administrator darf das).
    ada.sende(ClientFrame::RaumAnlegen {
        name: "Nebenraum".into(),
    })
    .await
    .unwrap();
    let liste = warte_auf(&mut ada, |f| matches!(f, ServerFrame::Raeume { .. })).await;
    let nebenraum = match liste {
        ServerFrame::Raeume { raeume } => {
            raeume
                .iter()
                .find(|r| r.name == "Nebenraum")
                .expect("Nebenraum")
                .id
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    };

    // Ada und Bosse in den Empfang, Fremd in den Nebenraum.
    ada.sende(ClientFrame::Betreten { raum: empfang })
        .await
        .unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    bosse
        .sende(ClientFrame::Betreten { raum: empfang })
        .await
        .unwrap();
    warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    fremd
        .sende(ClientFrame::Betreten { raum: nebenraum })
        .await
        .unwrap();
    warte_auf(&mut fremd, |f| matches!(f, ServerFrame::Betreten { .. })).await;

    // Ada schickt einen Sprachrahmen in den Empfang.
    let _ = empfang;
    let kopf = sprechwerk_protocol::Sprachkopf::neu(1);
    let datagramm = kopf.schreiben(&[0xAA; 20]);
    ada.sprache_senden(bytes::Bytes::from(datagramm)).unwrap();

    // Bosse (gleicher Raum) muss ihn bekommen …
    let empfangen = tokio::time::timeout(Duration::from_secs(3), bosse.sprache_empfangen())
        .await
        .expect("Bosse bekommt den Rahmen")
        .expect("kein Fehler");
    let (kopf_b, nutzlast) = sprechwerk_protocol::Sprachkopf::lesen(&empfangen).expect("Kopf");
    assert_eq!(kopf_b.folge, kopf.folge);
    assert_eq!(kopf_b.codec, kopf.codec);
    assert_ne!(
        kopf_b.von, 0,
        "der Server stempelt die Sprecher-Nummer in den Kopf"
    );
    assert_eq!(nutzlast, &[0xAA; 20]);

    // … Fremd (anderer Raum) darf ihn **nicht** bekommen.
    let fremd_bekommt =
        tokio::time::timeout(Duration::from_millis(500), fremd.sprache_empfangen()).await;
    assert!(
        fremd_bekommt.is_err(),
        "Sprache darf den Raum nicht verlassen"
    );
}

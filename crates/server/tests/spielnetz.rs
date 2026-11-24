//! Das Spielnetz im Betrieb: Schalter, Anmeldung, Weiterleitung, Ende.
//!
//! Jeder Test startet einen **echten** Serverprozess (wie `rundlauf.rs` und
//! `anwesenheit.rs`) — die Anmeldung, das Aushandeln des ALPN und die
//! Weiterleitung sind genau die Stellen, an denen eine Attrappe nichts beweist.
//! Zwei Mitglieder sprechen dabei über **zwei** Verbindungen: die Sitzung
//! (`sprechwerk/1`) und das Spielnetz (`sprechwerk-spielnetz/1`), auf demselben
//! UDP-Port (ADR 0016 §2).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use bytes::Bytes;
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::{ClientFrame, ServerFrame};
use sprechwerk_spielnetz::{
    protokoll, Anmeldefehler, Art, ClientRahmen, Datagrammkopf, Fehler, Grund, ServerRahmen,
    Steuerung, StromKopf,
};
use tokio::io::AsyncWriteExt;

/// Ein laufender Server auf einem freien Port.
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
    /// Server mit Spielnetz **an** (das ist der Regelfall dieser Tests).
    ///
    /// Der Start wird mehrfach versucht: zwischen „Port ist frei" und dem
    /// Binden liegt ein Moment, in dem ein anderer Test denselben Port
    /// erwischt. Statt daran zu scheitern, wird es noch einmal probiert.
    fn neu(name: &str, extra: &[(&str, &str)]) -> Self {
        let mut letzter = String::new();
        for versuch in 0..5 {
            match Self::versuchen(name, versuch, extra) {
                Ok(server) => return server,
                Err(text) => letzter = text,
            }
        }
        panic!("Server startet nicht:\n{letzter}");
    }

    fn versuchen(name: &str, versuch: u32, extra: &[(&str, &str)]) -> Result<Self, String> {
        let port = freier_port();
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-spielnetz-{}-{}-{}-{versuch}",
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
            .env("SPRECHWERK_NAME", "Spielnetztest")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (schluessel, wert) in extra {
            befehl.env(schluessel, wert);
        }
        let mut kind = befehl.spawn().map_err(|f| f.to_string())?;

        // Auf „lauscht auf" warten — keine Sonde: der erste Ankömmling wird
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
            // Start gescheitert (Port belegt?): aufräumen und noch einmal.
            let _ = kind.kill();
            let _ = kind.wait();
            let mut fehler = String::new();
            if let Some(mut err) = kind.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut err, &mut fehler);
            }
            let _ = std::fs::remove_dir_all(&verzeichnis);
            return Err(format!("{gesamt}{fehler}"));
        }
        // Restliche Ausgabe im Hintergrund verwerfen (volle Pfeife würde den
        // Server anhalten).
        std::thread::spawn(move || {
            let mut rest = String::new();
            let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
        });

        Ok(Self {
            kind,
            adresse: format!("127.0.0.1:{port}").parse().unwrap(),
            verzeichnis,
        })
    }
}

/// Wartet auf den ersten Steuerrahmen, der zur Bedingung passt (höchstens 5 s).
async fn warte_auf<F>(steuerung: &mut Steuerung, bedingung: F) -> ServerRahmen
where
    F: Fn(&ServerRahmen) -> bool,
{
    let frist = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let rest = frist.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !rest.is_zero(),
            "Zeitüberschreitung beim Warten auf einen Spielnetz-Rahmen"
        );
        let rahmen = tokio::time::timeout(rest, steuerung.empfang())
            .await
            .expect("Zeitüberschreitung")
            .expect("Lesefehler")
            .expect("Strom beendet");
        if bedingung(&rahmen) {
            return rahmen;
        }
    }
}

/// Wartet auf den ersten Sitzungsrahmen, der zur Bedingung passt.
async fn warte_auf_sitzung<F>(verbindung: &mut Verbindung, bedingung: F) -> ServerFrame
where
    F: Fn(&ServerFrame) -> bool,
{
    let frist = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let rest = frist.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !rest.is_zero(),
            "Zeitüberschreitung beim Warten auf einen Sitzungsrahmen"
        );
        let rahmen = tokio::time::timeout(rest, verbindung.empfang())
            .await
            .expect("Zeitüberschreitung")
            .expect("Lesefehler")
            .expect("Sitzung beendet");
        if bedingung(&rahmen) {
            return rahmen;
        }
    }
}

/// Liest einen Stromkopf, auch wenn er mit den ersten Nutzbytes zusammen ankommt.
async fn lese_kopf(recv: &mut quinn::RecvStream, puffer: &mut Vec<u8>) -> StromKopf {
    loop {
        if let Some((kopf, verbraucht)) = protokoll::lese::<StromKopf>(puffer).expect("Rahmen") {
            puffer.drain(..verbraucht);
            return kopf;
        }
        let mut block = [0u8; 4096];
        let n = recv
            .read(&mut block)
            .await
            .expect("Strom lesbar")
            .expect("Strom zu");
        puffer.extend_from_slice(&block[..n]);
    }
}

/// Liest genau `anzahl` Bytes, zuerst aus dem Puffer.
async fn lese_bytes(recv: &mut quinn::RecvStream, puffer: &mut Vec<u8>, anzahl: usize) -> Vec<u8> {
    while puffer.len() < anzahl {
        let mut block = [0u8; 4096];
        let n = recv
            .read(&mut block)
            .await
            .expect("Strom lesbar")
            .expect("Strom zu");
        puffer.extend_from_slice(&block[..n]);
    }
    puffer.drain(..anzahl).collect()
}

/// Meldet ein Mitglied im Spielnetz an (Sitzung muss schon stehen).
async fn anmelden(
    adresse: SocketAddr,
    schluessel: &Schluessel,
    fingerabdruck: &str,
) -> (Steuerung, sprechwerk_spielnetz::Grenzen) {
    tokio::time::timeout(
        Duration::from_secs(5),
        sprechwerk_spielnetz::anmelden(adresse, schluessel, fingerabdruck),
    )
    .await
    .expect("Zeitüberschreitung bei der Spielnetz-Anmeldung")
    .expect("Spielnetz-Anmeldung")
}

// --------------------------------------------------------------- Schalter

#[tokio::test]
async fn ohne_schalter_wird_das_modul_nicht_angeboten() {
    let server = Testserver::neu("aus", &[]); // ohne SPRECHWERK_SPIELNETZ_AKTIV
    let schluessel = Schluessel::neu();
    let sitzung = Verbindung::oeffnen(server.adresse, &schluessel, "Ada", None)
        .await
        .expect("Sitzung");

    let fehler = sprechwerk_spielnetz::anmelden(server.adresse, &schluessel, &sitzung.zertifikat)
        .await
        .expect_err("ohne Schalter darf es keine Spielnetz-Verbindung geben");
    assert!(
        matches!(fehler, Anmeldefehler::NichtAngeboten),
        "erwartet: NichtAngeboten, bekommen: {fehler:?}"
    );
    // Und der Text ist die Auskunft für den Menschen (ADR 0016 §3).
    assert!(fehler.to_string().contains("kein Spielnetz"), "{fehler}");
}

// -------------------------------------------------------------- Anmeldung

#[tokio::test]
async fn ohne_sitzung_keine_spielnetz_anmeldung() {
    let server = Testserver::neu("ohne-sitzung", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    // Eine Kennung, die in der Sitzung nie angemeldet war — sie kennt den
    // Fingerabdruck des Servers (aus einem anderen Client), darf ihn prüfen
    // und wird trotzdem abgewiesen: ohne Sitzung kein Spielnetz.
    let fremd = Schluessel::neu();
    let sitzung = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .expect("Sitzung");

    let fehler = sprechwerk_spielnetz::anmelden(server.adresse, &fremd, &sitzung.zertifikat)
        .await
        .expect_err("ohne Sitzung darf es keine Spielnetz-Verbindung geben");
    match fehler {
        Anmeldefehler::Sonst(inner) => assert!(
            inner.to_string().contains(Fehler::NichtAngemeldet.text()),
            "erwartet: „{}“, bekommen: {inner}",
            Fehler::NichtAngemeldet.text()
        ),
        anderes => panic!("erwartet: Ablehnung, bekommen: {anderes:?}"),
    }
}

#[tokio::test]
async fn ein_gast_bekommt_kein_spielnetz() {
    let server = Testserver::neu("gast", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let mut ada = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");
    assert_eq!(ada.rolle, sprechwerk_domain::Role::Administrator);

    // Ada macht Bosse zum Gast (SPEC §5, ADR 0016 §8).
    ada.sende(ClientFrame::RolleSetzen {
        mitglied: bosse.ich,
        rolle: sprechwerk_domain::Role::Gast,
    })
    .await
    .expect("Rolle setzen");
    warte_auf_sitzung(
        &mut ada,
        |f| matches!(f, ServerFrame::MitgliedGeaendert { mitglied } if mitglied.id == bosse.ich),
    )
    .await;

    let fehler = sprechwerk_spielnetz::anmelden(server.adresse, &bosse_k, &bosse.zertifikat)
        .await
        .expect_err("ein Gast darf kein Spielnetz benutzen");
    match fehler {
        Anmeldefehler::Sonst(inner) => assert!(
            inner.to_string().contains(Fehler::KeinRecht.text()),
            "erwartet: „{}“, bekommen: {inner}",
            Fehler::KeinRecht.text()
        ),
        anderes => panic!("erwartet: Ablehnung, bekommen: {anderes:?}"),
    }
}

// ------------------------------------------------------------------- TCP

#[tokio::test]
async fn ein_tcp_strom_wird_weitergeleitet() {
    let server = Testserver::neu("tcp", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let ada_sitzung = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse_sitzung = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");

    assert_eq!(
        ada_sitzung.zertifikat, bosse_sitzung.zertifikat,
        "beide sehen dasselbe Zertifikat (TOFU, ADR 0002 §3)"
    );
    let (mut ada, grenzen) = anmelden(server.adresse, &ada_k, &ada_sitzung.zertifikat).await;
    let (mut bosse, _) = anmelden(server.adresse, &bosse_k, &bosse_sitzung.zertifikat).await;
    // Die Grenzen kommen beim Willkommen mit (ADR 0016 §7).
    assert_eq!(grenzen.stroeme_je_teilnehmer, 32);

    // Ada gibt einen Port frei.
    ada.sende(ClientRahmen::Anbieten {
        name: "Minecraft".into(),
        port: 25565,
        art: Art::Tcp,
    })
    .await
    .expect("Anbieten");
    let angebot = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Angeboten { .. })).await {
        ServerRahmen::Angeboten { angebot } => angebot,
        anderes => panic!("unerwartet: {anderes:?}"),
    };

    // Bosse sieht die Freigabe im Raum und tritt bei.
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Angebote { liste } if liste.iter().any(|a| a.id == angebot)),
    )
    .await;
    bosse
        .sende(ClientRahmen::Beitreten { angebot })
        .await
        .expect("Beitreten");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beigetreten { angebot: a } if *a == angebot),
    )
    .await;

    // Bosse öffnet einen Strom: erst der Kopf, dann Nutzbytes (Geheimtext).
    let (mut bosse_send, mut bosse_recv) = bosse.conn.open_bi().await.expect("Strom");
    let kopf = StromKopf {
        angebot,
        von: None,
        strom: 0,
    };
    let kopf_bytes = protokoll::schreibe(&kopf).expect("Kopf");
    bosse_send.write_all(&kopf_bytes).await.expect("Kopf");
    bosse_send.write_all(b"hallo welt").await.expect("Bytes");
    bosse_send.flush().await.expect("flush");

    // Ada nimmt den Strom an und sieht, von wem er kommt.
    let (mut ada_send, mut ada_recv) =
        tokio::time::timeout(Duration::from_secs(5), ada.conn.accept_bi())
            .await
            .expect("Zeitüberschreitung")
            .expect("Strom");
    let mut puffer = Vec::new();
    let gelesen = lese_kopf(&mut ada_recv, &mut puffer).await;
    assert_eq!(gelesen.angebot, angebot);
    assert_eq!(gelesen.von.as_deref(), Some(bosse_k.kennung().as_str()));
    assert!(gelesen.strom >= 1, "die Stromnummer vergibt der Server");
    let nutz = lese_bytes(&mut ada_recv, &mut puffer, 10).await;
    assert_eq!(nutz, b"hallo welt");

    // Antwort in die andere Richtung.
    ada_send.write_all(b"antwort!").await.expect("Antwort");
    ada_send.flush().await.expect("flush");
    let antwort = lese_bytes(&mut bosse_recv, &mut Vec::new(), 8).await;
    assert_eq!(antwort, b"antwort!");
}

// ------------------------------------------------------------------- UDP

#[tokio::test]
async fn ein_udp_datagramm_wird_weitergeleitet() {
    let server = Testserver::neu("udp", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let ada_sitzung = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse_sitzung = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");

    let (mut ada, _) = anmelden(server.adresse, &ada_k, &ada_sitzung.zertifikat).await;
    let (mut bosse, _) = anmelden(server.adresse, &bosse_k, &bosse_sitzung.zertifikat).await;

    ada.sende(ClientRahmen::Anbieten {
        name: "Sprachserver".into(),
        port: 9987,
        art: Art::Udp,
    })
    .await
    .expect("Anbieten");
    let angebot = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Angeboten { .. })).await {
        ServerRahmen::Angeboten { angebot } => angebot,
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Angebote { liste } if liste.iter().any(|a| a.id == angebot)),
    )
    .await;
    bosse
        .sende(ClientRahmen::Beitreten { angebot })
        .await
        .expect("Beitreten");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beigetreten { angebot: a } if *a == angebot),
    )
    .await;

    // Bosse schickt auf seinem Kanal 7.
    let datagramm = Datagrammkopf { angebot, kanal: 7 }.schreiben(b"hallo");
    bosse
        .conn
        .send_datagram(Bytes::from(datagramm))
        .expect("send_datagram");

    // Ada erfährt zuerst, welcher Kanal beim Gastgeber zu Bosse gehört …
    let kanal = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Kanal { .. })).await {
        ServerRahmen::Kanal {
            angebot: a,
            kanal,
            von,
        } => {
            assert_eq!(a, angebot);
            assert_eq!(von, bosse_k.kennung());
            kanal
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    assert_ne!(kanal, 7, "der Gastgeber-Kanal vergibt der Server");

    // … dann kommt das Datagramm mit genau diesem Kanal.
    let empfangen = tokio::time::timeout(Duration::from_secs(5), ada.conn.read_datagram())
        .await
        .expect("Zeitüberschreitung")
        .expect("Datagramm");
    let (kopf, nutzlast) = Datagrammkopf::lesen(&empfangen).expect("Kopf");
    assert_eq!(kopf.angebot, angebot);
    assert_eq!(kopf.kanal, kanal);
    assert_eq!(nutzlast, b"hallo");

    // Antwort auf demselben Kanal: Bosse bekommt sie auf seinem Kanal 7.
    let antwort = Datagrammkopf { angebot, kanal }.schreiben(b"antwort");
    ada.conn
        .send_datagram(Bytes::from(antwort))
        .expect("send_datagram");
    let empfangen = tokio::time::timeout(Duration::from_secs(5), bosse.conn.read_datagram())
        .await
        .expect("Zeitüberschreitung")
        .expect("Datagramm");
    let (kopf, nutzlast) = Datagrammkopf::lesen(&empfangen).expect("Kopf");
    assert_eq!(kopf.angebot, angebot);
    assert_eq!(kopf.kanal, 7, "zurück auf den eigenen Kanal");
    assert_eq!(nutzlast, b"antwort");
}

// ------------------------------------------------------------ Raumwechsel

#[tokio::test]
async fn ein_raumwechsel_beendet_die_freigabe() {
    let server = Testserver::neu("raumwechsel", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let mut ada_sitzung = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse_sitzung = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");

    let (mut ada, _) = anmelden(server.adresse, &ada_k, &ada_sitzung.zertifikat).await;
    let (mut bosse, _) = anmelden(server.adresse, &bosse_k, &bosse_sitzung.zertifikat).await;

    ada.sende(ClientRahmen::Anbieten {
        name: "Minecraft".into(),
        port: 25565,
        art: Art::Tcp,
    })
    .await
    .expect("Anbieten");
    let angebot = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Angeboten { .. })).await {
        ServerRahmen::Angeboten { angebot } => angebot,
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Angebote { liste } if liste.iter().any(|a| a.id == angebot)),
    )
    .await;
    bosse
        .sende(ClientRahmen::Beitreten { angebot })
        .await
        .expect("Beitreten");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beigetreten { angebot: a } if *a == angebot),
    )
    .await;

    // Ada legt einen zweiten Raum an und zieht um.
    ada_sitzung
        .sende(ClientFrame::RaumAnlegen {
            name: "Werkstatt".into(),
        })
        .await
        .expect("Raum anlegen");
    let raum = match warte_auf_sitzung(&mut ada_sitzung, |f| {
        matches!(f, ServerFrame::Raeume { raeume } if raeume.iter().any(|r| r.name == "Werkstatt"))
    })
    .await
    {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.name == "Werkstatt")
            .map(|r| r.id)
            .expect("neuer Raum"),
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    ada_sitzung
        .sende(ClientFrame::Betreten { raum })
        .await
        .expect("Betreten");

    // Bosse bekommt es binnen zwei Sekunden gesagt.
    let beendet = tokio::time::timeout(
        Duration::from_secs(2),
        warte_auf(&mut bosse, |r| matches!(r, ServerRahmen::Beendet { .. })),
    )
    .await
    .expect("Zeitüberschreitung: kein Beendet nach dem Raumwechsel");
    match beendet {
        ServerRahmen::Beendet { angebot: a, grund } => {
            assert_eq!(a, angebot);
            assert_eq!(grund, Grund::RaumVerlassen);
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }
}

// -------------------------------------------------------------- Sitzung weg

#[tokio::test]
async fn ohne_sitzung_endet_die_spielnetz_verbindung() {
    let server = Testserver::neu("sitzung-weg", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let ada_sitzung = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse_sitzung = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");

    let (mut ada, _) = anmelden(server.adresse, &ada_k, &ada_sitzung.zertifikat).await;
    let (mut bosse, _) = anmelden(server.adresse, &bosse_k, &bosse_sitzung.zertifikat).await;

    ada.sende(ClientRahmen::Anbieten {
        name: "Minecraft".into(),
        port: 25565,
        art: Art::Tcp,
    })
    .await
    .expect("Anbieten");
    let angebot = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Angeboten { .. })).await {
        ServerRahmen::Angeboten { angebot } => angebot,
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Angebote { liste } if liste.iter().any(|a| a.id == angebot)),
    )
    .await;
    bosse
        .sende(ClientRahmen::Beitreten { angebot })
        .await
        .expect("Beitreten");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beigetreten { angebot: a } if *a == angebot),
    )
    .await;

    // Ada trennt ihre **Sitzung** — nicht das Spielnetz.
    ada_sitzung.schliessen().await;

    // Bosse erfährt, dass der Gastgeber weg ist …
    let beendet = tokio::time::timeout(
        Duration::from_secs(5),
        warte_auf(&mut bosse, |r| matches!(r, ServerRahmen::Beendet { .. })),
    )
    .await
    .expect("Zeitüberschreitung: kein Beendet");
    match beendet {
        ServerRahmen::Beendet { angebot: a, grund } => {
            assert_eq!(a, angebot);
            assert_eq!(grund, Grund::GastgeberWeg);
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // … und Adas Spielnetz-Verbindung geht zu.
    tokio::time::timeout(Duration::from_secs(5), ada.conn.closed())
        .await
        .expect("die Spielnetz-Verbindung muss geschlossen werden");
}

// ------------------------------------------------------------ Rauswurf

/// Regression (Prüfung des Architekten, P2): endete einer von zwei Strömen
/// desselben Paares, trug er die Abbruchgriffe **beider** aus. Ein danach
/// entfernter Teilnehmer behielt seinen zweiten Strom.
#[tokio::test]
async fn entfernen_beendet_auch_die_uebrigen_stroeme() {
    let server = Testserver::neu("rauswurf", &[("SPRECHWERK_SPIELNETZ_AKTIV", "1")]);

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let ada_sitzung = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada");
    let bosse_sitzung = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
        .await
        .expect("Bosse");
    let (mut ada, _) = anmelden(server.adresse, &ada_k, &ada_sitzung.zertifikat).await;
    let (mut bosse, _) = anmelden(server.adresse, &bosse_k, &bosse_sitzung.zertifikat).await;

    ada.sende(ClientRahmen::Anbieten {
        name: "Minecraft".into(),
        port: 25565,
        art: Art::Tcp,
    })
    .await
    .expect("Anbieten");
    let angebot = match warte_auf(&mut ada, |r| matches!(r, ServerRahmen::Angeboten { .. })).await {
        ServerRahmen::Angeboten { angebot } => angebot,
        anderes => panic!("unerwartet: {anderes:?}"),
    };
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Angebote { liste } if liste.iter().any(|a| a.id == angebot)),
    )
    .await;
    bosse
        .sende(ClientRahmen::Beitreten { angebot })
        .await
        .expect("Beitreten");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beigetreten { angebot: a } if *a == angebot),
    )
    .await;

    // Zwei Ströme von Bosse, beide bei Ada angenommen.
    let kopf = protokoll::schreibe(&StromKopf {
        angebot,
        von: None,
        strom: 0,
    })
    .expect("Kopf");
    let mut stroeme = Vec::new();
    for _ in 0..2 {
        let (mut b_send, b_recv) = bosse.conn.open_bi().await.expect("Strom");
        b_send.write_all(&kopf).await.expect("Kopf");
        b_send.write_all(b"x").await.expect("Byte");
        b_send.flush().await.expect("flush");
        let (a_send, mut a_recv) =
            tokio::time::timeout(Duration::from_secs(5), ada.conn.accept_bi())
                .await
                .expect("Zeitüberschreitung")
                .expect("Strom");
        let mut puffer = Vec::new();
        lese_kopf(&mut a_recv, &mut puffer).await;
        lese_bytes(&mut a_recv, &mut puffer, 1).await;
        stroeme.push((b_send, b_recv, a_send, a_recv));
    }

    // Der erste Strom endet regulär.
    let (mut b1_send, _b1_recv, mut a1_send, _a1_recv) = stroeme.remove(0);
    b1_send.finish().expect("finish");
    a1_send.finish().expect("finish");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Ada wirft Bosse hinaus.
    ada.sende(ClientRahmen::Entfernen {
        angebot,
        kennung: bosse_k.kennung(),
    })
    .await
    .expect("Entfernen");
    warte_auf(
        &mut bosse,
        |r| matches!(r, ServerRahmen::Beendet { angebot: a, .. } if *a == angebot),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Der zweite Strom darf nichts mehr transportieren.
    let (_b2_send, mut b2_recv, mut a2_send, _a2_recv) = stroeme.remove(0);
    let _ = a2_send.write_all(b"noch da").await;
    let _ = a2_send.flush().await;
    let mut block = [0u8; 64];
    let gelesen = tokio::time::timeout(Duration::from_secs(2), b2_recv.read(&mut block)).await;
    if let Ok(Ok(Some(n))) = gelesen {
        assert_ne!(
            &block[..n],
            b"noch da",
            "ein entfernter Teilnehmer bekommt keine Bytes mehr"
        );
    }
}

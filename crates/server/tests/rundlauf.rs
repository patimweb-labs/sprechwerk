//! Rundlauf: echter Serverprozess, echter Client, echte Verbindung.
//!
//! Diese Prüfung ist der Ersatz für „ich klicke es mal an". Sie startet das
//! Programm so, wie Docker es startet (Konfiguration über Umgebung), verbindet
//! zwei Clients und prüft, was sie voneinander sehen.

use sprechwerk_adapters::serverschluessel::{Bund, Lesbar};
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::inhalt::Inhalt;
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
            // Keine Verwaltung (Port 7444, Browser) für Testserver.
            .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
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

    let (ada_k, bosse_k) = (Schluessel::neu(), Schluessel::neu());
    let mut ada = Verbindung::oeffnen(server.adresse, &ada_k, "Ada", None)
        .await
        .expect("Ada meldet sich an");
    let mut bosse = Verbindung::oeffnen(server.adresse, &bosse_k, "Bosse", None)
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

    // Ende zu Ende (ADR 0012): Ada legt den Serverschlüssel an, Bosse bekommt
    // ihn von Ada — der Server vermittelt nur.
    let mut ada = Teil::neu(ada, ada_k);
    let mut bosse = Teil::neu(bosse, bosse_k);
    ada.anfangen().await;
    bediene_beide(&mut ada, &mut bosse, |a, _| a.bund.kann_schreiben()).await;
    bosse.anfangen().await;
    bediene_beide(&mut ada, &mut bosse, |_, b| b.bund.kann_schreiben()).await;

    // Ada schreibt, Bosse liest — der Server sieht nur Geheimtext.
    let geheim = ada
        .bund
        .verschluesseln(raum, ada.v.ich, &Inhalt::text("Guten Abend"))
        .unwrap();
    ada.v
        .sende(ClientFrame::Text { text: geheim })
        .await
        .unwrap();
    let nachricht = warte_auf(&mut bosse.v, |f| matches!(f, ServerFrame::Text { .. })).await;
    match nachricht {
        ServerFrame::Text { nachricht } => {
            assert!(
                !nachricht.text.contains("Guten Abend"),
                "Klartext beim Server"
            );
            assert_eq!(nachricht.verfasser_name, "Ada");
            assert_eq!(
                bosse.bund.lesen(&nachricht),
                Lesbar::Inhalt(Inhalt::text("Guten Abend"))
            );
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Klartext nimmt der Server nicht an.
    ada.v
        .sende(ClientFrame::Text {
            text: "Klartext".into(),
        })
        .await
        .unwrap();
    warte_auf(&mut ada.v, |f| matches!(f, ServerFrame::Fehler { .. })).await;

    // Und in der Datenbank steht er nicht.
    tokio::time::sleep(Duration::from_millis(200)).await;
    for datei in std::fs::read_dir(&server.verzeichnis).unwrap().flatten() {
        if let Ok(bytes) = std::fs::read(datei.path()) {
            assert!(
                !bytes.windows(11).any(|w| w == b"Guten Abend"),
                "Klartext in {}",
                datei.path().display()
            );
        }
    }
}

/// Ein Client mit Serverschlüsselbund — beantwortet Schlüsselrahmen wie der
/// echte Client (`sprechwerk-cli`, Fenster).
struct Teil {
    v: Verbindung,
    k: Schluessel,
    bund: Bund,
}

impl Teil {
    fn neu(v: Verbindung, k: Schluessel) -> Self {
        let bund = Bund::laden(None, &v.zertifikat);
        Self { v, k, bund }
    }

    async fn anfangen(&mut self) {
        for f in self.bund.stand(self.v.serverschluessel.clone()) {
            self.v.sende(f).await.unwrap();
        }
    }

    async fn bediene(&mut self, f: &ServerFrame) {
        let antworten = match f {
            ServerFrame::Serverschluessel { stand } => self.bund.stand(stand.clone()),
            ServerFrame::ServerschluesselAnfrage { fuer, kennung, id } => {
                vec![self.bund.anfrage(*fuer, kennung, id, &self.k).unwrap()]
            }
            ServerFrame::ServerschluesselPaket {
                von_kennung,
                id,
                daten,
                unterschrift,
                ..
            } => vec![self
                .bund
                .paket(von_kennung, id, daten, unterschrift, &self.k)
                .unwrap()],
            _ => Vec::new(),
        };
        for a in antworten {
            self.v.sende(a).await.unwrap();
        }
    }
}

/// Bedient beide Clients, bis `fertig` gilt (höchstens 5 s).
async fn bediene_beide(a: &mut Teil, b: &mut Teil, fertig: impl Fn(&Teil, &Teil) -> bool) {
    let frist = tokio::time::Instant::now() + Duration::from_secs(5);
    while !fertig(a, b) {
        assert!(
            tokio::time::Instant::now() < frist,
            "Serverschlüssel kam nicht an"
        );
        tokio::select! {
            f = a.v.empfang() => { let f = f.unwrap().unwrap(); a.bediene(&f).await; }
            f = b.v.empfang() => { let f = f.unwrap().unwrap(); b.bediene(&f).await; }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
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
    // Gezielt auf die Liste mit dem neuen Raum warten: Raumlisten kommen auch
    // bei jedem Betreten.
    let liste = warte_auf(&mut ada, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.name == "Vorstand"))
    })
    .await;
    match liste {
        ServerFrame::Raeume { raeume } => {
            assert!(raeume.iter().any(|r| r.name == "Vorstand"), "{raeume:?}");
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Protokoll enthält die Ablehnung **und** die Erlaubnis.
    ada.sende(ClientFrame::Protokoll {
        ab: 0,
        anzahl: 100,
        von_hinten: false,
    })
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
        .sende(ClientFrame::Protokoll {
            ab: 0,
            anzahl: 10,
            von_hinten: false,
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
    let bosse = Verbindung::oeffnen(server.adresse, &bosse_schluessel, "Bosse", None)
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

/// Wer den Server mit Steuerrahmen zuschüttet, wird getrennt — mit einer
/// Erklärung, nicht mit Stille.
///
/// Die Grenze steht in der Konfiguration (`rahmen_je_sekunde`); hier wird sie
/// klein gesetzt, damit der Test nicht tausende Rahmen schicken muss.
#[tokio::test]
async fn zu_viele_rahmen_werden_abgewiesen() {
    let server = Testserver::starten("drossel", &[("SPRECHWERK_RAHMEN_JE_SEKUNDE", "5")]);
    server.bereit().await;

    let mut v = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Fluter", None)
        .await
        .unwrap();

    // 5 gehen durch, die nächsten werden abgewiesen (Fehler `spaeter`).
    let mut abgewiesen = false;
    for _ in 0..40 {
        if v.sende(ClientFrame::Ping).await.is_err() {
            // Verbindung ist schon zu — das ist das erwartete Ende.
            abgewiesen = true;
            break;
        }
        match tokio::time::timeout(Duration::from_millis(200), v.empfang()).await {
            Ok(Ok(Some(ServerFrame::Pong))) => continue,
            Ok(Ok(Some(ServerFrame::Fehler { code, .. }))) => {
                assert_eq!(
                    code,
                    sprechwerk_protocol::Fehlercode::Spaeter,
                    "die Absage muss „später nochmal“ sein, nicht etwas anderes"
                );
                abgewiesen = true;
                break;
            }
            Ok(Ok(Some(_))) => continue,
            Ok(Ok(None)) | Ok(Err(_)) => {
                abgewiesen = true;
                break;
            }
            Err(_) => continue,
        }
    }
    assert!(
        abgewiesen,
        "der Server hat 40 Rahmen in einer Sekunde hingenommen"
    );
}

/// Eine Sperre lässt sich auch wieder aufheben — mit Recht, und mit einer
/// ehrlichen Antwort, wenn keine Sperre passte.
#[tokio::test]
async fn sperre_wird_aufgehoben() {
    let server = Testserver::starten("entsperren", &[]);
    server.bereit().await;

    let mut ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .unwrap();
    let fremd = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bosse", None)
        .await
        .unwrap();
    let kennung = sprechwerk_adapters::Schluessel::neu().kennung();

    // Sperren.
    ada.sende(ClientFrame::Sperren {
        kennung: Some(kennung.clone()),
        adresse: None,
        grund: "Test".into(),
    })
    .await
    .unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Ok)).await;

    // Aufheben: bestätigt.
    ada.sende(ClientFrame::Entsperren {
        kennung: Some(kennung.clone()),
        adresse: None,
    })
    .await
    .unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Ok)).await;

    // Nochmal aufheben: passt zu keiner Sperre → Fehler, nicht „Ok".
    ada.sende(ClientFrame::Entsperren {
        kennung: Some(kennung.clone()),
        adresse: None,
    })
    .await
    .unwrap();
    let absage = warte_auf(&mut ada, |f| matches!(f, ServerFrame::Fehler { .. })).await;
    assert!(
        matches!(absage, ServerFrame::Fehler { .. }),
        "eine Sperre, die es nicht gibt, darf nicht als Erfolg gemeldet werden"
    );

    // Ein Mitglied (kein Administrator) darf nicht entsperren.
    ada.sende(ClientFrame::Sperren {
        kennung: Some(kennung.clone()),
        adresse: None,
        grund: "Test".into(),
    })
    .await
    .unwrap();
    warte_auf(&mut ada, |f| matches!(f, ServerFrame::Ok)).await;
    let mut bosse = fremd;
    bosse
        .sende(ClientFrame::Entsperren {
            kennung: Some(kennung),
            adresse: None,
        })
        .await
        .unwrap();
    let absage = warte_auf(&mut bosse, |f| matches!(f, ServerFrame::Fehler { .. })).await;
    match absage {
        ServerFrame::Fehler { code, .. } => assert_eq!(
            code,
            sprechwerk_protocol::Fehlercode::KeinRecht,
            "Entsperren ist eine Administratorhandlung"
        ),
        anderes => panic!("unerwartet: {anderes:?}"),
    }
}

/// Lebenszeichen und Antwort: die Umlaufzeit, die das Fenster anzeigt.
///
/// Sie wird im Client gemessen (Ping hinaus, Pong zurück). Der Test misst
/// denselben Weg gegen den echten Server und hält fest, dass er kurz bleibt —
/// sonst wäre die Zahl in der Statuszeile eine Behauptung.
#[tokio::test]
async fn ping_und_pong_gehen_schnell() {
    let server = Testserver::starten("ping", &[]);
    server.bereit().await;

    let mut v = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
        .await
        .unwrap();

    let mut messungen = Vec::new();
    for _ in 0..20 {
        let beginn = std::time::Instant::now();
        v.sende(ClientFrame::Ping).await.unwrap();
        warte_auf(&mut v, |f| matches!(f, ServerFrame::Pong)).await;
        messungen.push(beginn.elapsed().as_micros() as u64);
    }
    messungen.sort_unstable();
    let p50 = messungen[messungen.len() / 2];
    println!(
        "Umlaufzeit Ping/Pong: p50 {p50} µs, max {} µs",
        messungen[messungen.len() - 1]
    );
    assert!(
        p50 < 50_000,
        "die Umlaufzeit ist mit {p50} µs zu hoch für eine lokale Verbindung"
    );
}

/// **Reihenfolge am Draht:** die Antwort auf das eigene `Betreten` muss vor
/// den Meldungen über Späterkommende ankommen.
///
/// Der Client ersetzt mit der Antwort (`Betreten { anwesend }`) seine ganze
/// Mitgliederliste. Kam die Meldung „Clemens ist jetzt hier" **vor** dieser
/// Momentaufnahme an, war Clemens danach für immer weg — und war der Client
/// Schlüsselwart, bekam Clemens nie einen Raumschlüssel (gefunden 2026-11-14,
/// `schluesselwechsel_im_laufenden_gespraech` kippte unter Last). Ursache war
/// eine Lücke zwischen „Befehl ausführen" und „Antwort ins Postfach legen", in
/// der eine andere Verbindung die Sperre nehmen konnte.
///
/// Geprüft wird, was der Nutzer davon hat: nach einem gleichzeitigen Betreten
/// sieht **jeder** alle anderen im Raum. Mehrere Runden, weil ein Rennen nicht
/// bei jedem Lauf auftritt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gleichzeitiges_betreten_laesst_niemanden_verschwinden() {
    const LEUTE: usize = 6;
    const RUNDEN: usize = 12;

    let server = Testserver::starten("gleichzeitig", &[("SPRECHWERK_RAHMEN_JE_SEKUNDE", "1000")]);
    server.bereit().await;

    let mut alle = Vec::new();
    for i in 0..LEUTE {
        let v = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), &format!("P{i}"), None)
            .await
            .expect("anmelden");
        alle.push(v);
    }

    // Räume anlegen (die erste Verbindung ist Administrator).
    for r in 0..RUNDEN {
        alle[0]
            .sende(ClientFrame::RaumAnlegen {
                name: format!("Runde {r}"),
            })
            .await
            .unwrap();
    }
    let raeume = loop {
        warte_auf(&mut alle[0], |f| matches!(f, ServerFrame::Raeume { .. })).await;
        let ids: Vec<_> = alle[0]
            .raeume
            .iter()
            .filter(|r| r.name.starts_with("Runde "))
            .map(|r| r.id)
            .collect();
        if ids.len() == RUNDEN {
            break ids;
        }
    };

    let schranke = std::sync::Arc::new(tokio::sync::Barrier::new(LEUTE));
    let aufgaben: Vec<_> = alle
        .into_iter()
        .map(|mut v| {
            let schranke = schranke.clone();
            let raeume = raeume.clone();
            tokio::spawn(async move {
                // Fehler sammeln statt aussteigen: wer aussteigt, lässt die
                // anderen an der Schranke hängen.
                let mut fehler = Vec::new();
                'runde: for raum in raeume {
                    schranke.wait().await;
                    v.sende(ClientFrame::Betreten { raum }).await.unwrap();
                    let frist = tokio::time::Instant::now() + Duration::from_secs(3);
                    loop {
                        let da = v.mitglieder.iter().filter(|m| m.raum == Some(raum)).count();
                        if da == LEUTE {
                            break;
                        }
                        let rest = frist.saturating_duration_since(tokio::time::Instant::now());
                        if rest.is_zero() {
                            fehler
                                .push(format!("{} sieht in {raum:?} nur {da} von {LEUTE}", v.ich));
                            continue 'runde;
                        }
                        match tokio::time::timeout(rest, v.empfang()).await {
                            Ok(Ok(Some(_))) | Err(_) => {}
                            Ok(Ok(None)) => panic!("Verbindung geschlossen"),
                            Ok(Err(e)) => panic!("Verbindungsfehler: {e}"),
                        }
                    }
                }
                fehler
            })
        })
        .collect();

    let mut fehler = Vec::new();
    for a in aufgaben {
        fehler.extend(a.await.expect("Aufgabe"));
    }
    assert!(
        fehler.is_empty(),
        "verschwundene Mitglieder:\n{}",
        fehler.join("\n")
    );
}

/// Die Verwaltung auf der Konsole spricht den echten Prozess an (ADR 0013):
/// `sprechwerk stand` mit demselben Datenverzeichnis zeigt Name und „noch
/// niemand da"; eine falsche Rolle wird abgelehnt.
#[test]
fn verwaltung_auf_der_konsole() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
        .to_string();
    let server = Testserver::starten(
        "verwaltung",
        &[
            ("SPRECHWERK_VERWALTUNG_AKTIV", "1"),
            ("SPRECHWERK_VERWALTUNG_PORT", &port),
        ],
    );
    let befehl = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_sprechwerk"))
            .args(args)
            .env("SPRECHWERK_KONFIG", "/gibt/es/nicht.toml")
            .env("SPRECHWERK_DATEN", &server.verzeichnis)
            .env("SPRECHWERK_VERWALTUNG_PORT", &port)
            .output()
            .unwrap()
    };
    // Der Steuerkanal startet kurz nach „lauscht" — ein paar Versuche.
    let mut stand = befehl(&["stand"]);
    for _ in 0..50 {
        if stand.status.success() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        stand = befehl(&["stand"]);
    }
    let text = String::from_utf8_lossy(&stand.stdout);
    assert!(
        stand.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&stand.stderr)
    );
    assert!(text.contains("Rundlauftest"), "{text}");
    assert!(text.contains("Noch niemand da"), "{text}");

    let falsch = befehl(&["rolle", "niemand", "administrator"]);
    assert!(!falsch.status.success());
    assert!(String::from_utf8_lossy(&falsch.stderr).contains("niemand passt"));
}

/// `docker stop` schickt SIGTERM: der Server beendet sich sofort und sauber,
/// statt nach 10 s per SIGKILL ohne letztes Speichern zu sterben.
#[cfg(unix)]
#[test]
fn sigterm_beendet_sauber() {
    let mut server = Testserver::starten("sigterm", &[]);
    let pid = server.kind.id().to_string();
    let status = Command::new("kill").args(["-TERM", &pid]).status().unwrap();
    assert!(status.success());
    let start = std::time::Instant::now();
    loop {
        if let Some(ende) = server.kind.try_wait().unwrap() {
            assert!(ende.success(), "Rückgabe {ende:?}");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Server reagiert nicht auf SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Ende-zu-Ende-Verschlüsselung durch den echten Server.
//!
//! Geprüft wird die ganze Kette ohne Oberfläche und ohne Ton:
//!
//! 1. Ein Raum wird verschlüsselt (Recht, Protokoll, Rundruf).
//! 2. Der Wärter verteilt den Raumschlüssel — der Server leitet nur weiter.
//! 3. Ein Sprachrahmen geht **verschlüsselt** durch den Server; ein Mitleser
//!    am Server sieht keinen Klartext, der Empfänger liest ihn wieder.
//! 4. Ein fremder Client ohne Schlüssel bekommt kein Paket.

use ed25519_dalek::Signer;
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_domain::{MemberId, RoomId};
use sprechwerk_protocol::{
    ClientFrame, Mitgliedsansicht, ServerFrame, Sprachkopf, CODEC_OPUS, CODEC_VERSCHLUESSELT,
};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Ein laufender Server auf einem freien Port (wie in `rundlauf.rs`).
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

/// Startet den Server; der Rückgabewert muss leben, solange geprüft wird
/// (sein `Drop` beendet den Prozess).
async fn starte_server() -> (Testserver, SocketAddr) {
    // Parallele Tests können denselben „freien" Port erwischen; dann startet
    // der Server nicht und wir versuchen es mit einem anderen.
    for _ in 0..8 {
        match versuche_server().await {
            Some(ergebnis) => return ergebnis,
            None => continue,
        }
    }
    panic!("kein Server startbar");
}

async fn versuche_server() -> Option<(Testserver, SocketAddr)> {
    let verzeichnis = std::env::temp_dir().join(format!(
        "sprechwerk-ver-{}-{}",
        std::process::id(),
        freier_port()
    ));
    std::fs::create_dir_all(&verzeichnis).unwrap();
    let port = freier_port();
    let mut kind = Command::new(env!("CARGO_BIN_EXE_sprechwerk"))
        .env("SPRECHWERK_DATEN", &verzeichnis)
        .env("SPRECHWERK_PORT", port.to_string())
        .env("SPRECHWERK_BIND", "127.0.0.1")
        .env("SPRECHWERK_NAME", "Verschlüsselungstest")
        .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Server startet");

    // Auf „lauscht auf" warten (eine Testverbindung würde den Administrator
    // verbrauchen). Danach muss die Ausgabe **weiter gelesen** werden: sonst
    // schlägt der nächste `println!` im Server mit „Broken pipe" fehl und der
    // Prozess stirbt — genau das ist hier zuerst passiert.
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
    // Der Server muss den Port wirklich belegt haben.
    for _ in 0..50 {
        if std::net::UdpSocket::bind(("127.0.0.1", port)).is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Ist der Server noch am Leben? Wenn der Port belegt war, ist er sofort
    // ausgestiegen — dann neu versuchen.
    if let Ok(Some(_)) = kind.try_wait() {
        let _ = std::fs::remove_dir_all(&verzeichnis);
        return None;
    }

    let adresse: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    Some((Testserver { kind, verzeichnis }, adresse))
}

/// Verbindet mit kleinem Nachfassen: der Server ist manchmal erst wenige
/// Millisekunden nach seiner Meldung wirklich ansprechbar.
async fn verbinde(adresse: SocketAddr, schluessel: &Schluessel, name: &str) -> Verbindung {
    let mut letzter = String::new();
    for _ in 0..12 {
        match tokio::time::timeout(
            Duration::from_secs(3),
            Verbindung::oeffnen(adresse, schluessel, name, None),
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

/// Wartet auf den nächsten Rahmen, der zu `passt`, und liefert ihn.
async fn warte_auf(
    v: &mut Verbindung,
    passt: impl Fn(&ServerFrame) -> bool,
) -> ServerFrame {
    for _ in 0..60 {
        match tokio::time::timeout(Duration::from_millis(500), v.empfang()).await {
            Ok(Ok(Some(f))) if passt(&f) => return f,
            Ok(Ok(Some(_))) => continue,
            Ok(Ok(None)) => panic!("Verbindung wurde geschlossen"),
            Ok(Err(fehler)) => panic!("Fehler: {fehler}"),
            Err(_) => continue,
        }
    }
    panic!("kein passender Rahmen angekommen");
}

#[tokio::test]
async fn verschluesselte_sprache_geht_durch_den_server() {
    let (_server, adresse) = starte_server().await;

    let schluessel_a = Schluessel::neu();
    let schluessel_b = Schluessel::neu();
    let mut a = verbinde(adresse, &schluessel_a, "Ada").await;
    let mut b = verbinde(adresse, &schluessel_b, "Bosse").await;

    // A ist Administrator (erster) und legt einen Raum an.
    a.sende(ClientFrame::RaumAnlegen {
        name: "Geheim".into(),
    })
    .await
    .unwrap();
    let raeume = match warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume,
        _ => unreachable!(),
    };
    let raum: RoomId = raeume
        .iter()
        .find(|r| r.name == "Geheim")
        .expect("neuer Raum")
        .id;

    // Beide hinein.
    for v in [&mut a, &mut b] {
        v.sende(ClientFrame::Betreten { raum }).await.unwrap();
        warte_auf(v, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    }

    // A schaltet die Verschlüsselung ein.
    a.sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    // Gezielt auf die Meldung warten, die den Raum als verschlüsselt zeigt —
    // seit dem Betreten kommen Raumlisten auch ohne Verschlüsselung.
    let verschluesselt = match warte_auf(&mut a, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.verschluesselt))
    })
    .await
    {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.id == raum)
            .expect("Raum")
            .verschluesselt,
        _ => unreachable!(),
    };
    assert!(verschluesselt, "der Raum gilt jetzt als verschlüsselt");

    // Die Mitglieder kennen ihre Kennungen (für den Austausch).
    let mitglieder = a.mitglieder.clone();
    let b_kennung = mitglieder
        .iter()
        .find(|m| m.name == "Bosse")
        .map(|m| m.kennung.clone())
        .expect("Bosse hat eine Kennung");
    assert_eq!(
        b_kennung,
        schluessel_b.kennung(),
        "der Server gibt die öffentliche Kennung weiter"
    );

    // A verteilt den Raumschlüssel an B (wie es der Wärter tut).
    let raumschluessel = sprechwerk_adapters::verschluesselt::Raumschluessel::neu(1);
    let umschlag = sprechwerk_adapters::verschluesselt::Umschlag::verpacken(
        &schluessel_a.austausch_geheimnis(),
        &sprechwerk_adapters::verschluesselt::oeffentlich_aus_kennung(&b_kennung).unwrap(),
        raum.text().as_str(),
        &raumschluessel,
    )
    .unwrap();
    let text = sprechwerk_adapters::verschluesselt::zu_signieren(
        raum.text().as_str(),
        1,
        &umschlag.bytes,
    );
    let unterschrift = schluessel_a.unterschreibe(&text);

    a.sende(ClientFrame::Schluesselpaket {
        raum,
        epoche: 1,
        fuer: b.ich,
        daten: hex::encode(&umschlag.bytes),
        unterschrift: unterschrift.clone(),
    })
    .await
    .unwrap();

    let paket = warte_auf(&mut b, |f| {
        matches!(f, ServerFrame::Schluesselpaket { .. })
    })
    .await;
    let ServerFrame::Schluesselpaket {
        raum: paket_raum,
        epoche,
        von,
        daten,
        unterschrift: paket_unterschrift,
    } = paket
    else {
        unreachable!()
    };
    assert_eq!(paket_raum, raum);
    assert_eq!(epoche, 1);
    assert_eq!(von, a.ich);
    // Der Server hat nichts verändert.
    assert_eq!(paket_unterschrift, unterschrift);

    // Unterschrift prüfen (das macht sonst der Client) …
    assert!(sprechwerk_adapters::pruefe_beweis(
        &schluessel_a.kennung(),
        &sprechwerk_adapters::verschluesselt::zu_signieren(
            raum.text().as_str(),
            1,
            &hex::decode(&daten).unwrap()
        ),
        &paket_unterschrift
    ));
    // … und öffnen.
    let geoeffnet = sprechwerk_adapters::verschluesselt::Umschlag {
        bytes: hex::decode(&daten).unwrap(),
    }
    .oeffnen(
        &schluessel_b.austausch_geheimnis(),
        &sprechwerk_adapters::verschluesselt::oeffentlich_aus_kennung(&schluessel_a.kennung())
            .unwrap(),
        raum.text().as_str(),
        1,
    )
    .expect("B öffnet den Umschlag");
    assert_eq!(geoeffnet.bytes(), raumschluessel.bytes());

    // Verschlüsselter Sprachrahmen durch den Server.
    let klar = Sprachkopf::neu(1).schreiben(b"Opus-Sprachdaten");
    let geheim = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
        &klar,
        &raumschluessel,
        raum.text().as_str(),
    )
    .unwrap();
    // Kein Klartext im Geheimtext (das ist die Zusage).
    assert!(!geheim.windows(5).any(|w| w == b"Sprach"));
    a.sprache_senden(bytes::Bytes::from(geheim.clone())).unwrap();

    let empfangen = tokio::time::timeout(Duration::from_secs(3), b.sprache_empfangen())
        .await
        .expect("B bekommt den Rahmen")
        .expect("kein Fehler");

    // Der Kopf bleibt lesbar (der Server braucht ihn zum Verteilen).
    let (kopf, _) = Sprachkopf::lesen(&empfangen).expect("Kopf");
    assert_eq!(kopf.codec, CODEC_VERSCHLUESSELT);
    assert_eq!(kopf.folge, 1);

    let entschluesselt = sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(
        &empfangen,
        &raumschluessel,
        raum.text().as_str(),
    )
    .unwrap();
    let (kopf_klar, nutzlast) = Sprachkopf::lesen(&entschluesselt).expect("Kopf");
    assert_eq!(kopf_klar.codec, CODEC_OPUS);
    assert_eq!(nutzlast, b"Opus-Sprachdaten");

    a.schliessen();
    b.schliessen();
}

#[tokio::test]
async fn schluesselpaket_verlaesst_den_raum_nicht() {
    let (_server, adresse) = starte_server().await;

    let mut a = verbinde(adresse, &Schluessel::neu(), "Ada").await;
    let mut fremd = verbinde(adresse, &Schluessel::neu(), "Fremd").await;

    // A legt einen Raum an, verschlüsselt ihn und bleibt drin.
    a.sende(ClientFrame::RaumAnlegen { name: "Geheim".into() })
        .await
        .unwrap();
    let raum = match warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume.iter().find(|r| r.name == "Geheim").unwrap().id,
        _ => unreachable!(),
    };
    a.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut a, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    a.sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await;

    // Fremd bleibt im Empfang. Ein Paket an ihn darf nicht ankommen.
    a.sende(ClientFrame::Schluesselpaket {
        raum,
        epoche: 1,
        fuer: fremd.ich,
        daten: "00".into(),
        unterschrift: "00".into(),
    })
    .await
    .unwrap();

    let antwort = warte_auf(&mut a, |f| {
        matches!(f, ServerFrame::Ok | ServerFrame::Fehler { .. })
    })
    .await;
    assert!(
        matches!(antwort, ServerFrame::Fehler { .. }),
        "der Server muss Pakete in fremde Räume ablehnen, war: {antwort:?}"
    );

    let beim_fremden = tokio::time::timeout(Duration::from_millis(400), fremd.empfang()).await;
    match beim_fremden {
        Ok(Ok(Some(ServerFrame::Schluesselpaket { .. }))) => {
            panic!("Paket hat den Raum verlassen")
        }
        _ => {}
    }

    a.schliessen();
    fremd.schliessen();
}

#[tokio::test]
async fn ohne_recht_kein_verschluesseln() {
    let (_server, adresse) = starte_server().await;

    let mut admin = verbinde(adresse, &Schluessel::neu(), "Ada").await;
    let mut mitglied = verbinde(adresse, &Schluessel::neu(), "Bosse").await;

    let raum: RoomId = match &mitglied.raeume.first() {
        Some(r) => r.id,
        None => {
            // Beim ersten Verbinden mitgeschickt; sonst aus der Begrüßung.
            warte_auf(&mut mitglied, |f| matches!(f, ServerFrame::Raeume { .. })).await;
            mitglied.raeume.first().unwrap().id
        }
    };
    let _ = MemberId::aus_roh(0);

    mitglied
        .sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    let antwort = warte_auf(&mut mitglied, |f| {
        matches!(f, ServerFrame::Fehler { .. } | ServerFrame::Raeume { .. })
    })
    .await;
    assert!(
        matches!(antwort, ServerFrame::Fehler { .. }),
        "ein Mitglied darf nicht verschlüsseln, war: {antwort:?}"
    );

    // Der Administrator darf.
    admin
        .sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    let raeume = match warte_auf(&mut admin, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume,
        _ => unreachable!(),
    };
    assert!(raeume.iter().find(|r| r.id == raum).unwrap().verschluesselt);

    admin.schliessen();
    mitglied.schliessen();
}

#[test]
fn kennung_ist_gueltiges_ed25519() {
    // Kleine Gegenprobe ohne Server: die Kennung ist ein echter öffentlicher
    // Schlüssel, und die Unterschrift darauf prüft.
    let s = Schluessel::neu();
    let kennung = s.kennung();
    let be = kennung.clone();
    let signatur = s.unterschreibe("hallo");
    assert!(sprechwerk_adapters::pruefe_beweis(&be, "hallo", &signatur));
    // Und der Austauschschlüssel passt dazu.
    assert!(sprechwerk_adapters::verschluesselt::oeffentlich_aus_kennung(&kennung).is_some());
    let _ = Mitgliedsansicht {
        id: MemberId::aus_roh(1),
        name: "x".into(),
        kennung,
        rolle: sprechwerk_domain::Role::Mitglied,
        raum: None,
        spricht: false,
        stumm: false,
        taub: false,
    };
    let _ = Signer::sign(&ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]), b"x");
}

/// Wer einen verschlüsselten Raum verlässt, nimmt den Schlüssel nicht mit —
/// und die Verbliebenen **erfahren** davon, sonst tauschen sie nichts aus.
///
/// Geprüft über echte Verbindungen: nach dem Verlassen muss die Raumliste mit
/// einer höheren Epoche ankommen (`verschluesselt_seit`), aus der die Clients
/// ihren Schlüssel bilden.
#[tokio::test]
async fn epoche_dreht_sich_wenn_jemand_geht() {
    let (_server, adresse) = starte_server().await;

    let mut a = verbinde(adresse, &Schluessel::neu(), "Ada").await;
    let mut b = verbinde(adresse, &Schluessel::neu(), "Bosse").await;

    a.sende(ClientFrame::RaumAnlegen {
        name: "Geheim".into(),
    })
    .await
    .unwrap();
    let raum = match warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.name == "Geheim")
            .expect("neuer Raum")
            .id,
        _ => unreachable!(),
    };
    for v in [&mut a, &mut b] {
        v.sende(ClientFrame::Betreten { raum }).await.unwrap();
        warte_auf(v, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    }

    // A schaltet die Verschlüsselung ein; B bekommt die Raumliste.
    a.sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    let seit = match warte_auf(&mut b, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.verschluesselt))
    })
    .await
    {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.id == raum)
            .expect("Raum")
            .verschluesselt_seit,
        _ => unreachable!(),
    };
    assert!(seit > 0, "verschlüsselter Raum ohne Zeitstempel");

    // B geht. A muss die neue Epoche erfahren — sonst behielte B den Schlüssel.
    b.sende(ClientFrame::Verlassen).await.unwrap();
    // Nur eine **neuere** Epoche zählt: die Meldungen vom Einschalten liegen
    // noch in der Warteschlange.
    let nachher = match warte_auf(&mut a, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.verschluesselt_seit > seit))
    })
    .await
    {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.id == raum)
            .expect("Raum")
            .verschluesselt_seit,
        _ => unreachable!(),
    };
    assert!(
        nachher > seit,
        "Epoche blieb gleich ({seit} -> {nachher}): der Schlüssel bliebe gültig und \
         niemand würde einen neuen aushandeln"
    );
}

/// Verbindungsabbruch mitten im Gespräch: keine Verabschiedung, kein
/// `Verlassen` — die Leitung ist einfach weg (WLAN, Akku, Zug).
///
/// Der Server muss das wie ein Gehen behandeln: Anwesenheit weg, Epoche
/// gedreht, Zähler stimmt. Und die Person muss mit **derselben Kennung**
/// zurückkommen können, ohne Geistermitglied zu bleiben.
#[tokio::test]
async fn abbruch_mitten_im_gespraech_raeumt_auf_und_dreht_die_epoche() {
    let (_server, adresse) = starte_server().await;

    let schluessel_b = Schluessel::neu();
    let mut a = verbinde(adresse, &Schluessel::neu(), "Ada").await;
    let mut b = verbinde(adresse, &schluessel_b, "Bosse").await;
    let b_id = b.ich;

    a.sende(ClientFrame::RaumAnlegen {
        name: "Geheim".into(),
    })
    .await
    .unwrap();
    let raum = match warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.name == "Geheim")
            .expect("neuer Raum")
            .id,
        _ => unreachable!(),
    };
    for v in [&mut a, &mut b] {
        v.sende(ClientFrame::Betreten { raum }).await.unwrap();
        warte_auf(v, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    }

    a.sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    let seit = match warte_auf(&mut b, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.verschluesselt))
    })
    .await
    {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.id == raum)
            .expect("Raum")
            .verschluesselt_seit,
        _ => unreachable!(),
    };

    // **Abbruch.** `schliessen` schickt kein `Verlassen`, sondern beendet nur
    // die QUIC-Verbindung — für den Server dasselbe wie ein Stromausfall.
    b.schliessen();
    drop(b);

    let mut abwesend = false;
    let mut neue_epoche = false;
    let mut anwesend_richtig = false;
    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_millis(500), a.empfang()).await {
            Ok(Ok(Some(ServerFrame::Anwesenheit {
                mitglied, anwesend, ..
            }))) if mitglied.id == b_id && !anwesend => abwesend = true,
            Ok(Ok(Some(ServerFrame::Raeume { raeume }))) => {
                if let Some(r) = raeume.iter().find(|r| r.id == raum) {
                    if r.verschluesselt_seit > seit {
                        neue_epoche = true;
                        // Niemand soll als „anwesend" gezählt bleiben.
                        anwesend_richtig = r.anwesend == 1;
                    }
                }
            }
            Ok(Ok(Some(_))) => continue,
            Ok(Ok(None)) => panic!("Verbindung von Ada wurde geschlossen"),
            Ok(Err(fehler)) => panic!("Fehler bei Ada: {fehler}"),
            Err(_) => continue,
        }
        if abwesend && neue_epoche && anwesend_richtig {
            break;
        }
    }
    assert!(abwesend, "der Abbruch wurde den anderen nicht gemeldet");
    assert!(
        neue_epoche,
        "Epoche blieb bei {seit}: der Schlüssel des Abgebrochenen bliebe gültig"
    );
    assert!(
        anwesend_richtig,
        "Raum zählt weiter ein anwesendes Mitglied, das gar nicht mehr da ist"
    );

    // Zurück mit derselben Kennung: Rolle bleibt, kein doppelter Eintrag.
    let mut wieder = verbinde(adresse, &schluessel_b, "Bosse").await;
    assert_eq!(wieder.ich, b_id, "dieselbe Kennung ergibt ein neues Mitglied");
    assert_eq!(
        wieder.rolle,
        sprechwerk_domain::Role::Mitglied,
        "die Rolle ging beim Abbruch verloren"
    );
    wieder.sende(ClientFrame::Betreten { raum }).await.unwrap();
    warte_auf(&mut wieder, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    let liste = warte_auf(&mut a, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.anwesend == 2))
    })
    .await;
    assert!(
        matches!(liste, ServerFrame::Raeume { .. }),
        "nach dem Wiederkommen stimmt die Zahl der Anwesenden nicht"
    );
}

/// Die Nachforderung des Raumschlüssels: der Server leitet sie weiter, ohne
/// den Schlüssel zu kennen — und nur innerhalb eines verschlüsselten Raums.
#[tokio::test]
async fn schluesselanfrage_wird_an_den_raum_weitergeleitet() {
    let (_server, adresse) = starte_server().await;

    let mut a = verbinde(adresse, &Schluessel::neu(), "Ada").await;
    let mut b = verbinde(adresse, &Schluessel::neu(), "Bosse").await;
    let mut fremd = verbinde(adresse, &Schluessel::neu(), "Fremder").await;

    a.sende(ClientFrame::RaumAnlegen {
        name: "Geheim".into(),
    })
    .await
    .unwrap();
    let raum = match warte_auf(&mut a, |f| matches!(f, ServerFrame::Raeume { .. })).await {
        ServerFrame::Raeume { raeume } => raeume
            .iter()
            .find(|r| r.name == "Geheim")
            .expect("Raum")
            .id,
        _ => unreachable!(),
    };
    for v in [&mut a, &mut b] {
        v.sende(ClientFrame::Betreten { raum }).await.unwrap();
        warte_auf(v, |f| matches!(f, ServerFrame::Betreten { .. })).await;
    }

    // Ohne Verschlüsselung: die Bitte ist sinnlos, also abgelehnt.
    b.sende(ClientFrame::SchluesselAnfrage { raum }).await.unwrap();
    let absage = warte_auf(&mut b, |f| matches!(f, ServerFrame::Fehler { .. })).await;
    assert!(
        matches!(absage, ServerFrame::Fehler { .. }),
        "Anfrage in einem offenen Raum muss abgelehnt werden"
    );

    // Mit Verschlüsselung: A (im Raum) bekommt sie, der Fremde nicht.
    a.sende(ClientFrame::RaumVerschluesseln { raum, an: true })
        .await
        .unwrap();
    warte_auf(&mut b, |f| {
        matches!(f, ServerFrame::Raeume { raeume }
            if raeume.iter().any(|r| r.id == raum && r.verschluesselt))
    })
    .await;

    b.sende(ClientFrame::SchluesselAnfrage { raum }).await.unwrap();
    let anfrage = warte_auf(&mut a, |f| matches!(f, ServerFrame::SchluesselAnfrage { .. })).await;
    match anfrage {
        ServerFrame::SchluesselAnfrage { raum: r, von } => {
            assert_eq!(r, raum);
            assert_eq!(von, b.ich, "die Anfrage muss den Fragenden nennen");
        }
        anderes => panic!("unerwartet: {anderes:?}"),
    }

    // Der Frager bekommt eine Bestätigung, aber keine eigene Anfrage zurück.
    warte_auf(&mut b, |f| matches!(f, ServerFrame::Ok)).await;

    // Der Fremde (anderer Raum) bekommt nichts — er hat keinen Zugang.
    match tokio::time::timeout(Duration::from_millis(300), fremd.empfang()).await {
        // Zeitüberschreitung heißt: es kam nichts. Genau das erwarten wir.
        Err(_) => {}
        Ok(Ok(Some(ServerFrame::SchluesselAnfrage { .. }))) => {
            panic!("die Anfrage ging an jemanden außerhalb des Raums")
        }
        Ok(Ok(Some(_))) => {}
        Ok(Ok(None)) => panic!("Verbindung des Fremden wurde geschlossen"),
        Ok(Err(fehler)) => panic!("Fehler beim Fremden: {fehler}"),
    }
}

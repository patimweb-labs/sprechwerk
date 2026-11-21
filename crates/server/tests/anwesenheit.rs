//! Anwesenheit, Neustart, Passwort — mit echtem Serverprozess.
//!
//! **Gefunden 2026-11-19 (Simulation):** nach einem Neustart meldete der Raum
//! Leute als anwesend, die längst weg waren, und ein Neuankömmling bekam
//! „kein Serverschlüssel: es ist kein Mitglied online", obwohl der Raum
//! „2 anwesend" behauptete. Außerdem stand bei falschem Serverpasswort nur
//! „closed by peer: 0", und `sprechwerk --help` startete den Server.

use sprechwerk_adapters::{Schluessel, Verbindung};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Ein laufender Server auf einem freien Port.
struct Testserver {
    kind: Child,
    adresse: SocketAddr,
    verzeichnis: PathBuf,
    /// Beim Verlassen aufräumen? Beim Neustart-Test bleibt das Verzeichnis
    /// stehen — es ist der Zustand, den der zweite Server laden soll.
    aufraeumen: bool,
}

impl Drop for Testserver {
    fn drop(&mut self) {
        let _ = self.kind.kill();
        let _ = self.kind.wait();
        if self.aufraeumen {
            let _ = std::fs::remove_dir_all(&self.verzeichnis);
        }
    }
}

fn freier_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn neues_verzeichnis(name: &str) -> PathBuf {
    let pfad = std::env::temp_dir().join(format!(
        "sprechwerk-anwesend-{}-{}-{}",
        std::process::id(),
        name,
        freier_port()
    ));
    std::fs::create_dir_all(&pfad).unwrap();
    pfad
}

impl Testserver {
    fn neu(name: &str, extra: &[(&str, &str)]) -> Self {
        let verzeichnis = neues_verzeichnis(name);
        Self::in_verzeichnis(verzeichnis, true, extra)
    }

    fn in_verzeichnis(verzeichnis: PathBuf, aufraeumen: bool, extra: &[(&str, &str)]) -> Self {
        let port = freier_port();
        let mut befehl = Command::new(env!("CARGO_BIN_EXE_sprechwerk"));
        befehl
            .env("SPRECHWERK_DATEN", &verzeichnis)
            .env("SPRECHWERK_PORT", port.to_string())
            .env("SPRECHWERK_BIND", "127.0.0.1")
            .env("SPRECHWERK_NAME", "Anwesenheitstest")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (schluessel, wert) in extra {
            befehl.env(schluessel, wert);
        }
        let mut kind = befehl.spawn().expect("Server startet");

        // Auf „lauscht" warten — keine Sonde: der erste Ankömmling wird
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
        assert!(bereit, "Server meldet sich nicht:\n{gesamt}");
        // Restliche Ausgabe im Hintergrund verwerfen (volle Pfeife würde den
        // Server anhalten).
        std::thread::spawn(move || {
            let mut rest = String::new();
            let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
        });

        Self {
            kind,
            adresse: format!("127.0.0.1:{port}").parse().unwrap(),
            verzeichnis,
            aufraeumen,
        }
    }

    /// Wie viele gerade in diesem Raum sind — aus der Sicht des Clients.
    fn anwesend_im_raum(verbindung: &Verbindung, raum: sprechwerk_domain::RoomId) -> usize {
        verbindung
            .raeume
            .iter()
            .find(|r| r.id == raum)
            .map(|r| r.anwesend)
            .unwrap_or(0)
    }
}

#[tokio::test]
async fn neustart_zeigt_niemanden_als_anwesend() {
    let verzeichnis = neues_verzeichnis("neustart");

    // 1. Lauf: Ada meldet sich an und sitzt im Empfang. Dann stirbt der
    //    Server ohne Abschied (Absturz, Stromausfall) — genau der Fall, in dem
    //    die Anwesenheit früher mitgespeichert wurde.
    let raum;
    {
        let server = Testserver::in_verzeichnis(verzeichnis.clone(), false, &[]);
        let ada = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None)
            .await
            .expect("Ada meldet sich an");
        raum = ada
            .raum
            .expect("Ada sitzt nach der Anmeldung in einem Raum");
        assert_eq!(Testserver::anwesend_im_raum(&ada, raum), 1);
        drop(server);
    }
    // Der Absturz darf die Datenbank nicht beschädigen (Write-Through).
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 2. Lauf: derselbe Datenbestand. Bo ist der Einzige, der da ist.
    let server = Testserver::in_verzeichnis(verzeichnis, true, &[]);
    let bo = Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Bo", None)
        .await
        .expect("Bo meldet sich an");
    let empfang = bo.raum.expect("Bo sitzt im Empfang");
    assert_eq!(
        Testserver::anwesend_im_raum(&bo, raum),
        1,
        "nach dem Neustart ist nur Bo anwesend — Ada ist weg, nicht im Raum"
    );
    assert_eq!(empfang, raum);
    // Ada bleibt Mitglied (Rolle hängt an der Kennung) — nur nicht anwesend.
    assert!(
        bo.mitglieder.iter().any(|m| m.name == "Ada"),
        "die Rolle von Ada bleibt erhalten: {:?}",
        bo.mitglieder.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
}

/// Ohne Passwort, mit falschem Passwort: der Client bekommt einen Grund, nicht
/// „closed by peer: 0".
#[tokio::test]
async fn falsches_passwort_wird_mit_grund_abgelehnt() {
    let server = Testserver::neu("passwort", &[("SPRECHWERK_PASSWORT", "vereinsgeheim")]);

    let grund = |ergebnis: anyhow::Result<Verbindung>| match ergebnis {
        Ok(_) => panic!("die Anmeldung hätte abgelehnt werden müssen"),
        Err(fehler) => fehler.to_string(),
    };

    let ohne = grund(Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", None).await);
    assert!(
        ohne.contains("Passwort nötig"),
        "ohne Passwort muss „Passwort nötig\" kommen, war: {ohne}"
    );

    let falsch = grund(
        Verbindung::oeffnen(server.adresse, &Schluessel::neu(), "Ada", Some("daneben")).await,
    );
    assert!(
        falsch.contains("Passwort falsch"),
        "bei falschem Passwort muss „Passwort falsch\" kommen, war: {falsch}"
    );

    // Mit dem richtigen Passwort geht es — der Grund ist also das Passwort und
    // nicht die Verbindung.
    Verbindung::oeffnen(
        server.adresse,
        &Schluessel::neu(),
        "Ada",
        Some("vereinsgeheim"),
    )
    .await
    .expect("mit dem richtigen Passwort klappt die Anmeldung");
}

/// Führt `sprechwerk` mit einer Frist aus.
///
/// **Die Frist ist der Prüfpunkt:** ein startender Server endet nie — ohne sie
/// hinge der Prüflauf, statt zu melden, dass `--help` den Server gestartet hat.
fn lauf_mit_frist(
    args: &[&str],
    verzeichnis: &std::path::Path,
    daten: &std::path::Path,
) -> std::process::Output {
    let mut kind = Command::new(env!("CARGO_BIN_EXE_sprechwerk"))
        .args(args)
        .current_dir(verzeichnis)
        .env("SPRECHWERK_DATEN", daten)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sprechwerk startet");
    let frist = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if kind.try_wait().expect("Prozesszustand").is_some() {
            return kind.wait_with_output().expect("Ausgabe");
        }
        if std::time::Instant::now() > frist {
            let _ = kind.kill();
            let _ = kind.wait();
            panic!("sprechwerk {args:?} endet nicht — der Server wurde gestartet");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `sprechwerk --help` zeigt die Übersicht und startet nichts; eine unbekannte
/// Angabe endet mit Fehler und Übersicht.
#[test]
fn hilfe_startet_keinen_server() {
    let verzeichnis = neues_verzeichnis("hilfe");
    let daten = verzeichnis.join("daten");

    let ausgabe = lauf_mit_frist(&["--help"], &verzeichnis, &daten);
    let text = String::from_utf8_lossy(&ausgabe.stdout).to_string();
    assert!(ausgabe.status.success(), "--help muss gelingen: {text}");
    assert!(text.contains("sprechwerk — Server"), "war: {text}");
    assert!(
        !daten.exists(),
        "--help darf kein Datenverzeichnis anlegen ({} existiert)",
        daten.display()
    );

    let ausgabe = lauf_mit_frist(&["--quatsch"], &verzeichnis, &daten);
    let fehler = String::from_utf8_lossy(&ausgabe.stderr).to_string();
    assert!(
        !ausgabe.status.success(),
        "eine unbekannte Angabe muss mit Fehler enden, nicht starten"
    );
    assert!(
        fehler.contains("unbekannte Angabe: --quatsch"),
        "war: {fehler}"
    );
    assert!(
        fehler.contains("sprechwerk — Server"),
        "die Übersicht fehlt: {fehler}"
    );
    assert!(!daten.exists(), "auch das darf nichts anlegen");

    let _ = std::fs::remove_dir_all(&verzeichnis);
}

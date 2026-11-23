//! Referenzclient: Verbindung, Anmeldung, Rahmen.
//!
//! Dieser Baustein ist die Grundlage für alles, was später spricht: die
//! Kommandozeilen-Anwendung (`sprechwerk-cli`) und die Prüfläufe benutzen ihn
//! genauso wie später die Fensteranwendung.
//!
//! **Festlegung (SPEC §4):** Die Kennung ist ein Ed25519-Schlüsselpaar. Der
//! Server fordert eine Zufallszahl an und prüft die Signatur — damit ist die
//! Kennung ein Besitznachweis. Wer die Schlüsseldatei kopiert, ist derselbe
//! Mensch; das ist die bewusst gewählte Schwäche („Kennung ist mitnehmbar").

pub mod anhang;

/// Eine QUIC-Verbindung — für Datenströme ([`anhang`]), ohne dass ein
/// Aufrufer `quinn` selbst kennen muss.
pub type Leitung = quinn::Connection;
pub mod echo;
pub mod klang;
pub mod serverschluessel;
pub mod sprechpfad;
pub mod verschluesselt;

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use quinn::crypto::rustls::QuicClientConfig;
use sprechwerk_protocol::{ClientFrame, Raumansicht, ServerFrame, PROTOKOLL_VERSION};
use std::net::SocketAddr;
use std::sync::Arc;

/// Schreibt eine Datei, die nur ihr Eigentümer lesen darf (Schlüssel, gemerkte
/// Passwörter).
///
/// **Die Rechte gelten ab dem ersten Byte.** Erst schreiben und dann
/// `chmod 600` ließe ein Fenster, in dem die Datei für alle lesbar ist — und
/// ein still verschluckter Fehler beim `chmod` ließe sie dauerhaft so. Hier wird
/// die Datei mit 0600 *angelegt*, und eine vorhandene wird vor dem Schreiben auf
/// 0600 gebracht; jeder Fehler kommt beim Aufrufer an. Unter Windows schützt
/// das Benutzerprofil (Zugriffsliste des Verzeichnisses), dort ist nichts zu
/// tun.
pub fn geheim_schreiben(pfad: &std::path::Path, inhalt: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(eltern) = pfad.parent() {
        std::fs::create_dir_all(eltern)?;
    }
    let mut optionen = std::fs::OpenOptions::new();
    optionen.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        optionen.mode(0o600);
    }
    let mut datei = optionen.open(pfad)?;
    #[cfg(unix)]
    {
        // `mode` wirkt nur beim Anlegen; eine alte Datei kann weiter offen sein.
        use std::os::unix::fs::PermissionsExt;
        datei.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    datei.write_all(inhalt)?;
    datei.sync_all()
}

/// Ein Schlüsselpaar in Textform.
#[derive(Clone)]
pub struct Schluessel {
    geheim: [u8; 32],
}

impl Schluessel {
    /// Erzeugt einen neuen Schlüssel aus dem Betriebssystem-Zufall.
    pub fn neu() -> Self {
        let mut geheim = [0u8; 32];
        getrandom::getrandom(&mut geheim).expect("Zufall");
        Self { geheim }
    }

    /// Lädt einen Schlüssel aus einer Datei oder erzeugt und speichert ihn.
    pub fn laden_oder_erzeugen(pfad: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let pfad = pfad.as_ref();
        if let Ok(text) = std::fs::read_to_string(pfad) {
            let roh = hex::decode(text.trim()).context("Schlüsseldatei ist beschädigt")?;
            let geheim: [u8; 32] = roh
                .try_into()
                .map_err(|_| anyhow::anyhow!("Schlüsseldatei hat die falsche Länge"))?;
            return Ok(Self { geheim });
        }
        let s = Self::neu();
        geheim_schreiben(pfad, hex::encode(s.geheim).as_bytes())
            .with_context(|| format!("Schlüsseldatei {}", pfad.display()))?;
        Ok(s)
    }

    pub fn aus_geheim(geheim: [u8; 32]) -> Self {
        Self { geheim }
    }

    fn signierer(&self) -> SigningKey {
        SigningKey::from_bytes(&self.geheim)
    }

    /// Öffentlicher Schlüssel als Hex — das ist die Kennung gegenüber dem Server.
    pub fn kennung(&self) -> String {
        hex::encode(self.signierer().verifying_key().to_bytes())
    }

    /// Signiert eine Zeichenkette (den Nonce des Servers).
    pub fn unterschreibe(&self, text: &str) -> String {
        hex::encode(self.signierer().sign(text.as_bytes()).to_bytes())
    }

    /// Der X25519-Anteil desselben Schlüssels (für den Schlüsselaustausch).
    ///
    /// Abgeleitet, nicht gespeichert: die Schlüsseldatei bleibt bei 32 Byte,
    /// und es gibt keinen zweiten Schlüssel, den man beim Umzug vergisst.
    pub fn austausch_geheimnis(&self) -> x25519_dalek::StaticSecret {
        verschluesselt::geheimnis_aus_anmeldeschluessel(&self.geheim)
    }

    /// Export als Text (zum Mitnehmen auf ein neues Gerät).
    pub fn export(&self) -> String {
        hex::encode(self.geheim)
    }
}

/// Prüft das Server-Zertifikat gegen den angehefteten Fingerabdruck.
///
/// **Festlegung (SPEC §7, ADR 0002 §3):** Der Server hat ein selbst signiertes
/// Zertifikat. Der Client nimmt es beim **ersten** Kontakt an und heftet den
/// Fingerabdruck an („trust on first use", wie SSH); ab dann muss er passen,
/// sonst bricht die Verbindung **im Handschlag** ab — bevor Kennung, Beweis
/// oder Serverpasswort über die Leitung gehen.
///
/// **Vergleichen allein genügt nicht.** Ein angeheftetes Zertifikat, dessen
/// Besitz niemand beweist, wäre wertlos: der öffentliche Teil steht in jedem
/// Handschlag, wer ihn kennt, könnte ihn vorzeigen. Deshalb prüft diese
/// Struktur zusätzlich die TLS-1.3-Unterschrift mit dem Anbieter von `rustls`
/// (`ring`) — bis 2026-11-14 stand hier für jede Signatur `Ok(())`.
#[derive(Debug)]
pub struct Zertifikatspruefung {
    /// Angehefteter Fingerabdruck (`None` = erster Kontakt).
    erwartet: Option<String>,
    /// Was der Server tatsächlich gezeigt hat (nach dem Aufbau gesetzt).
    gesehen: std::sync::Mutex<Option<String>>,
    /// Warum abgelehnt wurde — für eine Meldung, die den Grund nennt.
    ///
    /// `rustls` reicht den Text einer abgelehnten Prüfung nur als
    /// Transportfehler weiter; ohne dieses Feld stünde im Fenster „TLS-Fehler"
    /// statt „anderes Zertifikat als beim letzten Mal".
    abweichung: std::sync::Mutex<Option<String>>,
}

impl Zertifikatspruefung {
    /// `erwartet`: angehefteter Fingerabdruck oder `None` beim ersten Kontakt.
    pub fn neu(erwartet: Option<&str>) -> Self {
        Self {
            erwartet: erwartet.map(str::to_string),
            gesehen: std::sync::Mutex::new(None),
            abweichung: std::sync::Mutex::new(None),
        }
    }

    /// Der Fingerabdruck, den der Server gezeigt hat (nach dem Aufbau gesetzt).
    pub fn gesehen(&self) -> Option<String> {
        self.gesehen.lock().ok().and_then(|g| g.clone())
    }

    /// Der Grund einer Ablehnung, falls es eine gab.
    pub fn abweichung(&self) -> Option<String> {
        self.abweichung.lock().ok().and_then(|a| a.clone())
    }

    /// Die eigentliche Entscheidung — ohne TLS, damit prüfbar.
    pub fn pruefe_zertifikat(&self, zert: &[u8]) -> Result<(), rustls::Error> {
        let fingerabdruck = sprechwerk_protocol::zertifikat_fingerabdruck(zert);
        if let Err(grund) =
            sprechwerk_protocol::pruefe_fingerabdruck(self.erwartet.as_deref(), &fingerabdruck)
        {
            if let Ok(mut abweichung) = self.abweichung.lock() {
                *abweichung = Some(grund.clone());
            }
            return Err(rustls::Error::General(grund));
        }
        if let Ok(mut gesehen) = self.gesehen.lock() {
            *gesehen = Some(fingerabdruck);
        }
        Ok(())
    }
}

/// Die Unterschriftsverfahren des Anbieters — dieselbe Liste, die auch der
/// TLS-Stapel benutzt. Keine eigene Auswahl: was hier nicht steht, wird nicht
/// geprüft, und was nicht geprüft wird, ist keine Prüfung.
fn verfahren() -> rustls::crypto::WebPkiSupportedAlgorithms {
    anbieter().signature_verification_algorithms
}

/// Die Schlüsselvereinbarungs-Gruppen, in dieser Reihenfolge (ADR 0010).
///
/// **Hybrid, nicht „nur Quanten":** `X25519MLKEM768` ist beides in einem —
/// klassisch (X25519) und quantensicher (ML-KEM-768). Ein Angreifer müsste
/// beide brechen. `X25519` bleibt als zweite Gruppe stehen, damit ein Gegenüber
/// ohne PQ-Unterstützung nicht ausgesperrt ist; die Verbindung ist dann eben
/// nur klassisch, also derselbe Stand wie vor dem Umbau.
///
/// **Gemessen** (ADR 0010): der Handschlag mit ausschließlich PQ-Gruppe dauert
/// auf Loopback 18 ms statt 17 ms — der Aufschlag ist ein paar hundert Byte
/// mehr im ClientHello, kein zusätzlicher Umlauf.
pub fn kx_gruppen() -> Vec<&'static dyn rustls::crypto::SupportedKxGroup> {
    vec![
        rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768,
        rustls::crypto::aws_lc_rs::kx_group::X25519,
    ]
}

/// Der Krypto-Anbieter dieses Programms — **einer** für Handschlag,
/// Unterschriften und Schlüsselaustausch.
///
/// `ring` kennt kein ML-KEM; deshalb `aws-lc-rs`. Zwei Anbieter nebeneinander
/// gehen nicht: `rustls` hat genau einen Standard je Prozess.
///
/// Öffentlich, weil weitere Verbindungen desselben Programms (das Spielnetz,
/// ADR 0016 §2) denselben Anbieter und dieselben Gruppen benutzen müssen —
/// eine zweite Liste wäre eine zweite Wahrheit.
pub fn anbieter() -> rustls::crypto::CryptoProvider {
    let mut anbieter = rustls::crypto::aws_lc_rs::default_provider();
    anbieter.kx_groups = kx_gruppen();
    anbieter
}

impl rustls::client::danger::ServerCertVerifier for Zertifikatspruefung {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        self.pruefe_zertifikat(end_entity.as_ref())?;
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &verfahren())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &verfahren())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        verfahren().supported_schemes()
    }
}

/// Eine offene Verbindung.
pub struct Verbindung {
    conn: quinn::Connection,
    /// Der Endpunkt, über den die Verbindung läuft.
    ///
    /// **Warum er hier bleibt:** beim Beenden muss gewartet werden, bis das
    /// `CONNECTION_CLOSE` wirklich draußen ist ([`Verbindung::schliessen`]).
    /// Dafür braucht es den Endpunkt — ohne ihn endet der Prozess zu früh und
    /// der Server hält das Mitglied weiter für anwesend.
    endpunkt: quinn::Endpoint,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    puffer: Vec<u8>,
    /// Die eigene Kennung nach der Anmeldung.
    pub ich: sprechwerk_domain::MemberId,
    /// Rolle, die der Server vergeben hat.
    pub rolle: sprechwerk_domain::Role,
    /// Servername aus der Begrüßung.
    pub server_name: String,
    /// Fingerabdruck des Zertifikats, das dieser Server gezeigt hat.
    ///
    /// Steht im Modell, damit man ihn **nach** dem Verbinden noch vergleichen
    /// kann — mit dem, was der Betreiber ansagt oder auf seinem Bildschirm
    /// sieht. Ein Wert, den niemand sieht, ist keine Bestätigung.
    pub zertifikat: String,
    /// Raumliste aus der Begrüßung (wird bei Änderungen aktualisiert).
    pub raeume: Vec<Raumansicht>,
    /// Raum, in dem man nach der Anmeldung sitzt (der Standardraum).
    pub raum: Option<sprechwerk_domain::RoomId>,
    /// Mitgliederliste aus der Begrüßung.
    pub mitglieder: Vec<sprechwerk_protocol::Mitgliedsansicht>,
    /// Verlauf des zuletzt besuchten Raums.
    pub verlauf: Vec<sprechwerk_domain::Message>,
    /// Welche Serverschlüssel es gibt (ADR 0012) — aktuell gehalten.
    pub serverschluessel: sprechwerk_protocol::Schluesselstand,
}

impl Verbindung {
    /// Baut die Verbindung auf und meldet sich an — **erster Kontakt**.
    ///
    /// Ohne angehefteten Fingerabdruck: das Zertifikat wird angenommen und
    /// der gesehene Wert in [`Verbindung::zertifikat`] zurückgegeben, damit
    /// der Aufrufer ihn anheften kann. Wer schon einen kennt, nimmt
    /// [`Verbindung::oeffnen_mit`] — sonst wäre die Anheftung wirkungslos.
    pub async fn oeffnen(
        adresse: SocketAddr,
        schluessel: &Schluessel,
        name: &str,
        serverpasswort: Option<&str>,
    ) -> anyhow::Result<Self> {
        Self::oeffnen_mit(adresse, schluessel, name, serverpasswort, None).await
    }

    /// Wie [`Verbindung::oeffnen`], prüft aber gegen einen angehefteten
    /// Fingerabdruck (TOFU, ADR 0002 §3).
    ///
    /// Weicht das Zertifikat ab, scheitert der Handschlag — die Anmeldung
    /// (Kennung, Beweis, Serverpasswort) geht dann **nicht** über die Leitung.
    pub async fn oeffnen_mit(
        adresse: SocketAddr,
        schluessel: &Schluessel,
        name: &str,
        serverpasswort: Option<&str>,
        erwarteter_fingerabdruck: Option<&str>,
    ) -> anyhow::Result<Self> {
        let pruefung = Arc::new(Zertifikatspruefung::neu(erwarteter_fingerabdruck));
        let gewaehlt = anbieter();
        // Der erste Aufruf setzt den Standard des Prozesses; spätere Aufrufe
        // geben den vorhandenen zurück (deshalb `ok()`, nicht `unwrap`).
        gewaehlt.clone().install_default().ok();
        let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(gewaehlt))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(pruefung.clone())
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"sprechwerk/1".to_vec()];
        let quic = QuicClientConfig::try_from(tls)?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(quic));
        let mut transport = quinn::TransportConfig::default();
        transport.datagram_receive_buffer_size(Some(1 << 20));
        transport.keep_alive_interval(Some(std::time::Duration::from_secs(10)));
        // BBR statt Cubic, wie im Server: Cubic füllt beim Hochladen eines
        // Anhangs den Puffer des Routers, und die eigene Sprache steht dann
        // dahinter an (gemessen: +90 ms an 20 Mbit/s, `tests/latenz.rs`,
        // `sprachlatenz_waehrend_einer_uebertragung`).
        transport.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
        client_config.transport_config(Arc::new(transport));

        let mut endpunkt = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        endpunkt.set_default_client_config(client_config);
        let verbunden = endpunkt.connect(adresse, "sprechwerk");
        let conn = match verbunden {
            Ok(aufbau) => match aufbau.await {
                Ok(conn) => conn,
                Err(fehler) => {
                    // Der Grund der Ablehnung steht im Prüfer, nicht im
                    // Transportfehler: „anderes Zertifikat" ist die Meldung,
                    // die der Nutzer braucht.
                    if let Some(grund) = pruefung.abweichung() {
                        anyhow::bail!(grund);
                    }
                    return Err(fehler)
                        .with_context(|| format!("TLS/QUIC-Verbindung zu {adresse}"));
                }
            },
            Err(fehler) => return Err(fehler).context("Verbindung aufbauen"),
        };
        let zertifikat = pruefung.gesehen().unwrap_or_default();

        // Ab hier kann die Gegenstelle schon abgelehnt haben (Passwort): der
        // Grund steht dann nur im `CONNECTION_CLOSE`, nicht im Lesefehler.
        match Self::handschlag(
            conn.clone(),
            endpunkt,
            zertifikat,
            schluessel,
            name,
            serverpasswort,
        )
        .await
        {
            Ok(verbindung) => Ok(verbindung),
            Err(fehler) => Err(ablehnung_deuten(&conn, fehler)),
        }
    }

    /// Schritte 1–3 des ersten Kontakts: Beweis, Anmeldung, Begrüßung.
    async fn handschlag(
        conn: quinn::Connection,
        endpunkt: quinn::Endpoint,
        zertifikat: String,
        schluessel: &Schluessel,
        name: &str,
        serverpasswort: Option<&str>,
    ) -> anyhow::Result<Self> {
        // Der Server öffnet den Steuerstrom (siehe Server: ein Strom wird erst
        // mit den ersten Bytes sichtbar).
        let (mut send, mut recv) = conn.accept_bi().await?;

        // 1. Beweisaufforderung abwarten
        let mut puffer = Vec::new();
        let Some(ServerFrame::Anforderung { version, nonce }) =
            lese(&mut recv, &mut puffer).await?
        else {
            anyhow::bail!("Server hat keine Beweisaufforderung geschickt");
        };
        if version != PROTOKOLL_VERSION {
            anyhow::bail!(
                "Protokollversion {} passt nicht zu {}",
                version,
                PROTOKOLL_VERSION
            );
        }

        // 2. Beweis schicken
        let hallo = ClientFrame::Hallo {
            version: PROTOKOLL_VERSION,
            kennung: schluessel.kennung(),
            name: name.to_string(),
            beweis: schluessel.unterschreibe(&nonce),
            serverpasswort: serverpasswort.map(|p| p.to_string()),
        };
        schreibe(&mut send, &hallo).await?;

        // 3. Begrüßung oder Ablehnung
        let antwort = lese(&mut recv, &mut puffer)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Server hat die Verbindung geschlossen"))?;
        match antwort {
            ServerFrame::Willkommen {
                ich,
                rolle,
                server_name,
                raeume,
                mitglieder,
                raum,
                verlauf,
                serverschluessel,
                ..
            } => Ok(Self {
                conn,
                endpunkt,
                send,
                recv,
                puffer,
                ich,
                rolle,
                server_name,
                zertifikat,
                raeume,
                raum,
                mitglieder,
                verlauf,
                serverschluessel,
            }),
            ServerFrame::Fehler {
                code: sprechwerk_protocol::Fehlercode::AnmeldungAbgelehnt,
                text,
            } => {
                // Der Text ist die Meldung für den Menschen („Passwort falsch").
                anyhow::bail!("{text}")
            }
            ServerFrame::Fehler { code, text } => {
                anyhow::bail!("Anmeldung abgelehnt: {text} ({code:?})")
            }
            anderes => anyhow::bail!("unerwartete Antwort: {anderes:?}"),
        }
    }

    /// Schickt einen Befehl.
    pub async fn sende(&mut self, frame: ClientFrame) -> anyhow::Result<()> {
        schreibe(&mut self.send, &frame).await
    }

    /// Wartet auf den nächsten Rahmen und hält die lokalen Listen aktuell.
    pub async fn empfang(&mut self) -> anyhow::Result<Option<ServerFrame>> {
        let frame = lese(&mut self.recv, &mut self.puffer).await?;
        match &frame {
            Some(ServerFrame::Raeume { raeume }) => self.raeume = raeume.clone(),
            Some(ServerFrame::Serverschluessel { stand }) => self.serverschluessel = stand.clone(),
            Some(ServerFrame::Betreten {
                raum,
                verlauf,
                anwesend,
            }) => {
                self.verlauf = verlauf.clone();
                self.mitglieder = anwesend.clone();
                // Der Raum, in dem man jetzt sitzt — Schlüssel und Text gehören
                // zu ihm, nicht zu dem aus der Begrüßung.
                self.raum = Some(*raum);
            }
            Some(ServerFrame::MitgliedGeaendert { mitglied }) => {
                if let Some(vorhanden) = self.mitglieder.iter_mut().find(|m| m.id == mitglied.id) {
                    *vorhanden = mitglied.clone();
                } else {
                    self.mitglieder.push(mitglied.clone());
                }
            }
            Some(ServerFrame::Anwesenheit {
                mitglied, anwesend, ..
            }) => {
                if *anwesend {
                    if !self.mitglieder.iter().any(|m| m.id == mitglied.id) {
                        self.mitglieder.push(mitglied.clone());
                    }
                } else {
                    self.mitglieder.retain(|m| m.id != mitglied.id);
                }
            }
            _ => {}
        }
        Ok(frame)
    }

    /// Raum nach Position in der Liste (1 = erster).
    pub fn raum_nach_nummer(&self, nummer: usize) -> Option<sprechwerk_domain::RoomId> {
        self.raeume.get(nummer.saturating_sub(1)).map(|r| r.id)
    }

    /// Eine Kopie der QUIC-Verbindung, um Datagramme **getrennt** vom
    /// Steuerstrom zu lesen. Zwei Leser auf demselben Strom wären ein Fehler;
    /// Datagramme dagegen haben ihren eigenen Pfad.
    pub fn datagramm_quelle(&self) -> quinn::Connection {
        self.conn.clone()
    }

    /// Die QUIC-Verbindung für Datenströme (Anhänge, [`anhang`]). Ein
    /// Datenstrom ist ein **eigener** Strom — er stört den Steuerstrom nicht.
    pub fn leitung(&self) -> quinn::Connection {
        self.conn.clone()
    }

    /// Schickt ein Sprach-Datagramm.
    pub fn sprache_senden(&self, datagramm: bytes::Bytes) -> anyhow::Result<()> {
        self.conn.send_datagram(datagramm)?;
        Ok(())
    }

    /// Wartet auf ein Sprach-Datagramm.
    pub async fn sprache_empfangen(&self) -> anyhow::Result<bytes::Bytes> {
        Ok(self.conn.read_datagram().await?)
    }

    /// Schließt die Verbindung sauber.
    ///
    /// **Gefunden 2026-11-19 (Simulation):** hier stand nur `conn.close(…)`.
    /// Das stellt den Rahmen in die Warteschlange; gesendet wird er von der
    /// Triebwerk-Aufgabe des Endpunkts. Wer danach sofort den Prozess beendet
    /// (`sprechwerk-cli` nach `/ende`), verliert ihn — der Server merkt den
    /// Abschied erst nach dem Leerlauf-Timeout (30 s) und zeigt derweil
    /// jemanden als anwesend, der längst weg ist. Deshalb wird hier gewartet,
    /// bis der Endpunkt leer ist, mit einer Obergrenze: ein Abschied darf das
    /// Beenden nicht aufhalten.
    pub async fn schliessen(&self) {
        const FRIST: std::time::Duration = std::time::Duration::from_millis(500);
        self.conn.close(0u32.into(), b"tschuss");
        let _ = tokio::time::timeout(FRIST, self.endpunkt.wait_idle()).await;
    }
}

/// Übersetzt den Schließgrund der Gegenstelle in eine Meldung, die man
/// versteht.
///
/// Ohne das sieht der Nutzer bei falschem Serverpasswort nur
/// „connection lost / closed by peer: 0" — der Grund steht im Schließcode
/// (`protocol::schliessen`), nicht im Transportfehler.
fn ablehnung_deuten(conn: &quinn::Connection, fehler: anyhow::Error) -> anyhow::Error {
    let Some(quinn::ConnectionError::ApplicationClosed(abbruch)) = conn.close_reason() else {
        return fehler;
    };
    let code = abbruch.error_code.into_inner() as u32;
    match sprechwerk_protocol::schliessen::text(code) {
        Some(text) => anyhow::anyhow!(text),
        None => fehler,
    }
}

async fn schreibe(ziel: &mut quinn::SendStream, frame: &ClientFrame) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let bytes = sprechwerk_protocol::schreibe_rahmen(frame)?;
    ziel.write_all(&bytes).await?;
    ziel.flush().await?;
    Ok(())
}

async fn lese(
    quelle: &mut quinn::RecvStream,
    puffer: &mut Vec<u8>,
) -> anyhow::Result<Option<ServerFrame>> {
    loop {
        if let Some((frame, verbraucht)) = sprechwerk_protocol::lese_rahmen::<ServerFrame>(puffer)?
        {
            puffer.drain(..verbraucht);
            return Ok(Some(frame));
        }
        let mut block = [0u8; 8192];
        match quelle.read(&mut block).await? {
            Some(0) | None => return Ok(None),
            Some(n) => puffer.extend_from_slice(&block[..n]),
        }
    }
}

/// Prüft eine Signatur (für Tests und Werkzeuge).
pub fn pruefe_beweis(kennung: &str, nonce: &str, beweis: &str) -> bool {
    use ed25519_dalek::Verifier;
    let Ok(roh) = hex::decode(kennung) else {
        return false;
    };
    let Ok(roh) = <[u8; 32]>::try_from(roh) else {
        return false;
    };
    let Ok(schluessel) = VerifyingKey::from_bytes(&roh) else {
        return false;
    };
    let Ok(sig) = hex::decode(beweis) else {
        return false;
    };
    let Ok(sig) = <[u8; 64]>::try_from(sig) else {
        return false;
    };
    schluessel
        .verify(nonce.as_bytes(), &Signature::from_bytes(&sig))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn geheime_dateien_sind_nur_fuer_den_eigentuemer() {
        use std::os::unix::fs::PermissionsExt;
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-geheim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        let rechte =
            |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        // Neu angelegt: 0600 von Anfang an.
        let schluessel = verzeichnis.join("unter/schluessel");
        Schluessel::laden_oder_erzeugen(&schluessel).unwrap();
        assert_eq!(rechte(&schluessel), 0o600);

        // Vorhanden und für alle lesbar (alter Stand): wird beim Schreiben zu.
        let alt = verzeichnis.join("einstellungen.json");
        std::fs::write(&alt, "{}").unwrap();
        std::fs::set_permissions(&alt, std::fs::Permissions::from_mode(0o644)).unwrap();
        geheim_schreiben(&alt, b"{\"passwort\":\"x\"}").unwrap();
        assert_eq!(rechte(&alt), 0o600);
        assert_eq!(
            std::fs::read_to_string(&alt).unwrap(),
            "{\"passwort\":\"x\"}"
        );

        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[test]
    fn schluessel_signiert_und_prueft() {
        let s = Schluessel::neu();
        let kennung = s.kennung();
        assert_eq!(kennung.len(), 64);
        let beweis = s.unterschreibe("abc");
        assert!(pruefe_beweis(&kennung, "abc", &beweis));
        assert!(!pruefe_beweis(&kennung, "abd", &beweis));
    }

    #[test]
    fn schluessel_ist_mitnehmbar() {
        let s = Schluessel::neu();
        let text = s.export();
        let zurueck = Schluessel::aus_geheim(hex::decode(&text).unwrap().try_into().unwrap());
        assert_eq!(s.kennung(), zurueck.kennung());
    }

    #[test]
    fn kx_gruppen_beginnen_mit_der_hybriden_gruppe() {
        // ADR 0010: hybrid, nicht „nur Quanten" — und die hybride Gruppe muss
        // **zuerst** stehen, sonst bleibt eine Verbindung still klassisch.
        let gruppen = kx_gruppen();
        assert_eq!(
            gruppen[0].name(),
            rustls::NamedGroup::X25519MLKEM768,
            "die hybride Gruppe gehört an die erste Stelle"
        );
        assert_eq!(
            gruppen[1].name(),
            rustls::NamedGroup::X25519,
            "der Rückfallweg bleibt, damit ältere Gegenstellen sich verbinden"
        );
    }

    #[test]
    fn der_anbieter_kennt_die_hybride_gruppe() {
        // `ring` kennt kein ML-KEM: dieser Test schlägt fehl, sobald jemand den
        // Anbieter zurückdreht, ohne die Gruppen mitzunehmen.
        let a = anbieter();
        assert!(a
            .kx_groups
            .iter()
            .any(|g| g.name() == rustls::NamedGroup::X25519MLKEM768));
        assert_eq!(a.kx_groups[0].name(), rustls::NamedGroup::X25519MLKEM768);
    }

    #[test]
    fn erster_kontakt_heftet_den_gesehenen_wert_an() {
        let p = Zertifikatspruefung::neu(None);
        assert!(p.pruefe_zertifikat(b"erster").is_ok());
        assert_eq!(
            p.gesehen().as_deref(),
            Some(sprechwerk_protocol::zertifikat_fingerabdruck(b"erster").as_str()),
            "der gesehene Fingerabdruck muss zum Anheften zurückkommen"
        );
    }

    #[test]
    fn angehefteter_fingerabdruck_wird_geprueft() {
        let richtig = sprechwerk_protocol::zertifikat_fingerabdruck(b"unser-server");
        let p = Zertifikatspruefung::neu(Some(&richtig));
        assert!(p.pruefe_zertifikat(b"unser-server").is_ok());
        assert_eq!(p.gesehen().as_deref(), Some(richtig.as_str()));
        assert!(p.abweichung().is_none());
    }

    #[test]
    fn fremdes_zertifikat_wird_abgelehnt_und_der_grund_gemerkt() {
        let richtig = sprechwerk_protocol::zertifikat_fingerabdruck(b"unser-server");
        let p = Zertifikatspruefung::neu(Some(&richtig));
        let fehler = p.pruefe_zertifikat(b"fremder-server").unwrap_err();
        assert!(fehler.to_string().contains("anderes Zertifikat"));
        // Der Grund muss auch dann lesbar bleiben, wenn `rustls` ihn nur als
        // Transportfehler weiterreicht.
        assert!(p
            .abweichung()
            .unwrap_or_default()
            .contains("anderes Zertifikat"));
        assert!(
            p.gesehen().is_none(),
            "ein abgelehntes Zertifikat darf nicht als gesehen gelten"
        );
    }
}

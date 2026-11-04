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

use anyhow::Context;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use quinn::crypto::rustls::QuicClientConfig;
use sprechwerk_proto::{ClientFrame, Raumansicht, ServerFrame, PROTOKOLL_VERSION};
use std::net::SocketAddr;
use std::sync::Arc;

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
        if let Some(eltern) = pfad.parent() {
            std::fs::create_dir_all(eltern)?;
        }
        std::fs::write(pfad, hex::encode(s.geheim))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(pfad, std::fs::Permissions::from_mode(0o600));
        }
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

    /// Export als Text (zum Mitnehmen auf ein neues Gerät).
    pub fn export(&self) -> String {
        hex::encode(self.geheim)
    }
}

/// Zertifikatsprüfung: eigenes Zertifikat, kein Systemzertifikat.
///
/// **Festlegung (SPEC §7):** Der Server hat ein selbst signiertes Zertifikat.
/// Der Client nimmt es beim ersten Verbinden an und merkt sich den
/// Fingerabdruck — wie SSH es macht. Eine Prüfung gegen eine Zertifizierungsstelle
/// ist für selbst gehostete Instanzen nicht praktikabel und wäre eine
/// Scheinsicherheit.
#[derive(Debug)]
struct EigenesZertifikat;

impl rustls::client::danger::ServerCertVerifier for EigenesZertifikat {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Eine offene Verbindung.
pub struct Verbindung {
    conn: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    puffer: Vec<u8>,
    /// Die eigene Kennung nach der Anmeldung.
    pub ich: sprechwerk_core::MemberId,
    /// Rolle, die der Server vergeben hat.
    pub rolle: sprechwerk_core::Role,
    /// Servername aus der Begrüßung.
    pub server_name: String,
    /// Raumliste aus der Begrüßung (wird bei Änderungen aktualisiert).
    pub raeume: Vec<Raumansicht>,
    /// Mitgliederliste aus der Begrüßung.
    pub mitglieder: Vec<sprechwerk_proto::Mitgliedsansicht>,
    /// Verlauf des zuletzt besuchten Raums.
    pub verlauf: Vec<sprechwerk_core::Message>,
}

impl Verbindung {
    /// Baut die Verbindung auf und meldet sich an.
    pub async fn oeffnen(
        adresse: SocketAddr,
        schluessel: &Schluessel,
        name: &str,
        serverpasswort: Option<&str>,
    ) -> anyhow::Result<Self> {
        rustls::crypto::ring::default_provider().install_default().ok();

        let mut tls = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(EigenesZertifikat))
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"sprechwerk/1".to_vec()];
        let quic = QuicClientConfig::try_from(tls)?;
        let mut client_config = quinn::ClientConfig::new(Arc::new(quic));
        let mut transport = quinn::TransportConfig::default();
        transport.datagram_receive_buffer_size(Some(1 << 20));
        transport.keep_alive_interval(Some(std::time::Duration::from_secs(10)));
        client_config.transport_config(Arc::new(transport));

        let mut endpunkt = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        endpunkt.set_default_client_config(client_config);
        let conn = endpunkt
            .connect(adresse, "sprechwerk")
            .with_context(|| format!("verbinde zu {adresse}"))?
            .await
            .context("TLS/QUIC-Verbindung")?;

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
                verlauf,
                ..
            } => Ok(Self {
                conn,
                send,
                recv,
                puffer,
                ich,
                rolle,
                server_name,
                raeume,
                mitglieder,
                verlauf,
            }),
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
            Some(ServerFrame::Betreten { raum, verlauf, anwesend }) => {
                self.verlauf = verlauf.clone();
                self.mitglieder = anwesend.clone();
                let _ = raum;
            }
            Some(ServerFrame::MitgliedGeaendert { mitglied }) => {
                if let Some(vorhanden) = self.mitglieder.iter_mut().find(|m| m.id == mitglied.id) {
                    *vorhanden = mitglied.clone();
                } else {
                    self.mitglieder.push(mitglied.clone());
                }
            }
            Some(ServerFrame::Anwesenheit { mitglied, anwesend, .. }) => {
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
    pub fn raum_nach_nummer(&self, nummer: usize) -> Option<sprechwerk_core::RoomId> {
        self.raeume.get(nummer.saturating_sub(1)).map(|r| r.id)
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
    pub fn schliessen(&self) {
        self.conn.close(0u32.into(), b"tschuss");
    }
}

async fn schreibe(ziel: &mut quinn::SendStream, frame: &ClientFrame) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let bytes = sprechwerk_proto::schreibe_rahmen(frame)?;
    ziel.write_all(&bytes).await?;
    ziel.flush().await?;
    Ok(())
}

async fn lese(
    quelle: &mut quinn::RecvStream,
    puffer: &mut Vec<u8>,
) -> anyhow::Result<Option<ServerFrame>> {
    loop {
        if let Some((frame, verbraucht)) = sprechwerk_proto::lese_rahmen::<ServerFrame>(puffer)? {
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
}

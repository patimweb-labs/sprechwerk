//! Die Clientseite des Spielnetzes — nur der Verbindungsaufbau.
//!
//! Was danach kommt (Freigaben anzeigen, lokale Ports öffnen, ver- und
//! entschlüsseln) gehört in den Client-Kern und die Fenster; hier steht
//! bewusst nicht mehr als nötig: **eine zweite QUIC-Verbindung zum selben
//! Port**, unterschieden am ALPN ([`ALPN`]), angemeldet mit demselben Beweis
//! wie die Sitzung.
//!
//! # Warum eine eigene Verbindung
//!
//! Spielverkehr teilt sich weder Steuerstrom noch Staukontrolle mit der
//! Sprache (ADR 0016 §2). Ein großes Update im getunnelten Spiel darf die
//! Sprache nicht hinter sich anstehen lassen — und umgekehrt.
//!
//! # Was der Client sieht, wenn der Server das Modul nicht anbietet
//!
//! Der Handschlag scheitert am ALPN. Das ist kein Netzfehler, sondern eine
//! Auskunft: [`Anmeldefehler::NichtAngeboten`]. Der Client zeigt dann „Dieser
//! Server bietet kein Spielnetz an", statt das Angebot zu verstecken
//! (ADR 0016 §3).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use quinn::crypto::rustls::QuicClientConfig;
use tokio::io::AsyncWriteExt;

use sprechwerk_adapters::{anbieter, Schluessel, Zertifikatspruefung};

use crate::protokoll::{schreibe, ClientRahmen, Grenzen, ServerRahmen, ALPN, VERSION};

/// Warum die Anmeldung nicht geklappt hat.
///
/// `NichtAngeboten` ist der einzige Fall, der **kein** Fehler ist: der Server
/// hat das Modul abgeschaltet (ADR 0016 §3). Alles andere ist eine Ablehnung
/// oder ein Netzproblem.
#[derive(Debug, thiserror::Error)]
pub enum Anmeldefehler {
    /// Der Handschlag scheiterte am ALPN — der Server bietet kein Spielnetz an.
    #[error("dieser Server bietet kein Spielnetz an")]
    NichtAngeboten,
    /// Alles andere: falscher Beweis, keine Sitzung, kein Recht, Netzfehler.
    #[error("{0}")]
    Sonst(#[from] anyhow::Error),
}

/// Eine angemeldete Spielnetz-Verbindung.
#[derive(Debug)]
pub struct Steuerung {
    /// Die QUIC-Verbindung — für Ströme (TCP-Tunnel) und Datagramme (UDP).
    pub conn: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    puffer: Vec<u8>,
}

impl Steuerung {
    /// Schickt einen Steuerrahmen.
    pub async fn sende(&mut self, rahmen: ClientRahmen) -> anyhow::Result<()> {
        let bytes = schreibe(&rahmen)?;
        self.send.write_all(&bytes).await?;
        self.send.flush().await?;
        Ok(())
    }

    /// Wartet auf den nächsten Steuerrahmen. `None` heißt: die Gegenstelle hat
    /// den Strom beendet.
    pub async fn empfang(&mut self) -> anyhow::Result<Option<ServerRahmen>> {
        lese(&mut self.recv, &mut self.puffer).await
    }
}

/// Baut die Spielnetz-Verbindung auf und meldet sich an.
///
/// Das Zertifikat wird gegen `fingerabdruck` geprüft (TOFU, wie die Sitzung,
/// ADR 0002 §3) — derselbe Wert, den der Server in der Sitzung zeigt.
pub async fn anmelden(
    adresse: SocketAddr,
    schluessel: &Schluessel,
    fingerabdruck: &str,
) -> Result<(Steuerung, Grenzen), Anmeldefehler> {
    let pruefung = Arc::new(Zertifikatspruefung::neu(Some(fingerabdruck)));

    // Derselbe Krypto-Anbieter wie Sitzung und Server (ADR 0010): ein Anbieter
    // je Prozess, und der erste Aufruf setzt ihn.
    let gewaehlt = anbieter();
    gewaehlt.clone().install_default().ok();
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(gewaehlt))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .context("TLS 1.3")?
        .dangerous()
        .with_custom_certificate_verifier(pruefung.clone())
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let quic = QuicClientConfig::try_from(tls).context("QUIC-Verschlüsselung")?;
    let mut client_config = quinn::ClientConfig::new(Arc::new(quic));

    let mut transport = quinn::TransportConfig::default();
    // Datagramme sind der UDP-Tunnel; derselbe Puffer wie in der Sitzung.
    transport.datagram_receive_buffer_size(Some(1 << 20));
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    client_config.transport_config(Arc::new(transport));

    let mut endpunkt = quinn::Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0)))
        .map_err(|fehler| Anmeldefehler::Sonst(anyhow::Error::from(fehler)))?;
    endpunkt.set_default_client_config(client_config);

    let verbunden = match endpunkt.connect(adresse, "sprechwerk") {
        Ok(aufbau) => aufbau,
        Err(fehler) => return Err(Anmeldefehler::Sonst(anyhow::Error::from(fehler))),
    };
    let conn = match verbunden.await {
        Ok(conn) => conn,
        Err(fehler) => {
            // Ein abweichendes Zertifikat ist die Meldung, die der Mensch braucht.
            if let Some(grund) = pruefung.abweichung() {
                return Err(Anmeldefehler::Sonst(anyhow::anyhow!(grund)));
            }
            if alpn_verweigert(&fehler) {
                return Err(Anmeldefehler::NichtAngeboten);
            }
            return Err(Anmeldefehler::Sonst(
                anyhow::Error::from(fehler).context(format!("TLS/QUIC-Verbindung zu {adresse}")),
            ));
        }
    };

    match handschlag(&conn, schluessel).await {
        Ok(ergebnis) => Ok(ergebnis),
        // Der Grund einer Ablehnung steht im Schließen, wenn der Rahmen im
        // Schließen untergeht — dieselbe Meldung, nur ein anderer Weg.
        Err(fehler) => Err(Anmeldefehler::Sonst(ablehnung_deuten(&conn, fehler))),
    }
}

/// Schritte 1–3: Beweisaufforderung, Hallo, Willkommen.
async fn handschlag(
    conn: &quinn::Connection,
    schluessel: &Schluessel,
) -> anyhow::Result<(Steuerung, Grenzen)> {
    // Der Server öffnet den Steuerstrom (ein Strom wird erst mit den ersten
    // Bytes sichtbar).
    let (mut send, mut recv) = conn.accept_bi().await?;

    // 1. Beweisaufforderung
    let mut puffer = Vec::new();
    match lese::<ServerRahmen>(&mut recv, &mut puffer).await? {
        Some(ServerRahmen::Anforderung { version, nonce }) => {
            if version != VERSION {
                anyhow::bail!("Spielnetz-Version {} passt nicht zu {}", version, VERSION);
            }
            // 2. Beweis über die Nonce — dieselbe Signatur wie in der Sitzung.
            let hallo = ClientRahmen::Hallo {
                version: VERSION,
                kennung: schluessel.kennung(),
                beweis: schluessel.unterschreibe(&nonce),
            };
            send.write_all(&schreibe(&hallo)?).await?;
            send.flush().await?;
        }
        Some(anderes) => anyhow::bail!("unerwartete Antwort: {anderes:?}"),
        None => anyhow::bail!("Server hat keine Beweisaufforderung geschickt"),
    }

    // 3. Willkommen oder Fehler
    match lese::<ServerRahmen>(&mut recv, &mut puffer).await? {
        Some(ServerRahmen::Willkommen { grenzen }) => Ok((
            Steuerung {
                conn: conn.clone(),
                send,
                recv,
                puffer,
            },
            grenzen,
        )),
        // Der stabile Text des Codes ist die Meldung („nicht angemeldet",
        // „dafür fehlt das Recht").
        Some(ServerRahmen::Fehler { code, .. }) => anyhow::bail!(code.text()),
        Some(anderes) => anyhow::bail!("unerwartete Antwort: {anderes:?}"),
        None => anyhow::bail!("Server hat die Verbindung geschlossen"),
    }
}

/// Ist der Handschlag am ALPN gescheitert?
///
/// TLS 1.3 meldet „kein gemeinsames Protokoll" als Alert 120; QUIC macht daraus
/// den Transportfehler `0x100 | 0x78`. Das ist die einzige Stelle, an der der
/// Client erkennen kann, dass der Server das Modul **abgeschaltet** hat und
/// nicht etwa nicht erreichbar ist.
fn alpn_verweigert(fehler: &quinn::ConnectionError) -> bool {
    const NO_APPLICATION_PROTOCOL: u64 = 0x100 + 0x78;
    match fehler {
        quinn::ConnectionError::TransportError(t) => u64::from(t.code) == NO_APPLICATION_PROTOCOL,
        quinn::ConnectionError::ConnectionClosed(c) => {
            u64::from(c.error_code) == NO_APPLICATION_PROTOCOL
        }
        _ => false,
    }
}

/// Ersetzt „closed by peer" durch den Grund, den der Server genannt hat.
fn ablehnung_deuten(conn: &quinn::Connection, fehler: anyhow::Error) -> anyhow::Error {
    let Some(quinn::ConnectionError::ApplicationClosed(abbruch)) = conn.close_reason() else {
        return fehler;
    };
    if abbruch.error_code.into_inner() as u32 != crate::server::SCHLIESSEN_ABGELEHNT {
        return fehler;
    }
    let text = String::from_utf8_lossy(&abbruch.reason).to_string();
    if text.is_empty() {
        fehler
    } else {
        anyhow::anyhow!(text)
    }
}

/// Liest genau einen Rahmen (wartet, bis er vollständig ist).
async fn lese<T: serde::de::DeserializeOwned>(
    quelle: &mut quinn::RecvStream,
    puffer: &mut Vec<u8>,
) -> anyhow::Result<Option<T>> {
    loop {
        if let Some((wert, verbraucht)) = sprechwerk_protocol::lese_rahmen::<T>(puffer)? {
            puffer.drain(..verbraucht);
            return Ok(Some(wert));
        }
        let mut block = [0u8; 8192];
        match quelle.read(&mut block).await? {
            None | Some(0) => return Ok(None),
            Some(n) => puffer.extend_from_slice(&block[..n]),
        }
    }
}

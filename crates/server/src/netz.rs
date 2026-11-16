//! Der QUIC-Dienst: Verbindungen annehmen, Anmeldung prüfen, Rahmen verteilen.
//!
//! Ablauf einer Verbindung (**Festlegung SPEC §4/§7**):
//!
//! ```text
//! Client                                  Server
//!   ── QUIC-Verbindung (TLS 1.3) ──►
//!   ◄── Anforderung { nonce }            ← Beweisaufforderung
//!   ── Hallo { kennung, beweis } ──►      ← Signatur über den Nonce
//!   ◄── Willkommen { Räume, Mitglieder }  ← oder Fehler
//!   ◄──► Frames (Text, Befehle)           ← ein Strom, längenpräfixiertes JSON
//!   ◄──► Datagramme (Sprache)             ← unzuverlässig, ungeordnet
//! ```
//!
//! Der `nonce` ist der Grund, warum eine mitgelesene Anmeldung nicht
//! wiederverwendbar ist: jede Verbindung fordert einen neuen Beweis.

use anyhow::Context;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use quinn::crypto::rustls::QuicServerConfig;
use sprechwerk_domain::MemberId;
use sprechwerk_protocol::{ClientFrame, Fehlercode, ServerFrame, PROTOKOLL_VERSION};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::kern::Kern;

/// Zufall als Hexadezimaltext.
pub fn zufall_hex(anzahl: usize) -> String {
    let mut roh = vec![0u8; anzahl];
    getrandom::getrandom(&mut roh).expect("Zufall aus dem Betriebssystem");
    hex::encode(roh)
}

/// Startet den Dienst und kehrt erst zurück, wenn der Endpunkt endet.
pub async fn lauschen(kern: Arc<Mutex<Kern>>) -> anyhow::Result<()> {
    // Quantensicherheit (ADR 0010): derselbe Anbieter und dieselbe
    // Gruppenreihenfolge wie im Client (`crates/adapters`). `ring` kennt kein
    // ML-KEM, deshalb `aws-lc-rs`. Ein Anbieter je Prozess — zwei gehen nicht.
    let anbieter = {
        let mut a = rustls::crypto::aws_lc_rs::default_provider();
        a.kx_groups = vec![
            rustls::crypto::aws_lc_rs::kx_group::X25519MLKEM768,
            rustls::crypto::aws_lc_rs::kx_group::X25519,
        ];
        a
    };
    // Erster Aufruf setzt den Standard des Prozesses (spätere geben ihn zurück).
    anbieter.clone().install_default().ok();
    let anbieter = Arc::new(anbieter);

    let (adresse, daten, staukontrolle, sprachpuffer) = {
        let k = kern.lock().expect("Sperre");
        (
            k.konfig.adresse(),
            k.konfig.server.daten.clone(),
            k.konfig.server.staukontrolle.clone(),
            k.konfig.server.sprachpuffer_kib * 1024,
        )
    };

    let zert = crate::tls::laden_oder_erzeugen(&daten)?;
    // TLS 1.3 (von QUIC vorgeschrieben) mit dem eigenen Zertifikat.
    let mut tls = rustls::ServerConfig::builder_with_provider(anbieter)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .context("TLS 1.3")?
        .with_no_client_auth()
        .with_single_cert(zert.kette.clone(), zert.schluessel.clone_key())
        .context("QUIC-Konfiguration")?;
    tls.alpn_protocols = vec![b"sprechwerk/1".to_vec()];

    let quic = QuicServerConfig::try_from(tls).context("QUIC-Verschlüsselung")?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic));

    // Datagramme sind der Sprachpfad: großzügiger Puffer, straffe Fristen.
    let mut transport = quinn::TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(sprachpuffer));
    transport.datagram_send_buffer_size(sprachpuffer);
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into()?));
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    // Staukontrolle: BBR schätzt die Bandbreite, statt bei Verlust zu halbieren.
    if staukontrolle == "bbr" {
        transport.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
    } else {
        transport
            .congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
    }
    server_config.transport_config(Arc::new(transport));

    let adresse: std::net::SocketAddr = adresse.parse()?;
    let endpunkt = quinn::Endpoint::server(server_config, adresse)
        .with_context(|| format!("binde UDP {adresse}"))?;

    tracing::info!(
        adresse = %adresse,
        fingerabdruck = %zert.fingerabdruck,
        "lauscht"
    );
    println!("──────────────────────────────────────────────────────────────");
    println!("sprechwerk lauscht auf {adresse}");
    println!("Zertifikat-Fingerabdruck (SHA-256):");
    println!("  {}", gruppiere(&zert.fingerabdruck));
    println!("Diesen Wert beim ersten Verbinden bestätigen.");
    println!("──────────────────────────────────────────────────────────────");

    while let Some(ankunft) = endpunkt.accept().await {
        let kern = kern.clone();
        tokio::spawn(async move {
            // `Incoming` ist die unbestätigte Ankunft; erst `await` macht
            // daraus eine Verbindung (TLS-Handshake abgeschlossen).
            let Ok(verbindung) = ankunft.await else {
                return;
            };
            if let Err(fehler) = bediene(verbindung, kern).await {
                tracing::debug!(fehler = %fehler, "Verbindung beendet");
            }
        });
    }
    Ok(())
}

fn gruppiere(hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join(":")
}

async fn schreibe<W: AsyncWrite + Unpin>(ziel: &mut W, frame: &ServerFrame) -> anyhow::Result<()> {
    let bytes = sprechwerk_protocol::schreibe_rahmen(frame)?;
    tokio::io::AsyncWriteExt::write_all(ziel, &bytes).await?;
    tokio::io::AsyncWriteExt::flush(ziel).await?;
    Ok(())
}

/// Liest genau einen Rahmen (wartet, bis er vollständig ist).
async fn lese<R: AsyncRead + Unpin>(
    quelle: &mut R,
    puffer: &mut Vec<u8>,
) -> anyhow::Result<Option<ClientFrame>> {
    loop {
        if let Some((frame, verbraucht)) = sprechwerk_protocol::lese_rahmen::<ClientFrame>(puffer)?
        {
            puffer.drain(..verbraucht);
            return Ok(Some(frame));
        }
        let mut block = [0u8; 8192];
        let n = tokio::io::AsyncReadExt::read(quelle, &mut block).await?;
        if n == 0 {
            return Ok(None);
        }
        puffer.extend_from_slice(&block[..n]);
    }
}

/// Begrenzt, wie viele Steuerrahmen eine Verbindung je Sekunde schicken darf.
///
/// **Warum überhaupt:** ohne Grenze kann ein Client den Server mit Rahmen
/// zuschütten (jeder Rahmen kostet eine Sperre, eine Zustandsänderung oder
/// einen Rundruf an alle). Sprache läuft als Datagramm daneben und wird hier
/// nicht gezählt — sie hat ihre eigene Grenze (Sendepuffer, `MAX_SPRACHRAHMEN`).
///
/// Ein einfaches Fenster je Sekunde, keine Gleitkommarechnung: der Zähler
/// beginnt neu, sobald eine Sekunde um ist. Damit ist die Grenze im
/// schlechtesten Fall doppelt so hoch (zwei Fenster hintereinander) — für einen
/// Schutz gegen Fluten reicht das, und es ist ohne Tabellen prüfbar.
struct Rahmendrossel {
    /// Erlaubte Rahmen je Sekunde; 0 = unbegrenzt.
    grenze: u32,
    /// Beginn des laufenden Fensters.
    fenster: std::time::Instant,
    /// Im laufenden Fenster gezählte Rahmen.
    gezaehlt: u32,
    /// Wie oft die Grenze schon erreicht wurde (für die Protokollzeile).
    abgewiesen: u64,
}

impl Rahmendrossel {
    fn neu(grenze: u32) -> Self {
        Self {
            grenze,
            fenster: std::time::Instant::now(),
            gezaehlt: 0,
            abgewiesen: 0,
        }
    }

    /// Darf jetzt ein weiterer Rahmen verarbeitet werden?
    fn erlaubt(&mut self, jetzt: std::time::Instant) -> bool {
        if self.grenze == 0 {
            return true;
        }
        if jetzt.duration_since(self.fenster) >= std::time::Duration::from_secs(1) {
            self.fenster = jetzt;
            self.gezaehlt = 0;
        }
        if self.gezaehlt < self.grenze {
            self.gezaehlt += 1;
            true
        } else {
            self.abgewiesen += 1;
            false
        }
    }
}

async fn bediene(verbindung: quinn::Connection, kern: Arc<Mutex<Kern>>) -> anyhow::Result<()> {
    let adresse = verbindung.remote_address().ip().to_string();
    tracing::debug!(%adresse, "Verbindung angenommen");

    // **Der Server öffnet den Strom**, nicht der Client: ein QUIC-Strom wird
    // der Gegenstelle erst mit den ersten Bytes sichtbar. Würde der Client
    // öffnen und auf die Beweisaufforderung warten, während der Server auf
    // `accept_bi` wartet, stünden beide still.
    let (mut send, mut recv) = verbindung.open_bi().await?;

    // 1. Beweisaufforderung
    let nonce = zufall_hex(32);
    schreibe(
        &mut send,
        &ServerFrame::Anforderung {
            version: PROTOKOLL_VERSION,
            nonce: nonce.clone(),
        },
    )
    .await?;

    // 2. Hallo
    let mut puffer = Vec::new();
    let frame = lese(&mut recv, &mut puffer).await?;
    let Some(frame) = frame else {
        return Ok(());
    };
    let ClientFrame::Hallo {
        version,
        kennung,
        name,
        beweis,
        serverpasswort,
    } = frame
    else {
        schreibe(&mut send, &Kern::fehler(Fehlercode::UngueltigeAnfrage)).await?;
        return Ok(());
    };

    // 3. Prüfungen
    if version != PROTOKOLL_VERSION {
        schreibe(&mut send, &Kern::fehler(Fehlercode::Version)).await?;
        return Ok(());
    }
    if !beweis_passt(&kennung, &nonce, &beweis) {
        tracing::warn!(%adresse, "Anmeldung: Beweis falsch");
        schreibe(&mut send, &Kern::fehler(Fehlercode::AnmeldungAbgelehnt)).await?;
        return Ok(());
    }

    let (postfach, mut empfanger) = tokio::sync::mpsc::unbounded_channel::<ServerFrame>();

    // Die Sperre darf **nie** über ein `await` gehalten werden: der
    // Zustandsschutz ist ein normaler Mutex, und ein gehaltener
    // Schutz über eine Wartezeit hinweg blockiert den ganzen Server.
    // Deshalb: erst prüfen und ändern (synchron), dann schreiben (asynchron).
    enum Anmeldung {
        Ok(MemberId, ServerFrame),
        Abgelehnt(ServerFrame),
    }
    let anmeldung = {
        let mut k = kern.lock().expect("Sperre");
        let passwort_stimmt = k.konfig.server.passwort.is_empty()
            || serverpasswort.as_deref() == Some(k.konfig.server.passwort.as_str());
        if !passwort_stimmt {
            tracing::info!(%adresse, "Anmeldung: Serverpasswort falsch");
            Anmeldung::Abgelehnt(Kern::fehler(Fehlercode::AnmeldungAbgelehnt))
        } else {
            match k.anmelden(&kennung, &name, &adresse) {
                Ok((ich, antwort)) => {
                    let sprechnummer = k.sprechnummer_vergeben();
                    k.teilnehmer.insert(
                        ich,
                        crate::kern::Teilnehmer {
                            conn: verbindung.clone(),
                            postfach: postfach.clone(),
                            letzter_rahmen: 0,
                            sprechnummer,
                        },
                    );
                    tracing::info!(mitglied = %ich, name, %adresse, "angemeldet");
                    Anmeldung::Ok(ich, antwort)
                }
                Err(fehler) => Anmeldung::Abgelehnt(*fehler),
            }
        }
    };
    let (ich, antwort) = match anmeldung {
        Anmeldung::Ok(ich, antwort) => (ich, antwort),
        Anmeldung::Abgelehnt(fehler) => {
            schreibe(&mut send, &fehler).await?;
            return Ok(());
        }
    };
    schreibe(&mut send, &antwort).await?;

    // 4. Schreibaufgabe: alles, was andere an uns schicken, geht hier hinaus.
    let schreiber = tokio::spawn(async move {
        while let Some(frame) = empfanger.recv().await {
            if schreibe(&mut send, &frame).await.is_err() {
                break;
            }
        }
    });

    // 5. Leseschleife: Rahmen und Sprach-Datagramme.
    let mut drossel =
        Rahmendrossel::neu(kern.lock().expect("Sperre").konfig.server.rahmen_je_sekunde);
    let ergebnis = loop {
        tokio::select! {
            gelesen = lese(&mut recv, &mut puffer) => {
                match gelesen {
                    Ok(Some(frame)) => {
                        if !drossel.erlaubt(std::time::Instant::now()) {
                            // Zu viel in einer Sekunde: einmal sagen, dann trennen.
                            tracing::warn!(
                                mitglied = %ich,
                                grenze = drossel.grenze,
                                abgewiesen = drossel.abgewiesen,
                                "Rahmengrenze überschritten — Verbindung wird getrennt"
                            );
                            let k = kern.lock().expect("Sperre");
                            k.senden(ich, Kern::fehler(Fehlercode::Spaeter));
                            drop(k);
                            // Der Schreibstrom gehört der Schreibaufgabe; die
                            // Verbindung zu schließen genügt, damit die letzte
                            // Meldung noch hinausgeht.
                            verbindung.close(4u32.into(), b"zu viele Rahmen");
                            break Ok(());
                        }
                        // Ausführen und Antwort zustellen unter **einer**
                        // Sperre. `befehle` meldet die Änderung sofort an alle
                        // anderen; lägen dazwischen zwei Sperren, könnte eine
                        // andere Verbindung ihre Meldung *vor* unsere Antwort
                        // in unser Postfach legen. Der Client überschreibt dann
                        // mit der älteren Momentaufnahme (`Betreten {
                        // anwesend }`) die neuere Meldung — wer gleichzeitig
                        // kam, war für ihn verschwunden (und bekam als Folge
                        // keinen Raumschlüssel). `senden` wartet nicht, die
                        // Sperre bleibt also kurz.
                        // Prüfung: `tests/rundlauf.rs`,
                        // `gleichzeitiges_betreten_laesst_niemanden_verschwinden`.
                        {
                            let mut k = kern.lock().expect("Sperre");
                            for a in k.befehle(ich, frame) {
                                k.senden(ich, a);
                            }
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(fehler) => break Err(fehler),
                }
            }
            datagramm = verbindung.read_datagram() => {
                match datagramm {
                    Ok(d) => {
                        let mut k = kern.lock().expect("Sperre");
                        k.sprache(ich, &d);
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_)) => break Ok(()),
                    Err(quinn::ConnectionError::LocallyClosed) => break Ok(()),
                    Err(_) => break Ok(()),
                }
            }
        }
    };

    {
        let mut k = kern.lock().expect("Sperre");
        k.abmelden(ich);
    }
    schreiber.abort();
    let _ = verbindung.closed().await;
    ergebnis
}

/// Prüft die Signatur über den Nonce.
fn beweis_passt(kennung: &str, nonce: &str, beweis: &str) -> bool {
    let Ok(schluessel_roh) = hex::decode(kennung) else {
        return false;
    };
    let Ok(schluessel_roh) = <[u8; 32]>::try_from(schluessel_roh) else {
        return false;
    };
    let Ok(schluessel) = VerifyingKey::from_bytes(&schluessel_roh) else {
        return false;
    };
    let Ok(signatur_roh) = hex::decode(beweis) else {
        return false;
    };
    let Ok(signatur_roh) = <[u8; 64]>::try_from(signatur_roh) else {
        return false;
    };
    let signatur = Signature::from_bytes(&signatur_roh);
    schluessel.verify(nonce.as_bytes(), &signatur).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn beweis_wird_geprueft() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let kennung = hex::encode(sk.verifying_key().to_bytes());
        let nonce = "abc123";
        let beweis = hex::encode(sk.sign(nonce.as_bytes()).to_bytes());
        assert!(beweis_passt(&kennung, nonce, &beweis));
        // andere Nonce, anderer Beweis → falsch
        assert!(!beweis_passt(&kennung, "abc124", &beweis));
        // kaputte Eingaben → falsch, kein Absturz
        assert!(!beweis_passt("nicht-hex", nonce, &beweis));
        assert!(!beweis_passt(&kennung, nonce, "00"));
    }
}

#[cfg(test)]
mod drosseltests {
    use super::Rahmendrossel;
    use std::time::{Duration, Instant};

    /// Die Grenze gilt je Sekunde — und öffnet sich danach wieder.
    #[test]
    fn grenze_gilt_je_sekunde() {
        let start = Instant::now();
        let mut d = Rahmendrossel::neu(3);
        // Genau die Grenze geht durch.
        for i in 0..3 {
            assert!(
                d.erlaubt(start + Duration::from_millis(i * 10)),
                "Rahmen {i}"
            );
        }
        // Der vierte nicht.
        assert!(!d.erlaubt(start + Duration::from_millis(40)));
        assert!(!d.erlaubt(start + Duration::from_millis(900)));
        assert_eq!(d.abgewiesen, 2, "die Ablehnungen müssen gezählt werden");

        // Neue Sekunde: wieder drei.
        let spaeter = start + Duration::from_millis(1_001);
        assert!(d.erlaubt(spaeter));
        assert!(d.erlaubt(spaeter + Duration::from_millis(1)));
        assert!(d.erlaubt(spaeter + Duration::from_millis(2)));
        assert!(!d.erlaubt(spaeter + Duration::from_millis(3)));
    }

    /// 0 heißt unbegrenzt (für Prüfungen und Notfälle).
    #[test]
    fn null_ist_unbegrenzt() {
        let start = Instant::now();
        let mut d = Rahmendrossel::neu(0);
        for i in 0..10_000 {
            assert!(d.erlaubt(start + Duration::from_micros(i)));
        }
        assert_eq!(d.abgewiesen, 0);
    }

    /// Ein Fenster ohne Verkehr öffnet sich ebenfalls (kein Nachholen).
    #[test]
    fn stille_oeffnet_wieder() {
        let start = Instant::now();
        let mut d = Rahmendrossel::neu(1);
        assert!(d.erlaubt(start));
        assert!(!d.erlaubt(start + Duration::from_millis(500)));
        assert!(
            d.erlaubt(start + Duration::from_millis(2_000)),
            "nach einer Sekunde Stille muss wieder etwas durchgehen"
        );
    }
}

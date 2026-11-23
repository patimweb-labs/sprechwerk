//! Die Serverseite des Spielnetzes: Verbindung annehmen, Anmeldung prüfen,
//! Rahmen an die [`Vermittlung`] reichen, deren Aktionen ausführen und Daten
//! weiterleiten.
//!
//! # Was hier steht und was nicht
//!
//! Dieser Baustein weiß **nichts** über Räume, Rollen, Mitglieder oder den
//! Sitzungszustand des Servers. Er fragt einen [`Mitgliedschaft`] nach einer
//! Momentaufnahme (`stand()`), und alles Weitere entscheidet die
//! [`Vermittlung`] aus [`crate::vermittlung`] — derselbe Automat, der ohne Netz
//! geprüft ist. Damit ist die Grenze aus ADR 0016 §6 eingehalten: das Modul
//! kennt weder `Kern` noch `Anwendung`.
//!
//! # Die zwei Sperren
//!
//! * Die [`Vermittlung`] liegt hinter einem `std::sync::Mutex`. Sie wird **nie**
//!   über ein `await` gehalten — erst rechnen (synchron), dann senden
//!   (asynchron).
//! * `stand()` sperrt den Kern des Servers. Der Aufruf passiert, **bevor** die
//!   eigene Sperre genommen wird, damit sich die beiden nicht überkreuzen.
//!
//! # Was der Server sieht
//!
//! Köpfe und Mengen: wer welche Freigabe anbietet, welcher Strom zu welchem
//! Paar gehört, wie viele Byte fließen. Was durch die Ströme und Datagramme
//! geht, ist Geheimtext (ADR 0012, ADR 0016 §5).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use quinn::VarInt;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Notify;
use tokio::task::AbortHandle;

use crate::protokoll::{
    schreibe, AngebotId, ClientRahmen, Datagrammkopf, Fehler, Grenzen, ServerRahmen, StromKopf,
    VERSION,
};
use crate::vermittlung::{Aktion, Ereignis, Vermittlung};

/// Steuerrahmen je Sekunde und Verbindung. Wie bei der Sitzung
/// (`crates/server/src/netz.rs`): Spielverkehr zählt nicht mit, ein Mensch
/// erzeugt wenige Rahmen.
pub const RAHMEN_JE_SEKUNDE: u32 = 50;

/// Anwendungscode beim Schließen: Anmeldung abgelehnt oder Sitzung weg. Der
/// Text daneben sagt warum.
pub const SCHLIESSEN_ABGELEHNT: u32 = 1;

/// Anwendungscode beim Schließen: eine neue Verbindung derselben Kennung hat
/// diese abgelöst.
pub const SCHLIESSEN_ERSETZT: u32 = 2;

/// Code, mit dem ein Strom abgebrochen wird (der Client sieht „zurückgesetzt").
pub const STROM_ABBRUCH: u32 = 1;

/// So lange darf ein Teilnehmer auf seinen Stromkopf warten lassen.
const KOPF_FRIST: Duration = Duration::from_secs(10);

/// Wie lange `abgleichen` höchstens schläft, wenn niemand anstößt.
const ABGLEICH_TAKT: Duration = Duration::from_secs(1);

/// Wie lange ein Strom höchstens auf Durchsatz wartet, bevor er doch schreibt.
/// Ein hängender Eimer darf keine Verbindung festhalten.
const DURCHSATZ_GEDULD: Duration = Duration::from_secs(3);

/// Was das Modul über die Sitzung wissen muss. Der Server implementiert es
/// (`crates/server/src/spielnetz.rs`); das Modul kennt nur diese Momentaufnahme.
pub trait Mitgliedschaft: Send + Sync + 'static {
    /// Momentaufnahme aller in der Sitzung (ALPN `sprechwerk/1`) Angemeldeten.
    fn stand(&self) -> Vec<Stand>;
}

/// Ein Angemeldeter, wie ihn das Modul braucht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stand {
    /// Kennung (Hex) — dieselbe wie in der Sitzung.
    pub kennung: String,
    /// Anzeigename.
    pub name: String,
    /// Raum als Text; `None` = kein Raum.
    pub raum: Option<String>,
    /// Darf das Modul benutzen (`Capability::Spielnetz`, ADR 0016 §8).
    pub darf: bool,
}

/// Was eine Verbindung zu tun hat. Über ein Postfach, damit die Schreibaufgabe
/// die einzige Stelle bleibt, die auf den Steuerstrom schreibt.
enum Anweisung {
    Rahmen(ServerRahmen),
    /// Letzte Anweisung: Grund schicken (falls vorhanden), dann schließen.
    Schliessen {
        code: u32,
        text: String,
        fehler: Option<Fehler>,
    },
}

/// Eine angemeldete Spielnetz-Verbindung.
struct Verbunden {
    raum: Option<String>,
    postfach: UnboundedSender<Anweisung>,
    conn: quinn::Connection,
}

struct Inner {
    vermittlung: Vermittlung,
    verbunden: BTreeMap<String, Verbunden>,
    /// Laufende Weiterleitungen: `(Angebot, Teilnehmer) → Aufgaben`.
    weiterleitungen: BTreeMap<(AngebotId, String), Vec<AbortHandle>>,
}

impl Inner {
    /// Bricht die Weiterleitungen eines Paares (oder eines ganzen Angebots) ab.
    fn trennen(&mut self, angebot: AngebotId, teilnehmer: Option<&str>) {
        let schluessel: Vec<(AngebotId, String)> = self
            .weiterleitungen
            .keys()
            .filter(|(a, t)| *a == angebot && teilnehmer.is_none_or(|w| w == t))
            .cloned()
            .collect();
        for k in schluessel {
            if let Some(aufgaben) = self.weiterleitungen.remove(&k) {
                for aufgabe in aufgaben {
                    aufgabe.abort();
                }
            }
        }
    }

    /// Schickt einen Rahmen an eine angemeldete Kennung (still, wenn sie weg ist).
    fn sende(&self, an: &str, rahmen: ServerRahmen) {
        if let Some(verbunden) = self.verbunden.get(an) {
            let _ = verbunden.postfach.send(Anweisung::Rahmen(rahmen));
        }
    }
}

/// Das Modul: Grenzen, Mitgliedschaft, Zustand.
pub struct Modul {
    grenzen: Grenzen,
    mitglieder: Arc<dyn Mitgliedschaft>,
    inner: Mutex<Inner>,
    /// „Es hat sich etwas geändert" — billig, ohne Sperre.
    wachen: Notify,
    /// Verworfene Datagramme und Ströme (für Protokoll und Diagnose).
    verworfen: AtomicU64,
}

impl Modul {
    /// Ein frisches Modul. `mitglieder` ist die Momentaufnahme des Servers.
    pub fn neu(grenzen: Grenzen, mitglieder: Arc<dyn Mitgliedschaft>) -> Arc<Self> {
        Arc::new(Self {
            grenzen,
            mitglieder,
            inner: Mutex::new(Inner {
                vermittlung: Vermittlung::neu(grenzen),
                verbunden: BTreeMap::new(),
                weiterleitungen: BTreeMap::new(),
            }),
            wachen: Notify::new(),
            verworfen: AtomicU64::new(0),
        })
    }

    /// Die Grenzen, die dieses Modul durchsetzt.
    pub fn grenzen(&self) -> Grenzen {
        self.grenzen
    }

    /// Vom Server nach jeder Zustandsänderung gerufen (Raumwechsel, Rolle,
    /// Verbindung weg). Billig, sperrt nichts, weckt nur [`Modul::abgleichen`].
    pub fn anstossen(&self) {
        self.wachen.notify_one();
    }

    /// Wie viele Datagramme und Ströme verworfen wurden.
    pub fn verworfen(&self) -> u64 {
        self.verworfen.load(Ordering::Relaxed)
    }

    /// Gleicht in einer Schleife ab: wartet auf [`Modul::anstossen`] oder 1 s.
    ///
    /// Der Takt ist die Rückfalllinie: geht ein Anstoß verloren, ist der Stand
    /// nach einer Sekunde trotzdem richtig.
    pub async fn abgleichen(self: Arc<Self>) {
        loop {
            let _ = tokio::time::timeout(ABGLEICH_TAKT, self.wachen.notified()).await;
            self.gleiche_ab();
        }
    }

    /// Einmaliger Abgleich: wer ist weg, wer darf nicht mehr, wer ist umgezogen.
    fn gleiche_ab(&self) {
        // **Erst fragen, dann sperren.** `stand()` sperrt den Kern des Servers;
        // die eigene Sperre darf dabei nicht gehalten werden.
        let stand = self.mitglieder.stand();
        let karte: BTreeMap<String, Stand> =
            stand.into_iter().map(|s| (s.kennung.clone(), s)).collect();

        let mut inner = self.inner.lock().expect("Sperre");

        let mut zu_schliessen: Vec<(String, Fehler)> = Vec::new();
        let mut raumwechsel: Vec<(String, Option<String>)> = Vec::new();
        for (kennung, verbunden) in inner.verbunden.iter() {
            match karte.get(kennung) {
                None => zu_schliessen.push((kennung.clone(), Fehler::NichtAngemeldet)),
                Some(s) if !s.darf => zu_schliessen.push((kennung.clone(), Fehler::KeinRecht)),
                Some(s) if s.raum != verbunden.raum => {
                    raumwechsel.push((kennung.clone(), s.raum.clone()));
                }
                Some(_) => {}
            }
        }

        for (kennung, code) in zu_schliessen {
            if let Some(verbunden) = inner.verbunden.remove(&kennung) {
                // Grund zuerst, Schließen danach — die Reihenfolge macht die
                // Schreibaufgabe, deshalb geht der Rahmen nicht verloren.
                let _ = verbunden.postfach.send(Anweisung::Schliessen {
                    code: SCHLIESSEN_ABGELEHNT,
                    text: code.text().to_string(),
                    fehler: Some(code),
                });
            }
            let aktionen = inner.vermittlung.verarbeite(Ereignis::Getrennt { kennung });
            ausfuehren(&mut inner, aktionen);
        }

        for (kennung, raum) in raumwechsel {
            tracing::debug!(%kennung, ?raum, "Spielnetz: Raumwechsel");
            let aktionen = inner
                .vermittlung
                .verarbeite(Ereignis::Raumwechsel { kennung, raum });
            ausfuehren(&mut inner, aktionen);
        }
    }

    /// Bedient eine Verbindung, deren ALPN `protokoll::ALPN` ist.
    pub async fn bedienen(self: Arc<Self>, conn: quinn::Connection) -> anyhow::Result<()> {
        let adresse = conn.remote_address().ip().to_string();

        // **Der Server öffnet den Steuerstrom** — wie in der Sitzung: ein
        // QUIC-Strom wird der Gegenstelle erst mit den ersten Bytes sichtbar.
        let (mut send, mut recv) = conn.open_bi().await?;

        // 1. Beweisaufforderung
        let nonce = zufall_hex(32);
        schreibe_rahmen(
            &mut send,
            &ServerRahmen::Anforderung {
                version: VERSION,
                nonce: nonce.clone(),
            },
        )
        .await?;

        // 2. Hallo
        let mut puffer = Vec::new();
        let hallo = lese::<ClientRahmen>(&mut recv, &mut puffer).await?;
        let Some(hallo) = hallo else {
            return Ok(());
        };
        let ClientRahmen::Hallo {
            version,
            kennung,
            beweis,
        } = hallo
        else {
            return self
                .abweisen(&conn, &mut send, Fehler::UngueltigeAnfrage)
                .await;
        };

        // 3. Prüfungen — in der Reihenfolge, in der ein Client sie versteht.
        if version != VERSION {
            return self.abweisen(&conn, &mut send, Fehler::Version).await;
        }
        if !sprechwerk_adapters::pruefe_beweis(&kennung, &nonce, &beweis) {
            tracing::warn!(%adresse, "Spielnetz: Beweis falsch");
            return self
                .abweisen(&conn, &mut send, Fehler::AnmeldungAbgelehnt)
                .await;
        }
        // Die Sitzung ist die einzige Wahrheit: ohne Anmeldung dort kein
        // Spielnetz, und ohne Recht (`Capability::Spielnetz`) auch nicht.
        let Some(stand) = self
            .mitglieder
            .stand()
            .into_iter()
            .find(|s| s.kennung == kennung)
        else {
            return self
                .abweisen(&conn, &mut send, Fehler::NichtAngemeldet)
                .await;
        };
        if !stand.darf {
            return self.abweisen(&conn, &mut send, Fehler::KeinRecht).await;
        }

        // 4. Schreibaufgabe: alles, was andere an uns schicken, geht hier hinaus.
        let (postfach, mut empfanger) = unbounded_channel::<Anweisung>();
        let schreib_conn = conn.clone();
        let mut schreib_send = send;
        let schreiber = tokio::spawn(async move {
            while let Some(anweisung) = empfanger.recv().await {
                match anweisung {
                    Anweisung::Rahmen(rahmen) => {
                        if schreibe_rahmen(&mut schreib_send, &rahmen).await.is_err() {
                            break;
                        }
                    }
                    Anweisung::Schliessen { code, text, fehler } => {
                        if let Some(code) = fehler {
                            let _ = schreibe_rahmen(&mut schreib_send, &fehler_rahmen(code)).await;
                        }
                        // `finish` gibt die Bytes frei; ohne den kurzen Halt
                        // könnte das Schließen den Grund überholen.
                        let _ = schreib_send.finish();
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        schreib_conn.close(VarInt::from(code), text.as_bytes());
                        break;
                    }
                }
            }
        });

        // 5. Eintragen. Eine vorhandene Verbindung derselben Kennung wird
        //    abgelöst: erst abmelden (ihre Freigaben enden), dann die neue
        //    anmelden.
        let eigene_id = conn.stable_id();
        {
            let mut inner = self.inner.lock().expect("Sperre");
            if let Some(alt) = inner.verbunden.remove(&kennung) {
                tracing::info!(%kennung, "Spielnetz: Verbindung ersetzt");
                let aktionen = inner.vermittlung.verarbeite(Ereignis::Getrennt {
                    kennung: kennung.clone(),
                });
                ausfuehren(&mut inner, aktionen);
                let _ = alt.postfach.send(Anweisung::Schliessen {
                    code: SCHLIESSEN_ERSETZT,
                    text: "durch eine neue Verbindung ersetzt".to_string(),
                    fehler: None,
                });
            }
            inner.verbunden.insert(
                kennung.clone(),
                Verbunden {
                    raum: stand.raum.clone(),
                    postfach: postfach.clone(),
                    conn: conn.clone(),
                },
            );
            tracing::info!(%kennung, name = %stand.name, %adresse, "Spielnetz angemeldet");
            let aktionen = inner.vermittlung.verarbeite(Ereignis::Verbunden {
                kennung: kennung.clone(),
                name: stand.name.clone(),
                raum: stand.raum.clone(),
                darf: stand.darf,
            });
            ausfuehren(&mut inner, aktionen);
        }

        // Ab hier darf der Teilnehmer so viele Ströme öffnen, wie die Grenze
        // erlaubt (je TCP-Verbindung einer).
        conn.set_max_concurrent_bi_streams(VarInt::from(self.grenzen.stroeme_je_teilnehmer));

        // 6. Leseschleife: Steuerrahmen, Ströme, Datagramme.
        let mut drossel = Drossel::neu(RAHMEN_JE_SEKUNDE);
        let ergebnis = loop {
            tokio::select! {
                gelesen = lese::<ClientRahmen>(&mut recv, &mut puffer) => {
                    match gelesen {
                        Ok(Some(rahmen)) => {
                            if !drossel.erlaubt(Instant::now()) {
                                tracing::warn!(%kennung, "Spielnetz: Rahmengrenze überschritten");
                                self.sprengen(&kennung, "zu viele Rahmen");
                                break Ok(());
                            }
                            let mut inner = self.inner.lock().expect("Sperre");
                            let aktionen = inner.vermittlung.verarbeite(Ereignis::Rahmen {
                                kennung: kennung.clone(),
                                rahmen,
                            });
                            ausfuehren(&mut inner, aktionen);
                        }
                        Ok(None) => break Ok(()),
                        Err(fehler) => break Err(fehler),
                    }
                }
                strom = conn.accept_bi() => {
                    match strom {
                        Ok((send, recv)) => {
                            // Ein Strom ist eine eigene Aufgabe: ein großes
                            // Spiel darf die Steuerung nicht aufhalten.
                            let modul = self.clone();
                            let kennung = kennung.clone();
                            tokio::spawn(async move {
                                if let Err(fehler) = modul.leite_strom(kennung.clone(), send, recv).await {
                                    tracing::debug!(%kennung, fehler = %fehler, "Spielnetz: Strom beendet");
                                }
                            });
                        }
                        Err(_) => break Ok(()),
                    }
                }
                datagramm = conn.read_datagram() => {
                    match datagramm {
                        Ok(d) => self.leite_datagramm(&kennung, &d),
                        Err(_) => break Ok(()),
                    }
                }
            }
        };

        self.abmelden(&kennung, eigene_id);
        schreiber.abort();
        ergebnis
    }

    /// Lehnt eine Anmeldung ab: Grund als Rahmen, dann zu.
    async fn abweisen(
        &self,
        conn: &quinn::Connection,
        send: &mut quinn::SendStream,
        code: Fehler,
    ) -> anyhow::Result<()> {
        let _ = schreibe_rahmen(send, &fehler_rahmen(code)).await;
        let _ = send.finish();
        // Der Grund soll noch hinausgehen, bevor der Handschlag endet.
        tokio::time::sleep(Duration::from_millis(20)).await;
        conn.close(VarInt::from(SCHLIESSEN_ABGELEHNT), code.text().as_bytes());
        Ok(())
    }

    /// Schließt eine Verbindung, weil sie sich daneben benommen hat.
    fn sprengen(&self, kennung: &str, text: &str) {
        let inner = self.inner.lock().expect("Sperre");
        if let Some(verbunden) = inner.verbunden.get(kennung) {
            let _ = verbunden.postfach.send(Anweisung::Schliessen {
                code: SCHLIESSEN_ABGELEHNT,
                text: text.to_string(),
                fehler: None,
            });
        }
    }

    /// Trägt eine Verbindung aus. Nur, wenn es **diese** ist: eine ersetzte
    /// Verbindung darf den Eintrag ihrer Nachfolgerin nicht löschen.
    fn abmelden(&self, kennung: &str, id: usize) {
        let mut inner = self.inner.lock().expect("Sperre");
        if !inner
            .verbunden
            .get(kennung)
            .is_some_and(|v| v.conn.stable_id() == id)
        {
            return;
        }
        inner.verbunden.remove(kennung);
        tracing::info!(%kennung, "Spielnetz abgemeldet");
        let aktionen = inner.vermittlung.verarbeite(Ereignis::Getrennt {
            kennung: kennung.to_string(),
        });
        ausfuehren(&mut inner, aktionen);
    }

    // ------------------------------------------------------------- TCP

    /// Ein getunnelter TCP-Strom eines Teilnehmers.
    async fn leite_strom(
        self: Arc<Self>,
        kennung: String,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
    ) -> anyhow::Result<()> {
        let mut puffer = Vec::new();
        let kopf: StromKopf =
            match tokio::time::timeout(KOPF_FRIST, lese::<StromKopf>(&mut recv, &mut puffer)).await
            {
                Ok(Ok(Some(kopf))) => kopf,
                Ok(Ok(None)) | Ok(Err(_)) | Err(_) => {
                    let _ = send.reset(VarInt::from(STROM_ABBRUCH));
                    let _ = recv.stop(VarInt::from(STROM_ABBRUCH));
                    return Ok(());
                }
            };
        // Was nach dem Kopf schon im Puffer liegt, ist der Anfang des
        // Geheimtextes — es darf nicht verloren gehen.
        let anfang = std::mem::take(&mut puffer);

        let (gastgeber, strom) = {
            let mut inner = self.inner.lock().expect("Sperre");
            match inner.vermittlung.strom_oeffnen(&kennung, kopf.angebot) {
                Ok(paar) => paar,
                Err(code) => {
                    inner.sende(&kennung, fehler_rahmen(code));
                    drop(inner);
                    let _ = send.reset(VarInt::from(STROM_ABBRUCH));
                    let _ = recv.stop(VarInt::from(STROM_ABBRUCH));
                    return Ok(());
                }
            }
        };

        let host_conn = {
            let inner = self.inner.lock().expect("Sperre");
            inner.verbunden.get(&gastgeber).map(|v| v.conn.clone())
        };
        let Some(host_conn) = host_conn else {
            // Der Gastgeber ist gerade weg — dann ist auch die Freigabe weg.
            let mut inner = self.inner.lock().expect("Sperre");
            inner.vermittlung.strom_geschlossen(&kennung, kopf.angebot);
            return Ok(());
        };

        let (mut host_send, host_recv) = match host_conn.open_bi().await {
            Ok(paar) => paar,
            Err(fehler) => {
                let mut inner = self.inner.lock().expect("Sperre");
                inner.vermittlung.strom_geschlossen(&kennung, kopf.angebot);
                let _ = send.reset(VarInt::from(STROM_ABBRUCH));
                let _ = recv.stop(VarInt::from(STROM_ABBRUCH));
                return Err(fehler.into());
            }
        };
        // Der Gastgeber erfährt am Kopf, von wem der Strom kommt.
        let kopf_hin = StromKopf {
            angebot: kopf.angebot,
            von: Some(kennung.clone()),
            strom,
        };
        schreibe_rahmen_roh(&mut host_send, &schreibe(&kopf_hin)?).await?;

        // Beide Richtungen laufen als eigene Aufgaben; ihre Abgriffe stehen in
        // der Tabelle, damit `Trennen` sie abbrechen kann.
        let mut hin = tokio::spawn(kopiere(self.clone(), kopf.angebot, recv, host_send, anfang));
        let mut her = tokio::spawn(kopiere(
            self.clone(),
            kopf.angebot,
            host_recv,
            send,
            Vec::new(),
        ));
        let eigene = [hin.abort_handle(), her.abort_handle()];
        {
            let mut inner = self.inner.lock().expect("Sperre");
            // Zwischen `strom_oeffnen` und hier lag ein `await`: wurde der
            // Teilnehmer inzwischen entfernt, hat `Trennen` diesen Strom nicht
            // gesehen. Dann hier abbrechen, statt ihn weiterlaufen zu lassen.
            if inner.vermittlung.ist_teilnehmer(kopf.angebot, &kennung) {
                inner
                    .weiterleitungen
                    .entry((kopf.angebot, kennung.clone()))
                    .or_default()
                    .extend(eigene.iter().cloned());
            } else {
                hin.abort();
                her.abort();
            }
        }
        tokio::select! {
            _ = &mut hin => her.abort(),
            _ = &mut her => hin.abort(),
        }

        {
            let mut inner = self.inner.lock().expect("Sperre");
            // Nur die **eigenen** Griffe austragen: andere Ströme desselben
            // Paares laufen weiter und müssen für `Trennen` greifbar bleiben.
            // Sonst behielte ein entfernter Teilnehmer seine übrigen Ströme.
            let schluessel = (kopf.angebot, kennung.clone());
            if let Some(griffe) = inner.weiterleitungen.get_mut(&schluessel) {
                griffe.retain(|g| !eigene.iter().any(|e| e.id() == g.id()));
                if griffe.is_empty() {
                    inner.weiterleitungen.remove(&schluessel);
                }
            }
            inner.vermittlung.strom_geschlossen(&kennung, kopf.angebot);
        }
        Ok(())
    }

    // ------------------------------------------------------------- UDP

    /// Ein getunneltes UDP-Datagramm: Gastgeber → Teilnehmer oder umgekehrt.
    ///
    /// Die Richtung entscheidet die Vermittlung, nicht der Kopf: steht die
    /// Kennung als Gastgeber im Angebot, geht es her, sonst hin. Ein
    /// umgeschriebener Kopf macht den Empfänger ratlos, nicht gläubig — gegen
    /// Fälschung schützt die Prüfsumme des Geheimtextes.
    fn leite_datagramm(&self, kennung: &str, datagramm: &[u8]) {
        let Some((kopf, nutzlast)) = Datagrammkopf::lesen(datagramm) else {
            self.zaehle_verworfen("Kopf unlesbar");
            return;
        };

        let mut inner = self.inner.lock().expect("Sperre");

        // 1. Vom Gastgeber zum Teilnehmer?
        if let Some((teilnehmer, kanal)) =
            inner
                .vermittlung
                .datagramm_her(kennung, kopf.angebot, kopf.kanal)
        {
            let Some(ziel) = inner.verbunden.get(&teilnehmer) else {
                drop(inner);
                self.zaehle_verworfen("Teilnehmer weg");
                return;
            };
            let conn = ziel.conn.clone();
            let bytes = Datagrammkopf {
                angebot: kopf.angebot,
                kanal,
            }
            .schreiben(nutzlast);
            if !inner
                .vermittlung
                .durchsatz(kopf.angebot, bytes.len(), Instant::now())
            {
                drop(inner);
                self.zaehle_verworfen("Durchsatz");
                return;
            }
            drop(inner);
            self.sende_datagramm(&conn, bytes, "an Teilnehmer");
            return;
        }

        // 2. Vom Teilnehmer zum Gastgeber (ggf. mit neuer Kanalmeldung).
        let Some((gastgeber, kanal, aktion)) =
            inner
                .vermittlung
                .datagramm_hin(kennung, kopf.angebot, kopf.kanal)
        else {
            drop(inner);
            self.zaehle_verworfen("kein Paar");
            return;
        };
        if let Some(aktion) = aktion {
            ausfuehren(&mut inner, vec![aktion]);
        }
        let Some(ziel) = inner.verbunden.get(&gastgeber) else {
            drop(inner);
            self.zaehle_verworfen("Gastgeber weg");
            return;
        };
        let conn = ziel.conn.clone();
        let bytes = Datagrammkopf {
            angebot: kopf.angebot,
            kanal,
        }
        .schreiben(nutzlast);
        if !inner
            .vermittlung
            .durchsatz(kopf.angebot, bytes.len(), Instant::now())
        {
            drop(inner);
            self.zaehle_verworfen("Durchsatz");
            return;
        }
        drop(inner);
        self.sende_datagramm(&conn, bytes, "an Gastgeber");
    }

    fn sende_datagramm(&self, conn: &quinn::Connection, bytes: Vec<u8>, wohin: &'static str) {
        if let Some(grenze) = conn.max_datagram_size() {
            if bytes.len() > grenze {
                self.zaehle_verworfen("zu groß");
                return;
            }
        } else {
            self.zaehle_verworfen("keine Datagramme");
            return;
        }
        if let Err(fehler) = conn.send_datagram(Bytes::from(bytes)) {
            tracing::debug!(fehler = %fehler, wohin, "Spielnetz: Datagramm nicht gesendet");
            self.zaehle_verworfen("Sendepuffer");
        }
    }

    fn zaehle_verworfen(&self, grund: &'static str) {
        let gesamt = self.verworfen.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::debug!(grund, gesamt, "Spielnetz: Datagramm verworfen");
    }
}

/// Kopiert eine Richtung eines Tunnels. Der Inhalt ist Geheimtext — er wird
/// nicht angesehen, nur weitergegeben.
async fn kopiere(
    modul: Arc<Modul>,
    angebot: AngebotId,
    mut quelle: quinn::RecvStream,
    mut ziel: quinn::SendStream,
    anfang: Vec<u8>,
) -> anyhow::Result<()> {
    if !anfang.is_empty() {
        ziel.write_all(&anfang).await?;
    }
    let mut puffer = vec![0u8; 16 * 1024];
    loop {
        let n = match quelle.read(&mut puffer).await? {
            None | Some(0) => break,
            Some(n) => n,
        };
        // Durchsatz der Freigabe. Ist der Eimer leer, wird **gewartet**: ein
        // TCP-Strom darf keine Bytes verlieren, nur langsamer werden.
        warte_auf_durchsatz(&modul, angebot, n).await;
        ziel.write_all(&puffer[..n]).await?;
    }
    let _ = ziel.finish();
    Ok(())
}

/// Wartet, bis der Token-Eimer der Freigabe `bytes` hergibt.
async fn warte_auf_durchsatz(modul: &Modul, angebot: AngebotId, bytes: usize) {
    let beginn = Instant::now();
    loop {
        let platz = {
            let mut inner = modul.inner.lock().expect("Sperre");
            inner.vermittlung.durchsatz(angebot, bytes, Instant::now())
        };
        if platz {
            return;
        }
        if beginn.elapsed() >= DURCHSATZ_GEDULD {
            // Lieber zu schnell als hängend: eine stockende Freigabe darf keine
            // Verbindung festhalten. Der Eimer füllt sich gleich wieder.
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Führt die Aktionen der Vermittlung aus. Die Sperre wird nur hier gehalten,
/// und nie über ein `await`.
fn ausfuehren(inner: &mut Inner, aktionen: Vec<Aktion>) {
    for aktion in aktionen {
        match aktion {
            Aktion::Senden { an, rahmen } => {
                if let Some(verbunden) = inner.verbunden.get(&an) {
                    let _ = verbunden.postfach.send(Anweisung::Rahmen(rahmen));
                }
            }
            Aktion::Trennen {
                angebot,
                teilnehmer,
            } => inner.trennen(angebot, teilnehmer.as_deref()),
        }
    }
}

/// Zufall als Hexadezimaltext (die Nonce der Beweisaufforderung).
fn zufall_hex(anzahl: usize) -> String {
    let mut roh = vec![0u8; anzahl];
    getrandom::getrandom(&mut roh).expect("Zufall aus dem Betriebssystem");
    hex::encode(roh)
}

/// Schreibt einen Steuerrahmen auf einen Strom.
async fn schreibe_rahmen(
    ziel: &mut quinn::SendStream,
    rahmen: &ServerRahmen,
) -> anyhow::Result<()> {
    schreibe_rahmen_roh(ziel, &schreibe(rahmen)?).await
}

/// Schreibt fertige Bytes auf einen Strom.
async fn schreibe_rahmen_roh(ziel: &mut quinn::SendStream, bytes: &[u8]) -> anyhow::Result<()> {
    ziel.write_all(bytes).await?;
    ziel.flush().await?;
    Ok(())
}

/// Liest genau einen Rahmen (wartet, bis er vollständig ist).
async fn lese<T: DeserializeOwned>(
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

/// Ein Fehlerrahmen mit dem stabilen Text seines Codes.
fn fehler_rahmen(code: Fehler) -> ServerRahmen {
    ServerRahmen::Fehler {
        code,
        text: code.text().to_string(),
    }
}

/// Eine Sende-Drossel: höchstens `grenze` Steuerrahmen je Sekunde.
struct Drossel {
    grenze: u32,
    fenster: Instant,
    gezaehlt: u32,
}

impl Drossel {
    fn neu(grenze: u32) -> Self {
        Self {
            grenze,
            fenster: Instant::now(),
            gezaehlt: 0,
        }
    }

    fn erlaubt(&mut self, jetzt: Instant) -> bool {
        if self.grenze == 0 {
            return true;
        }
        if jetzt.saturating_duration_since(self.fenster) >= Duration::from_secs(1) {
            self.fenster = jetzt;
            self.gezaehlt = 0;
        }
        if self.gezaehlt < self.grenze {
            self.gezaehlt += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Grenze gilt je Sekunde — und öffnet sich danach wieder (dieselbe
    /// Regel wie im Sitzungscode, `crates/server/src/netz.rs`).
    #[test]
    fn rahmendrossel_gilt_je_sekunde() {
        let start = Instant::now();
        let mut d = Drossel::neu(2);
        assert!(d.erlaubt(start));
        assert!(d.erlaubt(start + Duration::from_millis(10)));
        assert!(
            !d.erlaubt(start + Duration::from_millis(20)),
            "die dritte nicht"
        );
        assert!(
            !d.erlaubt(start + Duration::from_millis(900)),
            "still bleibt zu"
        );
        assert!(
            d.erlaubt(start + Duration::from_millis(1_001)),
            "nach einer Sekunde geht wieder etwas durch"
        );
    }

    /// 0 heißt unbegrenzt (für Prüfungen und Notfälle).
    #[test]
    fn null_ist_unbegrenzt() {
        let start = Instant::now();
        let mut d = Drossel::neu(0);
        for i in 0..1_000 {
            assert!(d.erlaubt(start + Duration::from_micros(i)));
        }
    }

    /// Der Fehlerrahmen trägt den stabilen Text seines Codes — der Client zeigt
    /// ihn an, ohne die Zahl zu kennen.
    #[test]
    fn fehlerrahmen_hat_den_stabilen_text() {
        match fehler_rahmen(Fehler::KeinRecht) {
            ServerRahmen::Fehler { code, text } => {
                assert_eq!(code, Fehler::KeinRecht);
                assert_eq!(text, Fehler::KeinRecht.text());
                assert_eq!(text, "dafür fehlt das Recht");
            }
            anderes => panic!("unerwartet: {anderes:?}"),
        }
    }

    /// Die Nonce ist 32 Byte Zufall in Hex — nicht zweimal dieselbe.
    #[test]
    fn nonce_ist_zufall_in_hex() {
        let a = zufall_hex(32);
        let b = zufall_hex(32);
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}

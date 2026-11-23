//! Die Clientseite, die aus einer [`Steuerung`] echte Spielverbindungen macht.
//!
//! [`crate::client`] baut die zweite QUIC-Verbindung auf und meldet sich an —
//! mehr nicht. Hier kommt der Teil, der einen Spieler etwas angeht:
//!
//! * **Gastgeber** ([`Tunnel::anbieten`]) — nimmt Ströme und Datagramme vom
//!   Server an und reicht sie an `127.0.0.1:<port>` weiter.
//! * **Teilnehmer** ([`Tunnel::beitreten`]) — öffnet erst nach `Beigetreten`
//!   einen lokalen Port und spiegelt ihn auf die Freigabe.
//!
//! # Eine Aufgabe besitzt die Steuerung
//!
//! Alles, was den Steuerstrom anfasst, läuft in **einer** Aufgabe: sie liest die
//! Rahmen, schreibt die Befehle aus einem Postfach und liest die Datagramme.
//! Nur so gibt es genau eine Leseaufgabe je Verbindung (ADR 0016 §5) — zwei
//! Leser würden sich die Datagramme streitig machen.
//!
//! Was die Aufgabe nebenbei führt, ist die [`Lage`]: eine Momentaufnahme für die
//! Oberfläche, verteilt über `watch`. Sie ist **reine Daten** (serde), damit
//! `crates/fenster` sie anzeigen kann, ohne dieses Modul zu kennen.
//!
//! Zu jeder Freigabe und jeder Teilnahme gehören Aufgaben (Ströme, Kanäle,
//! lokale Zuhörer). Ihre [`tokio::task::JoinHandle`] liegen im gemeinsamen
//! Zustand; `Beendet`, [`Tunnel::zurueckziehen`], [`Tunnel::verlassen`],
//! [`Tunnel::entfernen`] und [`Tunnel::beenden`] brechen sie ab. Wird ein
//! Tunnel fallen gelassen, bricht [`Drop`] ab — ohne `await`, dafür sofort.
//!
//! # Der Rand bleibt der Rand
//!
//! Der Gastgeber verbindet **nur** `127.0.0.1:<port>` ([`ziel`]), der Teilnehmer
//! bindet **nur** `127.0.0.1` (ADR 0016 §9). Das Modul ist kein Weg in ein
//! Heimnetz — in keines von beiden.
//!
//! # Grenzen, die aus dem Protokoll folgen
//!
//! * **Halbes Schließen geht durch.** Schließt ein Spiel nur seine
//!   Schreibseite, kommt das als Ende des Stroms beim Gegenüber an, und die
//!   andere Richtung läuft weiter, bis auch sie endet (Client: `pumpe`,
//!   Server: `leite_strom`).
//! * **UDP bleibt unzuverlässig.** Über [`MAX_UDP_NUTZLAST`] hinaus wird
//!   verworfen und gezählt, nie zerteilt (ADR 0016 §7).

use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use bytes::Bytes;
use quinn::VarInt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use sprechwerk_adapters::Schluessel;

use crate::client::{anmelden, Anmeldefehler, Steuerung};
use crate::krypto::{
    strom_salz, Datagrammoeffner, Datagrammsiegel, Paar, Rolle, Stromoeffner, Stromsiegel,
    STROM_SALZ_LAENGE,
};
use crate::protokoll::{
    schreibe, Angebot, AngebotId, Art, ClientRahmen, Datagrammkopf, Grenzen, Kanal, ServerRahmen,
    StromKopf, MAX_UDP_NUTZLAST,
};

/// So lange darf eine Antwort des Servers auf einen Befehl auf sich warten
/// lassen. Danach gilt der Befehl als verloren — nicht als gelungen.
const ANTWORT_FRIST: Duration = Duration::from_secs(10);

/// So lange darf der Aufbau eines getunnelten Stroms dauern (Kopf und Salze).
const AUFBAU_FRIST: Duration = Duration::from_secs(10);

/// Größe eines Leseblocks beim Pumpen. Entspricht dem Block eines
/// [`Stromsiegel`]s (`krypto::STROM_BLOCK`), damit jeder Block genau eine
/// Nachricht ergibt.
const BLOCK: usize = 16 * 1024;

/// So viele Datagramme werden gepuffert, die vor ihrem `Kanal`-Rahmen kommen.
const PUFFER_MAX: usize = 32;

/// So lange bleibt ein gepuffertes Datagramm gültig.
const PUFFER_FRIST: Duration = Duration::from_secs(2);

/// Höchstzahl der lokalen Ports, die nach dem Wunschport probiert werden.
const PORTVERSUCHE: u16 = 20;

/// Was die Oberfläche anzeigt. Reine Daten — kein Griff, keine Sperre.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lage {
    /// Freigaben im eigenen Raum, wie der Server sie zuletzt geschickt hat.
    pub angebote: Vec<Angebot>,
    /// Meine eigenen Freigaben.
    pub eigene: Vec<AngebotId>,
    /// Freigaben, denen ich beigetreten bin, mit ihrem lokalen Port.
    pub teilnahmen: Vec<Teilnahme>,
    /// Letzter Fehler oder Grund, in Worten für den Menschen.
    pub meldung: Option<String>,
}

/// Eine Teilnahme: welche Freigabe und unter welcher lokalen Adresse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Teilnahme {
    pub angebot: AngebotId,
    pub lokal: SocketAddr,
}

/// Die lokale Adresse eines Gastgeber-Ports.
///
/// **Immer** `127.0.0.1` (ADR 0016 §9): das Modul ist kein Weg in ein Heimnetz.
/// Eine reine Funktion, damit die Regel prüfbar ist, ohne dass ein Tunnel
/// entsteht.
pub fn ziel(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

// ---------------------------------------------------------------- Tunnel

/// Ein laufendes Spielnetz auf der Clientseite.
///
/// Wird der Griff fallen gelassen, endet alles: die Aufgaben werden abgebrochen
/// und die Verbindung geschlossen ([`Drop`]). [`Tunnel::beenden`] macht
/// dasselbe mit der Gelegenheit, die Aufgaben noch abzuwarten.
pub struct Tunnel {
    befehle: mpsc::UnboundedSender<Befehl>,
    verbindung: quinn::Connection,
    ctx: Kontext,
    haupt: Option<tokio::task::AbortHandle>,
}

impl Tunnel {
    /// Meldet sich an und startet die Aufgaben.
    ///
    /// `adresse` ist der UDP-Port des Servers, `fingerabdruck` das angeheftete
    /// Zertifikat aus der Sitzung (TOFU, ADR 0002 §3).
    pub async fn starten(
        adresse: SocketAddr,
        schluessel: Schluessel,
        fingerabdruck: &str,
    ) -> Result<(Tunnel, watch::Receiver<Lage>), Anmeldefehler> {
        let (steuerung, grenzen) = anmelden(adresse, &schluessel, fingerabdruck).await?;
        let verbindung = steuerung.conn.clone();

        let (befehl_sender, befehl_empfaenger) = mpsc::unbounded_channel();
        let (aufbau_sender, aufbau_empfaenger) = mpsc::unbounded_channel();
        let (lage_sender, lage_empfaenger) = watch::channel(Lage::default());

        let ctx = Kontext {
            zustand: Arc::new(Mutex::new(Zustand::neu())),
            lage: lage_sender,
            conn: verbindung.clone(),
            schluessel,
            grenzen,
            aufbau: aufbau_sender,
        };
        let haupt = tokio::spawn(hauptaufgabe(
            steuerung,
            befehl_empfaenger,
            aufbau_empfaenger,
            ctx.clone(),
        ));

        Ok((
            Tunnel {
                befehle: befehl_sender,
                verbindung,
                ctx,
                haupt: Some(haupt.abort_handle()),
            },
            lage_empfaenger,
        ))
    }

    /// Gibt einen Port des eigenen Rechners für alle im Raum frei.
    pub async fn anbieten(&self, name: &str, port: u16, art: Art) -> anyhow::Result<AngebotId> {
        let (antwort, empfangen) = oneshot::channel();
        self.schicke(Befehl::Anbieten {
            name: name.to_string(),
            port,
            art,
            antwort,
        })?;
        warte(empfangen, "Freigeben").await
    }

    /// Nimmt die eigene Freigabe zurück; ihre Ströme und Kanäle enden sofort.
    pub async fn zurueckziehen(&self, angebot: AngebotId) -> anyhow::Result<()> {
        let (antwort, empfangen) = oneshot::channel();
        self.schicke(Befehl::Zurueckziehen { angebot, antwort })?;
        warte(empfangen, "Zurückziehen").await
    }

    /// Tritt einer Freigabe bei und öffnet den lokalen Port **nach** der
    /// Zusage des Servers.
    ///
    /// Der lokale Port ist der der Freigabe; ist er belegt, der nächste freie
    /// (ADR 0016 §9). Gebunden wird ausschließlich `127.0.0.1`.
    pub async fn beitreten(&self, angebot: AngebotId) -> anyhow::Result<SocketAddr> {
        let (antwort, empfangen) = oneshot::channel();
        self.schicke(Befehl::Beitreten { angebot, antwort })?;
        warte(empfangen, "Beitreten").await
    }

    /// Verlässt eine Freigabe; der lokale Port ist danach zu.
    pub async fn verlassen(&self, angebot: AngebotId) -> anyhow::Result<()> {
        let (antwort, empfangen) = oneshot::channel();
        self.schicke(Befehl::Verlassen { angebot, antwort })?;
        warte(empfangen, "Verlassen").await
    }

    /// Wirft einen Teilnehmer aus der eigenen Freigabe (nur als Gastgeber).
    pub async fn entfernen(&self, angebot: AngebotId, kennung: &str) -> anyhow::Result<()> {
        let (antwort, empfangen) = oneshot::channel();
        self.schicke(Befehl::Entfernen {
            angebot,
            kennung: kennung.to_string(),
            antwort,
        })?;
        warte(empfangen, "Entfernen").await
    }

    /// Beendet alles und wartet, bis die Aufgaben wirklich stehen.
    pub async fn beenden(mut self) {
        if let Some(haupt) = self.haupt.take() {
            haupt.abort();
        }
        self.ctx.raeume_alles_auf().await;
        self.verbindung
            .close(VarInt::from(0u32), b"Spielnetz beendet");
        // Der Grund soll noch hinausgehen, bevor der Griff verschwindet.
        let _ = tokio::time::timeout(Duration::from_millis(200), self.verbindung.closed()).await;
    }

    fn schicke(&self, befehl: Befehl) -> anyhow::Result<()> {
        self.befehle
            .send(befehl)
            .map_err(|_| anyhow::anyhow!("das Spielnetz ist beendet"))
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        // Kein `await` in `Drop`: abbrechen, aufräumen, zumachen. Die Aufgaben
        // halten ihre Sockets selbst und geben sie beim Abbruch frei.
        if let Some(haupt) = self.haupt.take() {
            haupt.abort();
        }
        self.ctx.hart_abbrechen();
        self.verbindung
            .close(VarInt::from(0u32), b"Spielnetz beendet");
    }
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tunnel").finish_non_exhaustive()
    }
}

/// Wartet auf die Antwort einer Aufgabe, mit Frist.
async fn warte<T>(empfangen: oneshot::Receiver<anyhow::Result<T>>, was: &str) -> anyhow::Result<T> {
    match tokio::time::timeout(ANTWORT_FRIST, empfangen).await {
        Ok(Ok(ergebnis)) => ergebnis,
        Ok(Err(_)) => Err(anyhow::anyhow!("{was}: keine Antwort")),
        Err(_) => Err(anyhow::anyhow!("{was}: der Server antwortet nicht")),
    }
}

// ---------------------------------------------------------------- Befehle

/// Ein Auftrag der Oberfläche an die Aufgabe, die die Steuerung besitzt.
enum Befehl {
    Anbieten {
        name: String,
        port: u16,
        art: Art,
        antwort: oneshot::Sender<anyhow::Result<AngebotId>>,
    },
    Zurueckziehen {
        angebot: AngebotId,
        antwort: oneshot::Sender<anyhow::Result<()>>,
    },
    Beitreten {
        angebot: AngebotId,
        antwort: oneshot::Sender<anyhow::Result<SocketAddr>>,
    },
    Verlassen {
        angebot: AngebotId,
        antwort: oneshot::Sender<anyhow::Result<()>>,
    },
    Entfernen {
        angebot: AngebotId,
        kennung: String,
        antwort: oneshot::Sender<anyhow::Result<()>>,
    },
}

/// Ein Befehl, dessen Antwort noch aussteht.
///
/// `Angeboten` und `Beigetreten` tragen keine Nummer, an der man sie einem
/// Auftrag zuordnen könnte — also wartet der älteste passende Auftrag.
enum Ausstehend {
    Anbieten {
        port: u16,
        art: Art,
        antwort: oneshot::Sender<anyhow::Result<AngebotId>>,
    },
    Beitreten {
        angebot: AngebotId,
        antwort: oneshot::Sender<anyhow::Result<SocketAddr>>,
    },
}

impl Ausstehend {
    fn beantworte(self, fehler: String) {
        match self {
            Ausstehend::Anbieten { antwort, .. } => {
                let _ = antwort.send(Err(anyhow::anyhow!(fehler)));
            }
            Ausstehend::Beitreten { antwort, .. } => {
                let _ = antwort.send(Err(anyhow::anyhow!(fehler)));
            }
        }
    }
}

// ---------------------------------------------------------------- Zustand

/// Eine eigene Freigabe, wie sie der Server bestätigt hat.
struct Freigabe {
    port: u16,
    art: Art,
}

/// Ein laufendes Siegel/Öffner-Paar für die Kanäle **eines** Teilnehmers.
struct UdpPaar {
    siegel: Datagrammsiegel,
    oeffner: Datagrammoeffner,
}

/// Ein Kanal beim Gastgeber: der Socket zum lokalen Spiel und wem er gehört.
struct HostKanal {
    socket: Arc<UdpSocket>,
    von: String,
}

/// Die UDP-Seite einer Teilnahme.
struct GastUdp {
    socket: Arc<UdpSocket>,
    siegel: Datagrammsiegel,
    oeffner: Datagrammoeffner,
    kanaele: Kanaele,
}

/// Eine Aufgabe, die zu einer Freigabe oder einem Paar gehört.
struct Aufgabe {
    angebot: AngebotId,
    von: Option<String>,
    griff: JoinHandle<()>,
}

/// Alles, was die Aufgaben gemeinsam brauchen. Die Sperre wird **nie** über ein
/// `await` gehalten.
struct Zustand {
    /// Die Angebotsliste des Raums, zuletzt vom Server.
    angebote: Vec<Angebot>,
    eigene: BTreeMap<AngebotId, Freigabe>,
    teilnahmen: BTreeMap<AngebotId, Teilnahme>,
    aufgaben: Vec<Aufgabe>,
    /// Je (Freigabe, Teilnehmer) ein Paar — die Kanäle teilen es sich.
    host_udp: BTreeMap<(AngebotId, String), UdpPaar>,
    /// Je (Freigabe, Kanal beim Gastgeber) der Socket zum lokalen Spiel.
    host_kanaele: BTreeMap<(AngebotId, Kanal), HostKanal>,
    /// Die eigene UDP-Seite je Teilnahme.
    gast_udp: BTreeMap<AngebotId, GastUdp>,
    /// Datagramme, die vor ihrem `Kanal`-Rahmen angekommen sind.
    puffer: Puffer,
    meldung: Option<String>,
    /// Verworfene Datagramme (zu groß, kein Kanal, kein Paar).
    verworfen: u64,
}

impl Zustand {
    fn neu() -> Self {
        Self {
            angebote: Vec::new(),
            eigene: BTreeMap::new(),
            teilnahmen: BTreeMap::new(),
            aufgaben: Vec::new(),
            host_udp: BTreeMap::new(),
            host_kanaele: BTreeMap::new(),
            gast_udp: BTreeMap::new(),
            puffer: Puffer::neu(),
            meldung: None,
            verworfen: 0,
        }
    }

    /// Die Momentaufnahme für die Oberfläche.
    fn lage(&self) -> Lage {
        Lage {
            angebote: self.angebote.clone(),
            eigene: self.eigene.keys().copied().collect(),
            teilnahmen: self.teilnahmen.values().copied().collect(),
            meldung: self.meldung.clone(),
        }
    }

    /// Ein Datagramm vom Server an das lokale Spiel des Gastgebers.
    ///
    /// `false` heißt: dieser Kanal ist (noch) nicht bekannt — dann gehört das
    /// Datagramm in den [`Puffer`]. Ein bekannter Kanal mit unbekanntem Paar
    /// wird verworfen, aber nicht gepuffert: da kommt kein Rahmen mehr.
    fn host_datagramm(&mut self, angebot: AngebotId, kanal: Kanal, nutzlast: &[u8]) -> bool {
        let Some(eintrag) = self.host_kanaele.get(&(angebot, kanal)) else {
            return false;
        };
        let socket = eintrag.socket.clone();
        let von = eintrag.von.clone();
        if let Some(paar) = self.host_udp.get_mut(&(angebot, von)) {
            if let Ok(klar) = paar.oeffner.oeffnen(nutzlast) {
                // `try_send`: ein voller Puffer kostet ein Datagramm, nicht den
                // Steuerstrom (UDP ist unzuverlässig, so ist es abgesprochen).
                let _ = socket.try_send(&klar);
            }
        }
        true
    }

    /// Ein Datagramm vom Server an den lokalen Socket eines Teilnehmers.
    fn teilnehmer_datagramm(&mut self, angebot: AngebotId, kanal: Kanal, nutzlast: &[u8]) {
        let Some(gast) = self.gast_udp.get_mut(&angebot) else {
            return;
        };
        let Some(quelle) = gast.kanaele.quelle(kanal) else {
            return;
        };
        let socket = gast.socket.clone();
        if let Ok(klar) = gast.oeffner.oeffnen(nutzlast) {
            let _ = socket.try_send_to(&klar, quelle);
        }
    }
}

// ---------------------------------------------------------------- Kontext

/// Was eine Aufgabe über ihren Auftrag hinaus braucht.
#[derive(Clone)]
struct Kontext {
    zustand: Arc<Mutex<Zustand>>,
    lage: watch::Sender<Lage>,
    conn: quinn::Connection,
    schluessel: Schluessel,
    grenzen: Grenzen,
    /// Vorbereitete Gastgeber-Ströme, die die Hauptaufgabe in eine Aufgabe gibt.
    aufbau: mpsc::UnboundedSender<Aufbau>,
}

impl Kontext {
    /// Veröffentlicht den Zustand als [`Lage`].
    fn veroeffentliche(&self) {
        let lage = self.zustand.lock().expect("Sperre").lage();
        self.lage.send_replace(lage);
    }

    /// Setzt die Meldung für den Menschen und veröffentlicht sie.
    fn merke(&self, text: impl Into<String>) {
        let lage = {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.meldung = Some(text.into());
            zustand.lage()
        };
        self.lage.send_replace(lage);
    }

    /// Trägt eine Aufgabe ein, damit sie abgebrochen werden kann.
    fn merke_aufgabe(&self, angebot: AngebotId, von: Option<String>, griff: JoinHandle<()>) {
        let mut zustand = self.zustand.lock().expect("Sperre");
        zustand.aufgaben.retain(|a| !a.griff.is_finished());
        zustand.aufgaben.push(Aufgabe {
            angebot,
            von,
            griff,
        });
    }

    /// Zählt ein zu großes Datagramm und erwähnt es **einmal** in der Meldung.
    fn zu_gross(&self) {
        let (erstes, lage) = {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.verworfen = zustand.verworfen.saturating_add(1);
            let erstes = zustand.verworfen == 1;
            if erstes {
                zustand.meldung = Some(format!("Datagramm über {MAX_UDP_NUTZLAST} Byte verworfen"));
            }
            (erstes, zustand.lage())
        };
        if erstes {
            self.lage.send_replace(lage);
        }
    }

    /// Ein Datagramm vom Server: nach Angebot und Rolle verteilen.
    fn verteile(&self, datagramm: &[u8]) {
        let Some((kopf, nutzlast)) = Datagrammkopf::lesen(datagramm) else {
            return;
        };
        let mut zustand = self.zustand.lock().expect("Sperre");
        // Die Rolle entscheidet das eigene Verzeichnis, nicht der Kopf: ein
        // umgeschriebener Kopf macht ratlos, nicht gläubig.
        let eigene_udp = zustand
            .eigene
            .get(&kopf.angebot)
            .is_some_and(|f| f.art == Art::Udp);
        if eigene_udp {
            if !zustand.host_datagramm(kopf.angebot, kopf.kanal, nutzlast) {
                // Der `Kanal`-Rahmen ist noch unterwegs (eigene Ströme und
                // Datagramme haben keine gemeinsame Reihenfolge).
                zustand
                    .puffer
                    .lege(kopf.angebot, kopf.kanal, nutzlast.to_vec(), Instant::now());
            }
        } else {
            zustand.teilnehmer_datagramm(kopf.angebot, kopf.kanal, nutzlast);
        }
    }

    /// Bricht alle Aufgaben ab, die zur Bedingung passen, und wartet sie ab.
    async fn brich_ab(&self, betroffen: impl Fn(&Aufgabe) -> bool) {
        let weg: Vec<Aufgabe> = {
            let mut zustand = self.zustand.lock().expect("Sperre");
            let (weg, bleibt) = std::mem::take(&mut zustand.aufgaben)
                .into_iter()
                .partition(|a| betroffen(a));
            zustand.aufgaben = bleibt;
            weg
        };
        for aufgabe in weg {
            aufgabe.griff.abort();
            let _ = aufgabe.griff.await;
        }
    }

    /// Beendet eine Freigabe oder Teilnahme samt allem, was daran hängt.
    async fn raeume_auf(&self, angebot: AngebotId) {
        self.brich_ab(|a| a.angebot == angebot).await;
        {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.eigene.remove(&angebot);
            zustand.teilnahmen.remove(&angebot);
            zustand.host_udp.retain(|(a, _), _| *a != angebot);
            zustand.host_kanaele.retain(|(a, _), _| *a != angebot);
            zustand.gast_udp.remove(&angebot);
            zustand.puffer.entferne(angebot);
        }
        self.veroeffentliche();
    }

    /// Beendet die Kanäle und Ströme **eines** Teilnehmers beim Gastgeber.
    async fn raeume_paar_auf(&self, angebot: AngebotId, von: &str) {
        self.brich_ab(|a| a.angebot == angebot && a.von.as_deref() == Some(von))
            .await;
        {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.host_udp.remove(&(angebot, von.to_string()));
            zustand
                .host_kanaele
                .retain(|(a, _), kanal| !(*a == angebot && kanal.von == von));
        }
        self.veroeffentliche();
    }

    /// Alles aus: Aufgaben abbrechen, Freigaben und Teilnahmen austragen.
    async fn raeume_alles_auf(&self) {
        self.brich_ab(|_| true).await;
        {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.eigene.clear();
            zustand.teilnahmen.clear();
            zustand.host_udp.clear();
            zustand.host_kanaele.clear();
            zustand.gast_udp.clear();
            zustand.puffer.leeren();
        }
        self.veroeffentliche();
    }

    /// Dasselbe ohne `await` — für [`Drop`].
    fn hart_abbrechen(&self) {
        let mut zustand = self.zustand.lock().expect("Sperre");
        for aufgabe in zustand.aufgaben.drain(..) {
            aufgabe.griff.abort();
        }
        zustand.eigene.clear();
        zustand.teilnahmen.clear();
        zustand.host_udp.clear();
        zustand.host_kanaele.clear();
        zustand.gast_udp.clear();
        zustand.puffer.leeren();
    }

    /// Bereitet einen vom Server geöffneten Strom vor und gibt ihn weiter.
    ///
    /// Diese Aufgabe steht bewusst in keiner Liste: sie hält den Strom nur bis
    /// zum Stromkopf und den Salzen ([`AUFBAU_FRIST`]) und endet danach von
    /// selbst. Erst was pumpt, gehört zu einem Angebot und wird abbrechbar.
    fn stelle_strom(&self, send: quinn::SendStream, recv: quinn::RecvStream) {
        let ctx = self.clone();
        tokio::spawn(async move {
            match aufbau_vorbereiten(&ctx, send, recv).await {
                Ok(aufbau) => {
                    // Ist die Hauptaufgabe weg, wird der Aufbau verworfen — dann
                    // endet auch die Verbindung.
                    let _ = ctx.aufbau.send(aufbau);
                }
                Err(fehler) => {
                    tracing::debug!(fehler = %fehler, "Spielnetz: Gastgeberstrom verworfen");
                }
            }
        });
    }

    /// Startet das Pumpen für einen vorbereiteten Gastgeberstrom.
    fn startet_strom(&self, aufbau: Aufbau) {
        let Aufbau {
            angebot,
            von,
            lokal,
            send,
            recv,
            siegel,
            oeffner,
            anfang,
        } = aufbau;
        let griff = tokio::spawn(async move {
            if let Err(fehler) = pumpe(lokal, send, recv, siegel, oeffner, anfang).await {
                tracing::debug!(fehler = %fehler, "Spielnetz: Strom beendet");
            }
        });
        self.merke_aufgabe(angebot, Some(von), griff);
    }

    /// Ein `Kanal`-Rahmen: dieser Kanal beim Gastgeber gehört zu `von`.
    async fn gastgeber_kanal(&self, angebot: AngebotId, kanal: Kanal, von: String) {
        let port = {
            let zustand = self.zustand.lock().expect("Sperre");
            match zustand.eigene.get(&angebot) {
                Some(freigabe) if freigabe.art == Art::Udp => freigabe.port,
                // Ein Kanal zu einer Freigabe, die es nicht (mehr) gibt, ist
                // entweder alt oder falsch — beides kein Grund zu handeln.
                _ => return,
            }
        };
        let Some(paar) = Paar::neu(&self.schluessel.austausch_geheimnis(), &von, angebot) else {
            return;
        };

        // Nur 127.0.0.1, nur der freigegebene Port (ADR 0016 §9).
        let socket = match UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await {
            Ok(socket) => socket,
            Err(fehler) => {
                self.merke(format!("lokaler UDP-Socket: {fehler}"));
                return;
            }
        };
        if let Err(fehler) = socket.connect(ziel(port)).await {
            self.merke(format!("Spiel auf 127.0.0.1:{port}: {fehler}"));
            return;
        }
        let socket = Arc::new(socket);

        {
            let mut zustand = self.zustand.lock().expect("Sperre");
            // Ein Paar je (Angebot, Teilnehmer): alle seine Kanäle teilen sich
            // Siegel und Öffner, sonst zählten sie getrennt (krypto §„Ein Paar,
            // viele Kanäle“).
            zustand
                .host_udp
                .entry((angebot, von.clone()))
                .or_insert_with(|| {
                    let (siegel, oeffner) = paar.datagramme(Rolle::Gastgeber);
                    UdpPaar { siegel, oeffner }
                });
            zustand.host_kanaele.insert(
                (angebot, kanal),
                HostKanal {
                    socket: socket.clone(),
                    von: von.clone(),
                },
            );
        }

        // Lokales Spiel → Siegel → Server.
        let ctx = self.clone();
        let schluessel = (angebot, von.clone());
        let griff = tokio::spawn(async move {
            let mut block = vec![0u8; MAX_UDP_NUTZLAST + 1];
            loop {
                let Ok(n) = socket.recv(&mut block).await else {
                    break;
                };
                if n > MAX_UDP_NUTZLAST {
                    ctx.zu_gross();
                    continue;
                }
                let geheim = {
                    let mut zustand = ctx.zustand.lock().expect("Sperre");
                    match zustand.host_udp.get_mut(&schluessel) {
                        Some(paar) => paar.siegel.versiegeln(&block[..n]),
                        None => break,
                    }
                };
                let bytes = Datagrammkopf { angebot, kanal }.schreiben(&geheim);
                match ctx.conn.send_datagram_wait(Bytes::from(bytes)).await {
                    Ok(()) => {}
                    // Ein zu großes Datagramm ist ein verlorenes Datagramm,
                    // kein Grund, den Kanal zu schließen.
                    Err(quinn::SendDatagramError::TooLarge) => ctx.zu_gross(),
                    Err(_) => break,
                }
            }
        });
        self.merke_aufgabe(angebot, Some(von), griff);

        // Was vor dem Rahmen kam, jetzt nachholen.
        let lage = {
            let mut zustand = self.zustand.lock().expect("Sperre");
            for nutzlast in zustand.puffer.nehmen(angebot, kanal) {
                zustand.host_datagramm(angebot, kanal, &nutzlast);
            }
            zustand.lage()
        };
        self.lage.send_replace(lage);
    }

    /// Öffnet den lokalen Port einer Teilnahme und startet ihre Aufgaben.
    async fn teilnahme_oeffnen(&self, angebot: AngebotId) -> anyhow::Result<SocketAddr> {
        let freigabe = {
            let zustand = self.zustand.lock().expect("Sperre");
            zustand
                .angebote
                .iter()
                .find(|a| a.id == angebot)
                .cloned()
                .context("die Freigabe steht nicht (mehr) in der Liste")?
        };
        // Doppeltes Beitreten ist kein Fehler: die alte Teilnahme gilt.
        if let Some(teilnahme) = self
            .zustand
            .lock()
            .expect("Sperre")
            .teilnahmen
            .get(&angebot)
            .copied()
        {
            return Ok(teilnahme.lokal);
        }

        match freigabe.art {
            Art::Tcp => self.teilnahme_tcp(angebot, &freigabe).await,
            Art::Udp => self.teilnahme_udp(angebot, &freigabe).await,
        }
    }

    /// TCP-Teilnahme: ein Zuhörer auf `127.0.0.1`, je Verbindung ein Strom.
    async fn teilnahme_tcp(
        &self,
        angebot: AngebotId,
        freigabe: &Angebot,
    ) -> anyhow::Result<SocketAddr> {
        let zuhoerer = binde_tcp(freigabe.port).await?;
        let lokal = zuhoerer.local_addr()?;
        let gastgeber = freigabe.gastgeber.clone();

        let ctx = self.clone();
        let griff = tokio::spawn(async move {
            loop {
                match zuhoerer.accept().await {
                    Ok((strom, _adresse)) => {
                        let conn = ctx.conn.clone();
                        let schluessel = ctx.schluessel.clone();
                        let gastgeber = gastgeber.clone();
                        let unter = tokio::spawn(async move {
                            if let Err(fehler) =
                                teilnehmer_strom(conn, strom, angebot, gastgeber, schluessel).await
                            {
                                tracing::debug!(fehler = %fehler, "Spielnetz: Tunnel beendet");
                            }
                        });
                        let mut zustand = ctx.zustand.lock().expect("Sperre");
                        zustand.aufgaben.retain(|a| !a.griff.is_finished());
                        zustand.aufgaben.push(Aufgabe {
                            angebot,
                            von: None,
                            griff: unter,
                        });
                    }
                    Err(fehler) => {
                        tracing::debug!(fehler = %fehler, "Spielnetz: lokaler Zuhörer beendet");
                        break;
                    }
                }
            }
        });
        self.merke_aufgabe(angebot, None, griff);

        self.trage_teilnahme_ein(Teilnahme { angebot, lokal });
        Ok(lokal)
    }

    /// UDP-Teilnahme: ein Socket auf `127.0.0.1`, je Quelle ein eigener Kanal.
    async fn teilnahme_udp(
        &self,
        angebot: AngebotId,
        freigabe: &Angebot,
    ) -> anyhow::Result<SocketAddr> {
        let socket = Arc::new(binde_udp(freigabe.port).await?);
        let lokal = socket.local_addr()?;
        let Some(paar) = Paar::neu(
            &self.schluessel.austausch_geheimnis(),
            &freigabe.gastgeber,
            angebot,
        ) else {
            anyhow::bail!("die Kennung des Gastgebers ist keine gültige Kennung");
        };
        let (siegel, oeffner) = paar.datagramme(Rolle::Teilnehmer);

        {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.gast_udp.insert(
                angebot,
                GastUdp {
                    socket: socket.clone(),
                    siegel,
                    oeffner,
                    kanaele: Kanaele::neu(self.grenzen.stroeme_je_teilnehmer),
                },
            );
        }

        let ctx = self.clone();
        let griff = tokio::spawn(async move {
            let mut block = vec![0u8; 65535];
            loop {
                let Ok((n, quelle)) = socket.recv_from(&mut block).await else {
                    break;
                };
                if n > MAX_UDP_NUTZLAST {
                    ctx.zu_gross();
                    continue;
                }
                let (kanal, geheim) = {
                    let mut zustand = ctx.zustand.lock().expect("Sperre");
                    let Some(gast) = zustand.gast_udp.get_mut(&angebot) else {
                        break;
                    };
                    // Grenze erreicht: dieses eine Datagramm fällt weg, der
                    // Kanal bleibt der älteren Quelle.
                    let Some(kanal) = gast.kanaele.kanal(quelle) else {
                        continue;
                    };
                    (kanal, gast.siegel.versiegeln(&block[..n]))
                };
                let bytes = Datagrammkopf { angebot, kanal }.schreiben(&geheim);
                match ctx.conn.send_datagram_wait(Bytes::from(bytes)).await {
                    Ok(()) => {}
                    // Ein zu großes Datagramm ist ein verlorenes Datagramm,
                    // kein Grund, den Kanal zu schließen.
                    Err(quinn::SendDatagramError::TooLarge) => ctx.zu_gross(),
                    Err(_) => break,
                }
            }
        });
        self.merke_aufgabe(angebot, None, griff);

        self.trage_teilnahme_ein(Teilnahme { angebot, lokal });
        Ok(lokal)
    }

    fn trage_teilnahme_ein(&self, teilnahme: Teilnahme) {
        let lage = {
            let mut zustand = self.zustand.lock().expect("Sperre");
            zustand.meldung = None;
            zustand.teilnahmen.insert(teilnahme.angebot, teilnahme);
            zustand.lage()
        };
        self.lage.send_replace(lage);
    }
}

/// Ein vorbereiteter Gastgeberstrom: alles gelesen, was zum Rechnen nötig war.
struct Aufbau {
    angebot: AngebotId,
    von: String,
    lokal: TcpStream,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    siegel: Stromsiegel,
    oeffner: Stromoeffner,
    /// Bytes, die beim Lesen von Kopf und Salz schon mitgekommen sind.
    anfang: Vec<u8>,
}

// ---------------------------------------------------------------- Aufgaben

/// Die Aufgabe, die die [`Steuerung`] besitzt.
async fn hauptaufgabe(
    mut steuerung: Steuerung,
    mut befehle: mpsc::UnboundedReceiver<Befehl>,
    mut aufbauten: mpsc::UnboundedReceiver<Aufbau>,
    ctx: Kontext,
) {
    let conn = ctx.conn.clone();
    let mut ausstehend: VecDeque<Ausstehend> = VecDeque::new();

    loop {
        tokio::select! {
            befehl = befehle.recv() => {
                let Some(befehl) = befehl else { break };
                if let Err(fehler) = fuehre_aus(&ctx, &mut steuerung, befehl, &mut ausstehend).await {
                    ctx.merke(fehler.to_string());
                    break;
                }
            }
            empfangen = steuerung.empfang() => {
                let Ok(Some(rahmen)) = empfangen else { break };
                if let Err(fehler) =
                    bearbeite_rahmen(&ctx, &mut steuerung, rahmen, &mut ausstehend).await
                {
                    ctx.merke(fehler.to_string());
                    break;
                }
            }
            datagramm = conn.read_datagram() => {
                let Ok(datagramm) = datagramm else { break };
                ctx.verteile(&datagramm);
            }
            strom = conn.accept_bi() => {
                let Ok((send, recv)) = strom else { break };
                ctx.stelle_strom(send, recv);
            }
            vorbereitet = aufbauten.recv() => {
                let Some(aufbau) = vorbereitet else { break };
                ctx.startet_strom(aufbau);
            }
        }
    }

    // Die Verbindung ist weg (oder wir sind fertig): nichts läuft weiter.
    ctx.raeume_alles_auf().await;
    ctx.merke("Spielnetz: Verbindung beendet");
    conn.close(VarInt::from(0u32), b"Spielnetz beendet");
}

/// Führt einen Befehl der Oberfläche aus: Rahmen schreiben, lokal aufräumen.
async fn fuehre_aus(
    ctx: &Kontext,
    steuerung: &mut Steuerung,
    befehl: Befehl,
    ausstehend: &mut VecDeque<Ausstehend>,
) -> anyhow::Result<()> {
    match befehl {
        Befehl::Anbieten {
            name,
            port,
            art,
            antwort,
        } => {
            steuerung
                .sende(ClientRahmen::Anbieten { name, port, art })
                .await?;
            ausstehend.push_back(Ausstehend::Anbieten { port, art, antwort });
        }
        Befehl::Zurueckziehen { angebot, antwort } => {
            steuerung
                .sende(ClientRahmen::Zurueckziehen { angebot })
                .await?;
            ctx.raeume_auf(angebot).await;
            let _ = antwort.send(Ok(()));
        }
        Befehl::Beitreten { angebot, antwort } => {
            steuerung.sende(ClientRahmen::Beitreten { angebot }).await?;
            ausstehend.push_back(Ausstehend::Beitreten { angebot, antwort });
        }
        Befehl::Verlassen { angebot, antwort } => {
            steuerung.sende(ClientRahmen::Verlassen { angebot }).await?;
            ctx.raeume_auf(angebot).await;
            let _ = antwort.send(Ok(()));
        }
        Befehl::Entfernen {
            angebot,
            kennung,
            antwort,
        } => {
            steuerung
                .sende(ClientRahmen::Entfernen {
                    angebot,
                    kennung: kennung.clone(),
                })
                .await?;
            ctx.raeume_paar_auf(angebot, &kennung).await;
            let _ = antwort.send(Ok(()));
        }
    }
    Ok(())
}

/// Verarbeitet einen Rahmen des Servers.
async fn bearbeite_rahmen(
    ctx: &Kontext,
    steuerung: &mut Steuerung,
    rahmen: ServerRahmen,
    ausstehend: &mut VecDeque<Ausstehend>,
) -> anyhow::Result<()> {
    match rahmen {
        ServerRahmen::Angebote { liste } => {
            {
                let mut zustand = ctx.zustand.lock().expect("Sperre");
                zustand.angebote = liste;
            }
            ctx.veroeffentliche();
        }
        ServerRahmen::Angeboten { angebot } => {
            let Some(Ausstehend::Anbieten { port, art, antwort }) =
                nimm(ausstehend, |a| matches!(a, Ausstehend::Anbieten { .. }))
            else {
                ctx.merke("unerwartete Antwort auf ein Freigeben");
                return Ok(());
            };
            let lage = {
                let mut zustand = ctx.zustand.lock().expect("Sperre");
                zustand.meldung = None;
                zustand.eigene.insert(angebot, Freigabe { port, art });
                zustand.lage()
            };
            ctx.lage.send_replace(lage);
            let _ = antwort.send(Ok(angebot));
        }
        ServerRahmen::Beigetreten { angebot } => {
            let Some(Ausstehend::Beitreten { angebot, antwort }) = nimm(
                ausstehend,
                |a| matches!(a, Ausstehend::Beitreten { angebot: gewuenscht, .. } if *gewuenscht == angebot),
            ) else {
                ctx.merke("unerwartete Antwort auf ein Beitreten");
                return Ok(());
            };
            match ctx.teilnahme_oeffnen(angebot).await {
                Ok(lokal) => {
                    let _ = antwort.send(Ok(lokal));
                }
                Err(fehler) => {
                    // Der Server hält uns für dabei; sag es ihm, sonst bliebe
                    // die Teilnahme dort stehen.
                    let _ = steuerung.sende(ClientRahmen::Verlassen { angebot }).await;
                    ctx.merke(fehler.to_string());
                    let _ = antwort.send(Err(fehler));
                }
            }
        }
        ServerRahmen::Kanal {
            angebot,
            kanal,
            von,
        } => ctx.gastgeber_kanal(angebot, kanal, von).await,
        ServerRahmen::Beendet { angebot, grund } => {
            ctx.raeume_auf(angebot).await;
            ctx.merke(grund.text());
        }
        ServerRahmen::Fehler { code, text } => {
            let text = if text.is_empty() {
                code.text().to_string()
            } else {
                text
            };
            match ausstehend.pop_front() {
                Some(offen) => offen.beantworte(text),
                // Ohne Bezug: das war eine Antwort auf etwas, das wir schon
                // vergessen haben (oder auf ein Zurückziehen/Verlassen).
                None => ctx.merke(text),
            }
        }
        // Kommt vor dem `Willkommen` nicht mehr, und die Anmeldung selbst macht
        // `client::anmelden`. Also: nichts zu tun.
        ServerRahmen::Anforderung { .. } | ServerRahmen::Willkommen { .. } => {}
    }
    Ok(())
}

/// Nimmt den ältesten Auftrag, auf den die Bedingung passt.
fn nimm(
    ausstehend: &mut VecDeque<Ausstehend>,
    bedingung: impl Fn(&Ausstehend) -> bool,
) -> Option<Ausstehend> {
    let stelle = ausstehend.iter().position(bedingung)?;
    ausstehend.remove(stelle)
}

// ------------------------------------------------------------ Gastgeber-TCP

/// Prüft einen vom Server geöffneten Strom und baut ihn auf.
///
/// Der Server schickt zuerst den [`StromKopf`]; er muss zu einer **eigenen**
/// TCP-Freigabe gehören und einen Absender nennen — sonst gibt es niemanden,
/// mit dem man rechnen könnte.
async fn aufbau_vorbereiten(
    ctx: &Kontext,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) -> anyhow::Result<Aufbau> {
    let mut puffer = Vec::new();
    let kopf = match tokio::time::timeout(
        AUFBAU_FRIST,
        lese_rahmen::<StromKopf>(&mut recv, &mut puffer),
    )
    .await
    {
        Ok(ergebnis) => ergebnis?.context("der Strom endet vor dem Kopf")?,
        Err(_) => {
            let _ = send.reset(VarInt::from(1u32));
            let _ = recv.stop(VarInt::from(1u32));
            anyhow::bail!("kein Stromkopf innerhalb von {AUFBAU_FRIST:?}");
        }
    };

    let port = {
        let zustand = ctx.zustand.lock().expect("Sperre");
        match zustand.eigene.get(&kopf.angebot) {
            Some(freigabe) if freigabe.art == Art::Tcp => freigabe.port,
            _ => {
                let _ = send.reset(VarInt::from(1u32));
                let _ = recv.stop(VarInt::from(1u32));
                anyhow::bail!("Strom zu keiner eigenen TCP-Freigabe ({})", kopf.angebot);
            }
        }
    };
    let Some(von) = kopf.von.clone() else {
        let _ = send.reset(VarInt::from(1u32));
        let _ = recv.stop(VarInt::from(1u32));
        anyhow::bail!("Strom ohne Absender");
    };
    let Some(paar) = Paar::neu(&ctx.schluessel.austausch_geheimnis(), &von, kopf.angebot) else {
        let _ = send.reset(VarInt::from(1u32));
        let _ = recv.stop(VarInt::from(1u32));
        anyhow::bail!("Kennung des Teilnehmers ist keine Kennung");
    };

    // Erst das Salz des Teilnehmers, dann das eigene schicken: beide gehen in
    // die Ableitung ein (krypto §„TCP: beide Salze“).
    let salz_t = lese_salz(&mut recv, &mut puffer).await?;
    let salz_g = strom_salz();
    send.write_all(&salz_g).await?;
    send.flush().await?;

    let (siegel, oeffner) = paar.strom(&salz_t, &salz_g, Rolle::Gastgeber);

    // Nur der eigene Rechner, nur der freigegebene Port (ADR 0016 §9).
    let lokal = TcpStream::connect(ziel(port))
        .await
        .with_context(|| format!("Spiel auf 127.0.0.1:{port}"))?;

    Ok(Aufbau {
        angebot: kopf.angebot,
        von,
        lokal,
        send,
        recv,
        siegel,
        oeffner,
        anfang: puffer,
    })
}

// ----------------------------------------------------------- Teilnehmer-TCP

/// Baut einen getunnelten Strom zu einer Freigabe auf und pumpt ihn.
async fn teilnehmer_strom(
    conn: quinn::Connection,
    lokal: TcpStream,
    angebot: AngebotId,
    gastgeber: String,
    schluessel: Schluessel,
) -> anyhow::Result<()> {
    let Some(paar) = Paar::neu(&schluessel.austausch_geheimnis(), &gastgeber, angebot) else {
        anyhow::bail!("die Kennung des Gastgebers ist keine gültige Kennung");
    };

    let (mut send, mut recv) = conn.open_bi().await?;
    // Kopf und Salz in einem Zug: ein Strom wird der Gegenstelle erst mit den
    // ersten Bytes sichtbar (`open_bi`).
    send.write_all(&schreibe(&StromKopf {
        angebot,
        von: None,
        strom: 0,
    })?)
    .await?;
    let salz_t = strom_salz();
    send.write_all(&salz_t).await?;
    send.flush().await?;

    let mut puffer = Vec::new();
    let salz_g = tokio::time::timeout(AUFBAU_FRIST, lese_salz(&mut recv, &mut puffer))
        .await
        .context("das Salz des Gastgebers lässt auf sich warten")??;
    let (siegel, oeffner) = paar.strom(&salz_t, &salz_g, Rolle::Teilnehmer);

    // Erst jetzt Nutzdaten: der Schlüssel hängt an beiden Salzen.
    pumpe(lokal, send, recv, siegel, oeffner, puffer).await
}

// ------------------------------------------------------------------ Pumpen

/// Pumpt einen Strom in beide Richtungen, bis eine Seite zu ist.
async fn pumpe(
    lokal: TcpStream,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    mut siegel: Stromsiegel,
    mut oeffner: Stromoeffner,
    anfang: Vec<u8>,
) -> anyhow::Result<()> {
    let (mut lesen, mut schreiben) = lokal.into_split();

    // Lokal → Siegel → Strom.
    let hin = async {
        let mut block = vec![0u8; BLOCK];
        loop {
            match lesen.read(&mut block).await {
                Ok(0) => {
                    let _ = send.finish();
                    return Ok::<(), ()>(());
                }
                Ok(n) => {
                    let geheim = siegel.versiegeln(&block[..n]);
                    if send.write_all(&geheim).await.is_err() {
                        return Err(());
                    }
                }
                Err(_) => return Err(()),
            }
        }
    };

    // Strom → Öffner → lokal.
    let her = async {
        let mut block = vec![0u8; BLOCK];
        let mut erste = anfang;
        loop {
            let eingang = if erste.is_empty() {
                match recv.read(&mut block).await {
                    Ok(Some(n)) if n > 0 => &block[..n],
                    // Zu Ende (oder abgebrochen): die Gegenstelle bekommt ihr
                    // Ende der Leitung.
                    _ => {
                        let _ = schreiben.shutdown().await;
                        return Ok::<(), ()>(());
                    }
                }
            } else {
                erste.as_slice()
            };
            match oeffner.oeffnen(eingang) {
                Ok(klar) => {
                    if !klar.is_empty() && schreiben.write_all(&klar).await.is_err() {
                        return Err(());
                    }
                }
                // Kryptofehler: beide Seiten zu, es gibt nichts mehr zu sagen.
                Err(_) => return Err(()),
            }
            if !erste.is_empty() {
                erste.clear();
            }
        }
    };

    let (a, b) = tokio::join!(hin, her);
    if a.is_err() || b.is_err() {
        let _ = send.reset(VarInt::from(1u32));
        let _ = recv.stop(VarInt::from(1u32));
        anyhow::bail!("Tunnel abgebrochen");
    }
    Ok(())
}

// ------------------------------------------------------------- Lesehelfer

/// Liest genau einen Rahmen (wartet, bis er vollständig ist).
async fn lese_rahmen<T: serde::de::DeserializeOwned>(
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

/// Liest ein Salz von 16 Byte; was danach schon da ist, bleibt im Puffer.
async fn lese_salz(
    quelle: &mut quinn::RecvStream,
    puffer: &mut Vec<u8>,
) -> anyhow::Result<[u8; STROM_SALZ_LAENGE]> {
    while puffer.len() < STROM_SALZ_LAENGE {
        let mut block = [0u8; 1024];
        match quelle.read(&mut block).await? {
            None | Some(0) => anyhow::bail!("der Strom endet vor dem Salz"),
            Some(n) => puffer.extend_from_slice(&block[..n]),
        }
    }
    let salz: Vec<u8> = puffer.drain(..STROM_SALZ_LAENGE).collect();
    salz.try_into()
        .map_err(|_| anyhow::anyhow!("Salz hat die falsche Länge"))
}

// ---------------------------------------------------------------- Ports

/// Die Reihenfolge der lokalen Ports, die probiert werden.
///
/// Wunschport zuerst, dann die nächsten [`PORTVERSUCHE`], zuletzt `0` („nimm
/// einen freien“). Doppelte fallen weg — bei 65535 kann es keine „nächsten“
/// geben, und `0` darf nie in der Mitte stehen.
fn portkandidaten(wunsch: u16) -> Vec<u16> {
    let mut kandidaten: Vec<u16> = Vec::with_capacity(usize::from(PORTVERSUCHE) + 2);
    for schritt in 0..=PORTVERSUCHE {
        let port = wunsch.saturating_add(schritt);
        if port != 0 && !kandidaten.contains(&port) {
            kandidaten.push(port);
        }
    }
    kandidaten.push(0);
    kandidaten
}

/// Bindet einen TCP-Zuhörer auf `127.0.0.1` — nur dort (ADR 0016 §9).
async fn binde_tcp(wunsch: u16) -> anyhow::Result<TcpListener> {
    let mut letzter: Option<std::io::Error> = None;
    for port in portkandidaten(wunsch) {
        match std::net::TcpListener::bind(ziel(port)) {
            Ok(zuhoerer) => {
                zuhoerer.set_nonblocking(true)?;
                return Ok(TcpListener::from_std(zuhoerer)?);
            }
            Err(fehler) => letzter = Some(fehler),
        }
    }
    Err(anyhow::Error::from(letzter.expect("mindestens ein Port")))
        .with_context(|| format!("kein freier lokaler TCP-Port ab {wunsch}"))
}

/// Bindet einen UDP-Socket auf `127.0.0.1` — nur dort (ADR 0016 §9).
async fn binde_udp(wunsch: u16) -> anyhow::Result<UdpSocket> {
    let mut letzter: Option<std::io::Error> = None;
    for port in portkandidaten(wunsch) {
        match UdpSocket::bind(ziel(port)).await {
            Ok(socket) => return Ok(socket),
            Err(fehler) => letzter = Some(fehler),
        }
    }
    Err(anyhow::Error::from(letzter.expect("mindestens ein Port")))
        .with_context(|| format!("kein freier lokaler UDP-Port ab {wunsch}"))
}

// ---------------------------------------------------------------- Kanäle

/// Kanalvergabe eines Teilnehmers: je Quelladresse ein eigener Kanal.
///
/// Ein Spiel öffnet oft mehrere UDP-Sockets; ohne eigene Kanäle landeten alle
/// Antworten beim ersten (ADR 0016 §5). Die Grenze ist dieselbe wie für
/// TCP-Ströme ([`Grenzen::stroeme_je_teilnehmer`]).
struct Kanaele {
    grenze: Kanal,
    naechster: Kanal,
    quellen: BTreeMap<SocketAddr, Kanal>,
}

impl Kanaele {
    fn neu(grenze: u8) -> Self {
        Self {
            grenze: Kanal::from(grenze),
            naechster: 1,
            quellen: BTreeMap::new(),
        }
    }

    /// Der Kanal dieser Quelle; `None`, wenn die Grenze erreicht ist.
    fn kanal(&mut self, quelle: SocketAddr) -> Option<Kanal> {
        if let Some(kanal) = self.quellen.get(&quelle) {
            return Some(*kanal);
        }
        if self.naechster > self.grenze {
            return None;
        }
        let kanal = self.naechster;
        self.naechster += 1;
        self.quellen.insert(quelle, kanal);
        Some(kanal)
    }

    /// Die Quelle eines Kanals (für Antworten vom Server).
    fn quelle(&self, kanal: Kanal) -> Option<SocketAddr> {
        self.quellen
            .iter()
            .find(|(_, k)| **k == kanal)
            .map(|(quelle, _)| *quelle)
    }
}

// ---------------------------------------------------------------- Puffer

/// Datagramme, die vor ihrem `Kanal`-Rahmen angekommen sind.
///
/// Der Server schickt den Rahmen zuerst, aber Ströme und Datagramme haben keine
/// gemeinsame Reihenfolge. Also warten ein paar Datagramme kurz — höchstens
/// [`PUFFER_MAX`] Stück und höchstens [`PUFFER_FRIST`] lang, dann sind sie weg
/// (UDP ist unzuverlässig, so ist es abgesprochen).
struct Puffer {
    eintraege: VecDeque<(Instant, AngebotId, Kanal, Vec<u8>)>,
}

impl Puffer {
    fn neu() -> Self {
        Self {
            eintraege: VecDeque::new(),
        }
    }

    fn lege(&mut self, angebot: AngebotId, kanal: Kanal, nutzlast: Vec<u8>, jetzt: Instant) {
        self.raeume(jetzt);
        if self.eintraege.len() >= PUFFER_MAX {
            self.eintraege.pop_front();
        }
        self.eintraege.push_back((jetzt, angebot, kanal, nutzlast));
    }

    /// Nimmt alle Datagramme dieses Kanals heraus (älteste zuerst).
    fn nehmen(&mut self, angebot: AngebotId, kanal: Kanal) -> Vec<Vec<u8>> {
        let mut passend = Vec::new();
        let mut bleibt = VecDeque::with_capacity(self.eintraege.len());
        while let Some((zeit, a, k, nutzlast)) = self.eintraege.pop_front() {
            if a == angebot && k == kanal {
                passend.push(nutzlast);
            } else {
                bleibt.push_back((zeit, a, k, nutzlast));
            }
        }
        self.eintraege = bleibt;
        passend
    }

    /// Wirft alles zu einer Freigabe weg (sie ist vorbei).
    fn entferne(&mut self, angebot: AngebotId) {
        self.eintraege.retain(|(_, a, _, _)| *a != angebot);
    }

    fn leeren(&mut self) {
        self.eintraege.clear();
    }

    /// Wirft weg, was zu alt ist.
    fn raeume(&mut self, jetzt: Instant) {
        while let Some((zeit, ..)) = self.eintraege.front() {
            if jetzt.saturating_duration_since(*zeit) >= PUFFER_FRIST {
                self.eintraege.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Gastgeber verbindet **nur** den eigenen Rechner (ADR 0016 §9).
    #[test]
    fn ziel_ist_immer_localhost() {
        for port in [1u16, 80, 25565, 65535] {
            let adresse = ziel(port);
            assert_eq!(adresse.port(), port);
            assert_eq!(adresse.ip(), std::net::IpAddr::from(Ipv4Addr::LOCALHOST));
            assert!(adresse.ip().is_loopback(), "nur die Schleife");
        }
    }

    #[test]
    fn portkandidaten_nehmen_den_wunsch_zuerst() {
        let kandidaten = portkandidaten(25565);
        assert_eq!(kandidaten[0], 25565);
        assert_eq!(kandidaten[1], 25566);
        assert_eq!(*kandidaten.last().expect("letzter"), 0);
        assert_eq!(
            kandidaten.len(),
            usize::from(PORTVERSUCHE) + 2,
            "Wunsch, die nächsten 20 und die freie Wahl"
        );
        let ohne_null: Vec<u16> = kandidaten.iter().copied().filter(|p| *p != 0).collect();
        let mut sortiert = ohne_null.clone();
        sortiert.dedup();
        assert_eq!(sortiert, ohne_null, "keine doppelten Ports");
    }

    #[test]
    fn portkandidaten_am_ende_der_skala() {
        // Bei 65535 gibt es keine „nächsten“ — dann bleibt nur die freie Wahl.
        assert_eq!(portkandidaten(65535), vec![65535, 0]);
        // 0 als Wunsch heißt: nimm irgendeinen. Er steht genau einmal da, am
        // Ende, und wird nicht als Kandidat wiederholt.
        assert_eq!(portkandidaten(0), (1..=20).chain([0]).collect::<Vec<u16>>());
    }

    #[test]
    fn kanaele_geben_jeder_quelle_einen_eigenen() {
        let a: SocketAddr = "127.0.0.1:5000".parse().expect("Adresse");
        let b: SocketAddr = "127.0.0.1:5001".parse().expect("Adresse");
        let mut kanaele = Kanaele::neu(32);
        assert_eq!(kanaele.kanal(a), Some(1));
        assert_eq!(kanaele.kanal(a), Some(1), "dieselbe Quelle, derselbe Kanal");
        assert_eq!(kanaele.kanal(b), Some(2));
        assert_eq!(kanaele.quelle(2), Some(b));
        assert_eq!(kanaele.quelle(1), Some(a));
        assert_eq!(kanaele.quelle(3), None);
    }

    #[test]
    fn kanaele_halten_die_grenze_ein() {
        let mut kanaele = Kanaele::neu(2);
        let a: SocketAddr = "127.0.0.1:5000".parse().expect("Adresse");
        let b: SocketAddr = "127.0.0.1:5001".parse().expect("Adresse");
        let c: SocketAddr = "127.0.0.1:5002".parse().expect("Adresse");
        assert_eq!(kanaele.kanal(a), Some(1));
        assert_eq!(kanaele.kanal(b), Some(2));
        assert_eq!(kanaele.kanal(c), None, "Grenze erreicht");
        // Die bekannten Quellen behalten ihren Kanal.
        assert_eq!(kanaele.kanal(a), Some(1));
    }

    #[test]
    fn kanaele_ohne_grenze_geben_keinen_kanal() {
        let mut kanaele = Kanaele::neu(0);
        let a: SocketAddr = "127.0.0.1:5000".parse().expect("Adresse");
        assert_eq!(kanaele.kanal(a), None);
    }

    #[test]
    fn puffer_gibt_nach_nach_kanal_zurueck() {
        let jetzt = Instant::now();
        let mut puffer = Puffer::neu();
        puffer.lege(1, 7, b"eins".to_vec(), jetzt);
        puffer.lege(1, 8, b"zwei".to_vec(), jetzt);
        puffer.lege(2, 7, b"drei".to_vec(), jetzt);
        assert_eq!(puffer.nehmen(1, 7), vec![b"eins".to_vec()]);
        assert_eq!(puffer.eintraege.len(), 2, "der Rest bleibt");
        assert!(puffer.nehmen(1, 7).is_empty(), "nichts doppelt");
    }

    #[test]
    fn puffer_haelt_hoechstens_zweiunddreissig() {
        let jetzt = Instant::now();
        let mut puffer = Puffer::neu();
        for i in 0..(PUFFER_MAX + 5) {
            puffer.lege(1, 1, vec![i as u8], jetzt);
        }
        assert_eq!(puffer.eintraege.len(), PUFFER_MAX);
        // Die ältesten sind weg, der jüngste ist da.
        let genommen = puffer.nehmen(1, 1);
        assert_eq!(genommen.len(), PUFFER_MAX);
        assert_eq!(
            genommen.last().expect("letzter"),
            &vec![(PUFFER_MAX + 4) as u8]
        );
    }

    #[test]
    fn puffer_verwirft_alte_datagramme() {
        let jetzt = Instant::now();
        let mut puffer = Puffer::neu();
        puffer.lege(1, 1, b"alt".to_vec(), jetzt);
        puffer.lege(
            1,
            1,
            b"neu".to_vec(),
            jetzt + PUFFER_FRIST + Duration::from_millis(1),
        );
        // Das alte ist beim Ablegen des neuen weggefallen.
        assert_eq!(puffer.nehmen(1, 1), vec![b"neu".to_vec()]);
    }

    #[test]
    fn puffer_entfernt_eine_freigabe() {
        let jetzt = Instant::now();
        let mut puffer = Puffer::neu();
        puffer.lege(1, 1, b"eins".to_vec(), jetzt);
        puffer.lege(2, 1, b"zwei".to_vec(), jetzt);
        puffer.entferne(1);
        assert_eq!(puffer.eintraege.len(), 1);
        assert_eq!(puffer.nehmen(2, 1), vec![b"zwei".to_vec()]);
    }
}

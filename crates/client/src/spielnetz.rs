// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Das Spielnetz im Client-Kern (ADR 0016).
//!
//! # Warum ein eigener Baustein
//!
//! Der Sitzungsthread ([`crate::anwendung`]) bleibt, wie er ist. Das Spielnetz
//! hängt **daneben**: es öffnet eine zweite QUIC-Verbindung zum selben Port
//! (eigener ALPN, ADR 0016 §2), hält ihre Angebote und ihre lokalen Ports und
//! meldet seinen Stand als [`Spielnetz`] in den Zustand. Angehängt wird es an
//! der Fassade ([`crate::sitzung`]) — `abgleichen` bekommt den Sollzustand
//! (Einstellung an/aus, Sitzung ja/nein), alles Weitere macht dieser Baustein.
//!
//! # Zwei Fäden, ein Sollzustand
//!
//! Angestoßen wird aus zwei Richtungen: aus der Oberfläche (Einstellungen
//! gespeichert) und aus dem Sitzungsthread (jede Zustandsmeldung). Beide rufen
//! [`Spielnetzbetrieb::abgleichen`], das den Sollzustand mit dem Ist vergleicht
//! und nur dann etwas tut, wenn er sich geändert hat. **Kein Endlos-Neuversuch:**
//! ein neuer Anlauf gibt es nur bei neuer Sitzung oder neuem Einschalten
//! (ADR 0016 §3) — ein Server ohne Modul wird nicht im Takt angefragt.
//!
//! # Was hier nicht passiert
//!
//! Kein Byte Spielverkehr läuft durch diesen Baustein: Ports, Ströme und
//! Kanäle gehören dem [`Tunnel`] aus `sprechwerk-spielnetz`. Hier stehen nur
//! der Sollzustand, die Abbildung auf das Modell und die Befehle.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;

use sprechwerk_adapters::Schluessel;
use sprechwerk_ipc::{Spielangebot, Spielart, Spielnetz, Spielteilnehmer};
use sprechwerk_spielnetz::client::Anmeldefehler;
use sprechwerk_spielnetz::{Art, Lage, Tunnel};

/// Was die Oberfläche am Spielnetz tun will (AGENTS.md §2.2: Summentyp).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spielbefehl {
    /// Einen Port des eigenen Rechners freigeben (ADR 0016 §4.1).
    Anbieten {
        name: String,
        port: u16,
        art: Spielart,
    },
    /// Die eigene Freigabe zurücknehmen.
    Zurueckziehen { angebot: u32 },
    /// Einer Freigabe beitreten; der lokale Port steht danach im Modell.
    Beitreten { angebot: u32 },
    /// Eine Freigabe verlassen, der man beigetreten ist.
    Verlassen { angebot: u32 },
    /// Einen Teilnehmer aus der eigenen Freigabe werfen.
    Entfernen { angebot: u32, kennung: String },
}

impl Spielbefehl {
    /// Kurzname für Meldungen („beitreten fehlgeschlagen: …").
    pub fn name(&self) -> &'static str {
        match self {
            Spielbefehl::Anbieten { .. } => "anbieten",
            Spielbefehl::Zurueckziehen { .. } => "zurueckziehen",
            Spielbefehl::Beitreten { .. } => "beitreten",
            Spielbefehl::Verlassen { .. } => "verlassen",
            Spielbefehl::Entfernen { .. } => "entfernen",
        }
    }
}

/// Wird gerufen, wenn sich der Stand des Spielnetzes geändert hat.
type Melder = Arc<dyn Fn() + Send + Sync>;

/// Was über den Sollzustand hinaus zu merken ist.
#[derive(Default)]
struct Inner {
    /// Der Stand, den die Oberfläche sieht.
    stand: Spielnetz,
    /// Sitzung, für die ein Tunnel läuft **oder gerade aufgebaut wird**.
    aktiv: Option<(String, String)>,
    /// Der laufende Tunnel (die Aufgaben halten ihre Sockets selbst).
    tunnel: Option<Arc<Tunnel>>,
    /// Sitzung, für die zuletzt ein Anlauf gestartet wurde — auch nach einem
    /// Fehlschlag. Dieselbe Sitzung bekommt keinen zweiten Versuch.
    versucht: Option<(String, String)>,
    /// Zählt bei jedem Neuanlauf hoch: ein alter Aufbau darf keinen neuen Stand
    /// schreiben und ein alter Beobachter nicht weiterreden.
    lauf: u64,
}

/// Das Spielnetz auf der Clientseite.
///
/// Wird beim [`crate::Client`] angelegt und lebt so lange wie er. Der Betrieb
/// blockiert nie: `abgleichen` legt den Sollzustand ab, die Arbeit läuft in
/// Aufgaben der übergebenen Laufzeit.
pub struct Spielnetzbetrieb {
    laufzeit: tokio::runtime::Handle,
    /// Dieselbe Schlüsseldatei wie die Sitzung — eine Kennung, ein Gerät.
    schluessel_pfad: PathBuf,
    melde: Melder,
    inner: Arc<Mutex<Inner>>,
}

impl Spielnetzbetrieb {
    /// Legt den Betrieb an. `melde` wird bei jeder Änderung des Standes gerufen
    /// (aus beliebigen Fäden) — die Fassade meldet daraufhin den Zustand neu.
    pub fn neu(
        laufzeit: tokio::runtime::Handle,
        schluessel_pfad: PathBuf,
        melde: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            laufzeit,
            schluessel_pfad,
            melde: Arc::new(melde),
            inner: Arc::new(Mutex::new(Inner::default())),
        })
    }

    /// Gleicht den Sollzustand ab.
    ///
    /// * `an` — die Einstellung „Spielnetz" (ADR 0016 §3).
    /// * `sitzung` — `Some((Serveradresse, Fingerabdruck))`, wenn die Sitzung
    ///   steht; `None`, wenn nicht (dann wartet das Modul, statt zu verbinden).
    ///
    /// Der Aufruf ist billig und darf bei jeder Zustandsmeldung kommen: er tut
    /// nur etwas, wenn sich der Sollzustand geändert hat.
    pub fn abgleichen(&self, an: bool, sitzung: Option<(String, String)>) {
        if !an {
            self.beende_tunnel();
            // Neues Einschalten ist ein neuer Versuch, auch für dieselbe
            // Sitzung (ADR 0016 §3: „erst dann geht das Angebot an den Server").
            self.merke(Spielnetz::Aus, |inner| {
                inner.versucht = None;
            });
            return;
        }
        let Some(sitzung) = sitzung else {
            // Ohne Sitzung keine zweite Verbindung — und ein späteres
            // Verbinden ist eine neue Sitzung, also ein neuer Versuch.
            self.beende_tunnel();
            self.merke(Spielnetz::Wartet, |inner| {
                inner.versucht = None;
            });
            return;
        };

        let entscheidung = {
            let mut inner = self.sperre();
            if inner.aktiv.as_ref() == Some(&sitzung) || inner.versucht.as_ref() == Some(&sitzung) {
                // Läuft, wird gerade aufgebaut, oder ist für diese Sitzung
                // schon gescheitert: nichts tun (kein Endlos-Neuversuch).
                None
            } else {
                inner.lauf = inner.lauf.wrapping_add(1);
                inner.aktiv = Some(sitzung.clone());
                inner.versucht = Some(sitzung.clone());
                inner.stand = Spielnetz::Verbindet;
                let lauf = inner.lauf;
                Some((lauf, inner.tunnel.take()))
            }
        };
        let Some((lauf, alt)) = entscheidung else {
            return;
        };
        // Erst den alten Tunnel beenden, dann melden — sonst sähe die
        // Oberfläche kurz zwei Wahrheiten.
        if let Some(alt) = alt {
            self.beende(alt);
        }
        (self.melde)();
        let inner = self.inner.clone();
        let melde = self.melde.clone();
        let pfad = self.schluessel_pfad.clone();
        self.laufzeit.spawn(async move {
            verbinde(lauf, inner, melde, pfad, sitzung).await;
        });
    }

    /// Der aktuelle Stand für das Modell.
    pub fn stand(&self) -> Spielnetz {
        self.sperre().stand.clone()
    }

    /// Führt einen Befehl aus. `Err` ist der Text für den Menschen.
    pub async fn befehl(&self, befehl: Spielbefehl) -> Result<(), String> {
        let tunnel = self
            .sperre()
            .tunnel
            .clone()
            .ok_or_else(|| "Spielnetz ist nicht verbunden".to_string())?;
        let ergebnis = match befehl {
            Spielbefehl::Anbieten { name, port, art } => {
                tunnel.anbieten(&name, port, art_aus(art)).await.map(|_| ())
            }
            Spielbefehl::Zurueckziehen { angebot } => tunnel.zurueckziehen(angebot).await,
            Spielbefehl::Beitreten { angebot } => tunnel.beitreten(angebot).await.map(|_| ()),
            Spielbefehl::Verlassen { angebot } => tunnel.verlassen(angebot).await,
            Spielbefehl::Entfernen { angebot, kennung } => {
                tunnel.entfernen(angebot, &kennung).await
            }
        };
        ergebnis.map_err(|fehler| fehler.to_string())
    }

    /// Setzt den Stand und ruft dabei `aendere` unter derselben Sperre.
    ///
    /// Meldet nur, wenn sich der Stand wirklich geändert hat — sonst käme bei
    /// jeder Zustandsmeldung der Sitzung eine überflüssige dazu.
    fn merke(&self, stand: Spielnetz, aendere: impl FnOnce(&mut Inner)) {
        let geaendert = {
            let mut inner = self.sperre();
            aendere(&mut inner);
            let geaendert = inner.stand != stand;
            inner.stand = stand;
            geaendert
        };
        if geaendert {
            (self.melde)();
        }
    }

    /// Nimmt den Tunnel aus dem Betrieb und beendet ihn in einer Aufgabe.
    fn beende_tunnel(&self) {
        let tunnel = {
            let mut inner = self.sperre();
            // Ein laufender Aufbau ist damit ungültig.
            inner.lauf = inner.lauf.wrapping_add(1);
            inner.aktiv = None;
            inner.tunnel.take()
        };
        if let Some(tunnel) = tunnel {
            self.beende(tunnel);
        }
    }

    /// Beendet einen Tunnel, ohne zu blockieren.
    ///
    /// [`Tunnel::beenden`] wartet, bis die Aufgaben stehen — deshalb nicht hier,
    /// sondern in einer eigenen Aufgabe. Ist der Griff noch anderswo (ein
    /// laufender Befehl), genügt das Fallenlassen: [`Drop`] räumt ab.
    fn beende(&self, tunnel: Arc<Tunnel>) {
        self.laufzeit.spawn(async move {
            match Arc::try_unwrap(tunnel) {
                Ok(tunnel) => tunnel.beenden().await,
                Err(geteilt) => drop(geteilt),
            }
        });
    }

    fn sperre(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("Sperre des Spielnetzes")
    }

    /// Ein Blick auf den Stand, der den Betrieb **nicht** am Leben hält.
    ///
    /// Gedacht für den Rückruf, den die Fassade übergibt: der läuft bei jeder
    /// Änderung und muss dafür den Stand lesen können — ohne dass Rückruf und
    /// Betrieb sich gegenseitig festhalten (sonst würde keiner je frei).
    pub fn blick(&self) -> Standblick {
        Standblick(self.inner.clone())
    }
}

/// Ein Faden-sicherer Blick auf den Spielnetz-Stand.
#[derive(Clone)]
pub struct Standblick(Arc<Mutex<Inner>>);

impl Standblick {
    /// Der aktuelle Stand.
    pub fn stand(&self) -> Spielnetz {
        self.0.lock().expect("Sperre des Spielnetzes").stand.clone()
    }
}

impl std::fmt::Debug for Spielnetzbetrieb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spielnetzbetrieb")
            .field("stand", &self.stand())
            .finish_non_exhaustive()
    }
}

/// Baut die Verbindung auf und meldet das Ergebnis in den Stand.
///
/// Alle Fehler landen im Modell, nicht in einem Protokoll: „dieser Server
/// bietet kein Spielnetz an" ist eine Auskunft, die der Mensch sehen soll
/// (ADR 0016 §3).
async fn verbinde(
    lauf: u64,
    inner: Arc<Mutex<Inner>>,
    melde: Melder,
    schluessel_pfad: PathBuf,
    sitzung: (String, String),
) {
    let (server, fingerabdruck) = sitzung;
    let adresse = match aufloesen(&server).await {
        Ok(adresse) => adresse,
        Err(text) => {
            scheitern(&inner, &melde, lauf, Spielnetz::Fehler { text });
            return;
        }
    };
    let schluessel = match Schluessel::laden_oder_erzeugen(&schluessel_pfad) {
        Ok(schluessel) => schluessel,
        Err(fehler) => {
            scheitern(
                &inner,
                &melde,
                lauf,
                Spielnetz::Fehler {
                    text: format!("Schlüssel: {fehler}"),
                },
            );
            return;
        }
    };
    let ich = schluessel.kennung();

    let (tunnel, lage) = match Tunnel::starten(adresse, schluessel, &fingerabdruck).await {
        Ok(paar) => paar,
        Err(Anmeldefehler::NichtAngeboten) => {
            scheitern(&inner, &melde, lauf, Spielnetz::NichtAngeboten);
            return;
        }
        // Ablehnung und Netzfehler kommen beide als Text: der Grund steht im
        // Schließen („dafür fehlt das Recht", „nicht angemeldet"), und der ist
        // genau das, was der Mensch lesen soll.
        Err(Anmeldefehler::Sonst(fehler)) => {
            scheitern(
                &inner,
                &melde,
                lauf,
                Spielnetz::Abgelehnt {
                    grund: format!("{fehler:#}"),
                },
            );
            return;
        }
    };

    {
        let mut gesperrt = inner.lock().expect("Sperre des Spielnetzes");
        if gesperrt.lauf != lauf {
            // Inzwischen abgeschaltet oder eine neue Sitzung: dieser Tunnel
            // gehört niemandem mehr.
            drop(gesperrt);
            drop(tunnel);
            return;
        }
        gesperrt.tunnel = Some(Arc::new(tunnel));
    }
    melde();

    beobachte(lauf, inner, melde, ich, lage).await;
}

/// Überträgt jede Änderung der [`Lage`] in den Stand.
async fn beobachte(
    lauf: u64,
    inner: Arc<Mutex<Inner>>,
    melde: Melder,
    ich: String,
    mut lage: watch::Receiver<Lage>,
) {
    loop {
        let stand = abbilden(&lage.borrow_and_update(), &ich);
        if !setze_stand(&inner, &melde, lauf, stand) {
            return;
        }
        if lage.changed().await.is_err() {
            // Der Tunnel ist weg; ob das ein Fehler war, sagt sein letzter
            // Stand (die Meldung darin).
            return;
        }
    }
}

/// Setzt den Stand, sofern dieser Aufbau noch der aktuelle ist.
///
/// `false` heißt: ein neuer Anlauf hat übernommen — dieser Beobachter hört auf.
fn setze_stand(inner: &Mutex<Inner>, melde: &Melder, lauf: u64, stand: Spielnetz) -> bool {
    let geaendert = {
        let mut gesperrt = inner.lock().expect("Sperre des Spielnetzes");
        if gesperrt.lauf != lauf {
            return false;
        }
        let geaendert = gesperrt.stand != stand;
        gesperrt.stand = stand;
        geaendert
    };
    if geaendert {
        melde();
    }
    true
}

/// Ein fehlgeschlagener Anlauf: Stand setzen, `aktiv` freigeben (es läuft
/// nichts), `versucht` stehen lassen (kein zweiter Versuch).
fn scheitern(inner: &Mutex<Inner>, melde: &Melder, lauf: u64, stand: Spielnetz) {
    let geaendert = {
        let mut gesperrt = inner.lock().expect("Sperre des Spielnetzes");
        if gesperrt.lauf != lauf {
            return;
        }
        gesperrt.aktiv = None;
        let geaendert = gesperrt.stand != stand;
        gesperrt.stand = stand;
        geaendert
    };
    if geaendert {
        melde();
    }
}

/// Der Stand, den die Oberfläche zeigt — **reine Funktion** von Tunnel-Lage,
/// eigener Kennung. Ohne I/O und ohne Sperre, damit die Abbildung prüfbar ist.
///
/// `eigenes` folgt aus der Kennung des Gastgebers, `lokal` aus der eigenen
/// Teilnahme: beides steht in der Lage, nicht im Angebot.
pub fn abbilden(lage: &Lage, ich: &str) -> Spielnetz {
    let angebote = lage
        .angebote
        .iter()
        .map(|angebot| Spielangebot {
            id: angebot.id,
            name: angebot.name.clone(),
            port: angebot.port,
            art: art_nach(angebot.art),
            gastgeber: angebot.gastgeber_name.clone(),
            eigenes: !ich.is_empty() && angebot.gastgeber == ich,
            teilnehmer: angebot
                .teilnehmer
                .iter()
                .map(|teilnehmer| Spielteilnehmer {
                    kennung: teilnehmer.kennung.clone(),
                    name: teilnehmer.name.clone(),
                })
                .collect(),
            lokal: lage
                .teilnahmen
                .iter()
                .find(|teilnahme| teilnahme.angebot == angebot.id)
                .map(|teilnahme| teilnahme.lokal.to_string()),
        })
        .collect();
    Spielnetz::Bereit {
        angebote,
        meldung: lage.meldung.clone(),
    }
}

/// Die Art des Ports, wie sie das Modell kennt — `ipc` hängt bewusst nicht am
/// Spielnetz-Crate (AGENTS.md §2: `ipc` bleibt schlank).
fn art_nach(art: Art) -> Spielart {
    match art {
        Art::Tcp => Spielart::Tcp,
        Art::Udp => Spielart::Udp,
    }
}

fn art_aus(art: Spielart) -> Art {
    match art {
        Spielart::Tcp => Art::Tcp,
        Spielart::Udp => Art::Udp,
    }
}

/// Löst eine Serveradresse auf — **dieselbe Funktion wie die Sitzung**.
///
/// `host:port` wird direkt genommen; sonst fragt der Namensdienst. Der erste
/// Treffer gilt (IPv4 oder IPv6, wie er kommt).
pub(crate) async fn aufloesen(server: &str) -> Result<SocketAddr, String> {
    if let Ok(ziel) = server.parse::<SocketAddr>() {
        return Ok(ziel);
    }
    match tokio::net::lookup_host(server).await {
        Ok(mut adressen) => adressen
            .next()
            .ok_or_else(|| format!("{server} nicht auflösbar")),
        Err(fehler) => Err(format!("{server}: {fehler}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_spielnetz::{Angebot, Teilnahme};

    const ICH: &str = "aa";

    fn angebot(id: u32, art: Art) -> Angebot {
        Angebot {
            id,
            name: "Minecraft".to_string(),
            port: 25565,
            art,
            gastgeber: "bb".repeat(32),
            gastgeber_name: "Bosse".to_string(),
            teilnehmer: Vec::new(),
        }
    }

    #[test]
    fn fremdes_angebot_ohne_teilnahme() {
        let lage = Lage {
            angebote: vec![angebot(1, Art::Tcp)],
            ..Lage::default()
        };
        let stand = abbilden(&lage, ICH);
        let Spielnetz::Bereit { angebote, meldung } = stand else {
            panic!("erwartet: bereit");
        };
        assert!(meldung.is_none());
        assert_eq!(angebote.len(), 1);
        let a = &angebote[0];
        assert_eq!(a.id, 1);
        assert_eq!(a.name, "Minecraft");
        assert_eq!(a.port, 25565);
        assert_eq!(a.art, Spielart::Tcp);
        assert_eq!(a.gastgeber, "Bosse", "der Name, nicht die Kennung");
        assert!(!a.eigenes);
        assert_eq!(a.lokal, None, "nicht beigetreten");
    }

    #[test]
    fn eigenes_angebot_und_andere_art() {
        let mut eigenes = angebot(2, Art::Udp);
        eigenes.gastgeber = ICH.to_string();
        eigenes.gastgeber_name = "Ada".to_string();
        let lage = Lage {
            angebote: vec![eigenes],
            eigene: vec![2],
            ..Lage::default()
        };
        let Spielnetz::Bereit { angebote, .. } = abbilden(&lage, ICH) else {
            panic!("erwartet: bereit");
        };
        assert!(angebote[0].eigenes, "die eigene Freigabe ist markiert");
        assert_eq!(angebote[0].art, Spielart::Udp);
        assert_eq!(angebote[0].ziel(), "25565/udp");
    }

    #[test]
    fn beigetretenes_angebot_hat_die_lokale_adresse() {
        let lage = Lage {
            angebote: vec![angebot(3, Art::Tcp)],
            teilnahmen: vec![Teilnahme {
                angebot: 3,
                lokal: "127.0.0.1:25566".parse().expect("Adresse"),
            }],
            ..Lage::default()
        };
        let Spielnetz::Bereit { angebote, .. } = abbilden(&lage, ICH) else {
            panic!("erwartet: bereit");
        };
        assert_eq!(angebote[0].lokal.as_deref(), Some("127.0.0.1:25566"));
    }

    #[test]
    fn teilnehmer_tragen_kennung_und_namen() {
        let mut mit_teilnehmern = angebot(4, Art::Tcp);
        mit_teilnehmern.teilnehmer = vec![
            sprechwerk_spielnetz::Teilnehmer {
                kennung: "cc".to_string(),
                name: "Cleo".to_string(),
            },
            sprechwerk_spielnetz::Teilnehmer {
                kennung: "dd".to_string(),
                name: "Dana".to_string(),
            },
        ];
        let lage = Lage {
            angebote: vec![mit_teilnehmern],
            ..Lage::default()
        };
        let Spielnetz::Bereit { angebote, .. } = abbilden(&lage, ICH) else {
            panic!("erwartet: bereit");
        };
        assert_eq!(
            angebote[0].teilnehmer,
            vec![
                Spielteilnehmer {
                    kennung: "cc".to_string(),
                    name: "Cleo".to_string(),
                },
                Spielteilnehmer {
                    kennung: "dd".to_string(),
                    name: "Dana".to_string(),
                },
            ],
            "die Oberfläche braucht beides: Name zum Zeigen, Kennung zum Entfernen"
        );
    }

    #[test]
    fn meldung_geht_mit_ins_modell() {
        let lage = Lage {
            meldung: Some("Freigabe zurückgezogen".to_string()),
            ..Lage::default()
        };
        let Spielnetz::Bereit { angebote, meldung } = abbilden(&lage, ICH) else {
            panic!("erwartet: bereit");
        };
        assert!(angebote.is_empty());
        assert_eq!(meldung.as_deref(), Some("Freigabe zurückgezogen"));
    }

    #[test]
    fn ohne_eigene_kennung_ist_nichts_eigenes() {
        // Solange die Kennung nicht bekannt ist, wird nichts als „meins"
        // ausgegeben — lieber nichts markieren als falsch.
        let mut eigenes = angebot(5, Art::Tcp);
        eigenes.gastgeber = String::new();
        let lage = Lage {
            angebote: vec![eigenes],
            ..Lage::default()
        };
        let Spielnetz::Bereit { angebote, .. } = abbilden(&lage, "") else {
            panic!("erwartet: bereit");
        };
        assert!(!angebote[0].eigenes);
    }

    #[test]
    fn befehle_haben_namen() {
        assert_eq!(Spielbefehl::Beitreten { angebot: 1 }.name(), "beitreten");
        assert_eq!(
            Spielbefehl::Entfernen {
                angebot: 1,
                kennung: "aa".into()
            }
            .name(),
            "entfernen"
        );
    }

    #[tokio::test]
    async fn adresse_wird_aufgeloest() {
        let direkt = aufloesen("127.0.0.1:7443").await.expect("IPv4");
        assert_eq!(direkt.port(), 7443);
        assert!(direkt.ip().is_loopback());
        let name = aufloesen("localhost:7443").await.expect("Name");
        assert_eq!(name.port(), 7443);
        assert!(aufloesen("kein.gueltiger.name.invalid:1").await.is_err());
    }

    #[tokio::test]
    async fn ohne_tunnel_ist_kein_befehl_moeglich() {
        let betrieb =
            Spielnetzbetrieb::neu(tokio::runtime::Handle::current(), PathBuf::new(), || {});
        assert_eq!(betrieb.stand(), Spielnetz::Aus);
        let fehler = betrieb
            .befehl(Spielbefehl::Beitreten { angebot: 1 })
            .await
            .expect_err("ohne Verbindung kein Befehl");
        assert!(fehler.contains("nicht verbunden"), "{fehler}");
    }

    #[tokio::test]
    async fn aus_und_ohne_sitzung_wartet_der_stand() {
        let betrieb =
            Spielnetzbetrieb::neu(tokio::runtime::Handle::current(), PathBuf::new(), || {});
        betrieb.abgleichen(false, None);
        assert_eq!(betrieb.stand(), Spielnetz::Aus);

        betrieb.abgleichen(true, None);
        assert_eq!(
            betrieb.stand(),
            Spielnetz::Wartet,
            "eingeschaltet, aber ohne Sitzung"
        );

        betrieb.abgleichen(false, None);
        assert_eq!(betrieb.stand(), Spielnetz::Aus, "ausschalten wirkt sofort");
        assert!(betrieb.stand().angebote().is_empty());
    }

    /// Dieselbe Sitzung bekommt keinen zweiten Anlauf — sonst fragte der Client
    /// einen Server ohne Modul im Takt der Zustandsmeldungen (ADR 0016 §3).
    #[tokio::test]
    async fn dieselbe_sitzung_bekommt_keinen_zweiten_anlauf() {
        let pfad = std::env::temp_dir().join(format!(
            "sprechwerk-spielnetz-schluessel-{}",
            std::process::id()
        ));
        let betrieb = Spielnetzbetrieb::neu(tokio::runtime::Handle::current(), pfad.clone(), || {});
        // Eine Adresse, an der nichts lauscht: der Anlauf scheitert.
        let sitzung = Some(("127.0.0.1:1".to_string(), "ab".repeat(32)));
        betrieb.abgleichen(true, sitzung.clone());
        let erster_lauf = {
            let inner = betrieb.sperre();
            assert_eq!(inner.stand, Spielnetz::Verbindet, "erst wird verbunden");
            assert_eq!(inner.versucht, sitzung, "die Sitzung ist gemerkt");
            inner.lauf
        };

        // Zweimal dieselbe Sitzung: kein neuer Anlauf, kein neuer Stand.
        betrieb.abgleichen(true, sitzung.clone());
        betrieb.abgleichen(true, sitzung.clone());
        assert_eq!(
            betrieb.sperre().lauf,
            erster_lauf,
            "dieselbe Sitzung wird nicht erneut versucht"
        );

        // Ausschalten und wieder einschalten ist ein neuer Versuch.
        betrieb.abgleichen(false, None);
        assert_eq!(betrieb.stand(), Spielnetz::Aus);
        betrieb.abgleichen(true, sitzung.clone());
        assert!(
            betrieb.sperre().lauf > erster_lauf,
            "neues Einschalten, neuer Anlauf"
        );

        betrieb.abgleichen(false, None);
        let _ = std::fs::remove_file(&pfad);
    }
}

//! Die Anwendung hinter dem Fenster: eine Verbindung, ein Modellstand.
//!
//! **Aufbau (AGENTS.md §3):** Das Fenster schickt [`Befehl`]e, diese Schicht
//! redet mit dem Netz (`sprechwerk_adapters`) und schreibt den Modellstand
//! ([`Zustand`]) fort. Nach jeder Änderung wird der ganze Stand ans Fenster
//! gemeldet — die Oberfläche ist `render(modell)`, sie hält keinen eigenen
//! Zustand.
//!
//! Die Verbindung lebt in einem **eigenen Thread mit eigener Laufzeit**. Grund:
//! das Fenster darf nie blockieren, und ein Netzfehler darf die Oberfläche
//! nicht anhalten.

use crate::dateien::Dateiablage;
use crate::sprech::Sprechbetrieb;
use crate::verschluesselung::Schluesselverwaltung;
use sprechwerk_adapters::anhang::{self, Dateischluessel};
use sprechwerk_adapters::serverschluessel::{Bund, Lesbar};
use sprechwerk_adapters::{Schluessel, Verbindung as Netz};
use sprechwerk_ipc::{abbilden, Antwort, Befehl, Zustand};
use sprechwerk_ipc::{Anhangentwurf, Dateiart, Entwurfsstand, Ladestand, Linkentwurf};
use sprechwerk_protocol::inhalt::{Datei, Inhalt, Linkvorschau};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// Ein Auftrag des Fensters an den Verbindungsthread.
enum Auftrag {
    Verbinden {
        server: String,
        name: String,
        passwort: Option<String>,
        /// Angehefteter Fingerabdruck des Server-Zertifikats (TOFU).
        /// `None` beim ersten Kontakt.
        erwartet: Option<String>,
        antwort: oneshot::Sender<Result<String, String>>,
    },
    Befehl {
        befehl: Befehl,
        antwort: oneshot::Sender<Antwort>,
    },
    /// Eine Link-Vorschau ist abgerufen (oder nicht zu haben). Kommt von der
    /// Abrufaufgabe zurück in die Schleife — nur dort wird das Modell geändert.
    Vorschau {
        url: String,
        vorschau: Option<Linkvorschau>,
    },
    /// Das Vorschaubild einer Datei im Entwurf ist gerechnet (Base64, Maße).
    Vorbereitet {
        nr: u64,
        vorschau: Option<(String, u32, u32)>,
    },
    /// Eine Datei im Entwurf lädt hoch (oder ist fertig: `ergebnis`).
    Hochladen {
        nr: u64,
        prozent: u8,
        ergebnis: Option<Result<(), String>>,
    },
    /// Ladestand einer empfangenen Datei.
    Laden { id: String, stand: Ladestand },
    /// Eine Datei ist entschlüsselt gespeichert — oder nicht.
    Gespeichert { ergebnis: Result<String, String> },
}

/// Eine Datei im Entwurf: was die Oberfläche nicht sieht (Kennung, Schlüssel,
/// Aufgabe) und was sie sieht (`ansicht`).
struct Entwurfsdatei {
    id: String,
    schluessel: Dateischluessel,
    typ: &'static str,
    breite: Option<u32>,
    hoehe: Option<u32>,
    ansicht: Anhangentwurf,
    aufgabe: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Entwurfsdatei {
    // Entfernt oder getrennt: das Hochladen bricht ab, der Server verwirft
    // den halben Upload.
    fn drop(&mut self) {
        if let Some(a) = self.aufgabe.take() {
            a.abort();
        }
    }
}

/// Vorschaubilder der Anhänge: kleiner als die einer Link-Vorschau, weil
/// bis zu 16 in einen Umschlag passen müssen.
const ANHANG_KANTE: u32 = 360;
const ANHANG_VORSCHAU_BYTES: usize = 40 * 1024;
/// Größer liest niemand ein Bild ganz ein, nur für ein Vorschaubild.
const ANHANG_VORSCHAU_HOECHSTENS: u64 = 64 * 1024 * 1024;

/// Der Zugang des Fensters zur Anwendung.
pub struct Anwendung {
    auftrag: mpsc::UnboundedSender<Auftrag>,
    zustand: Arc<Mutex<Zustand>>,
    sprech: Arc<Sprechbetrieb>,
}

impl Anwendung {
    /// Startet den Verbindungsthread.
    ///
    /// `melde` wird nach jeder Änderung mit dem vollständigen Stand aufgerufen
    /// (im Fenster: Ereignis `zustand`).
    pub fn starten<F>(
        schluessel_pfad: PathBuf,
        sprech: Arc<Sprechbetrieb>,
        verschluesselung: Arc<Schluesselverwaltung>,
        ablage: Arc<Dateiablage>,
        melde: F,
    ) -> Self
    where
        F: Fn(Zustand) + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let rueck = tx.clone();
        let zustand = Arc::new(Mutex::new(Zustand::neu()));

        let zustand_thread = zustand.clone();
        let sprech_thread = sprech.clone();
        let verwaltung = verschluesselung.clone();
        std::thread::Builder::new()
            .name("sprechwerk-verbindung".to_string())
            .spawn(move || {
                let laufzeit = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(fehler) => {
                        tracing_stumm(&zustand_thread, &melde, format!("Laufzeit: {fehler}"));
                        return;
                    }
                };
                laufzeit.block_on(lauf(
                    rx,
                    rueck,
                    zustand_thread,
                    Teile {
                        sprech: sprech_thread,
                        verwaltung,
                        ablage,
                    },
                    schluessel_pfad,
                    melde,
                ));
            })
            .expect("Verbindungsthread");

        Self {
            auftrag: tx,
            zustand,
            sprech,
        }
    }

    /// Der Sprechbetrieb (für die Befehle des Fensters).
    pub fn sprech(&self) -> Arc<Sprechbetrieb> {
        self.sprech.clone()
    }

    /// Der zuletzt gemeldete Stand.
    pub fn zustand(&self) -> Zustand {
        self.zustand.lock().expect("Sperre").clone()
    }

    /// Baut eine Verbindung auf. Liefert den Servernamen.
    pub async fn verbinden(
        &self,
        server: String,
        name: String,
        passwort: Option<String>,
        erwarteter_fingerabdruck: Option<String>,
    ) -> Result<String, String> {
        let (tx, rx) = oneshot::channel();
        self.auftrag
            .send(Auftrag::Verbinden {
                server,
                name,
                passwort,
                erwartet: erwarteter_fingerabdruck,
                antwort: tx,
            })
            .map_err(|_| "Verbindungsthread ist beendet".to_string())?;
        rx.await.map_err(|_| "keine Antwort".to_string())?
    }

    /// Schickt einen Befehl und wartet auf die Antwort.
    pub async fn befehl(&self, befehl: Befehl) -> Antwort {
        let (tx, rx) = oneshot::channel();
        if self
            .auftrag
            .send(Auftrag::Befehl {
                befehl,
                antwort: tx,
            })
            .is_err()
        {
            return Antwort::Fehler {
                text: "Verbindungsthread ist beendet".to_string(),
            };
        }
        rx.await.unwrap_or(Antwort::Fehler {
            text: "keine Antwort".to_string(),
        })
    }
}

fn tracing_stumm<F>(zustand: &Arc<Mutex<Zustand>>, melde: &F, text: String)
where
    F: Fn(Zustand),
{
    if let Ok(mut z) = zustand.lock() {
        z.meldung = Some(text);
        melde(z.clone());
    }
}

/// Was die Schleife neben Modell und Aufträgen braucht: Ton, Raumschlüssel,
/// Dateien.
struct Teile {
    sprech: Arc<Sprechbetrieb>,
    verwaltung: Arc<Schluesselverwaltung>,
    ablage: Arc<Dateiablage>,
}

/// Die Schleife: Aufträge annehmen, Rahmen des Servers verarbeiten.
async fn lauf<F>(
    mut rx: mpsc::UnboundedReceiver<Auftrag>,
    rueck: mpsc::UnboundedSender<Auftrag>,
    zustand: Arc<Mutex<Zustand>>,
    teile: Teile,
    schluessel_pfad: PathBuf,
    melde: F,
) where
    F: Fn(Zustand) + Send + 'static,
{
    let Teile {
        sprech,
        verwaltung,
        ablage,
    } = teile;
    let mut netz: Option<Netz> = None;
    // Der eigene Anmeldeschlüssel — nötig, um Raumschlüssel zu verteilen und
    // Umschläge zu öffnen.
    let mut eigener: Option<Schluessel> = None;
    // Serverschlüssel der aktuellen Verbindung (ADR 0012): liest und schreibt
    // den Chat. Neu je Verbindung, gemerkt in einer Datei neben dem Schlüssel.
    let mut bund: Option<Bund> = None;
    // Kommende Sprache aus dem Netz (eigene Aufgabe, eigener Pfad).
    let mut sprache: Option<mpsc::UnboundedReceiver<Vec<u8>>> = None;
    // Raum, in dem wir gerade sitzen — für Ver- und Entschlüsselung im
    // Netzrand (die Aufgaben dort kennen die Verbindung nicht).
    let aktueller: Arc<Mutex<Option<sprechwerk_domain::RoomId>>> = Arc::new(Mutex::new(None));
    // Wann das letzte Lebenszeichen hinausging (für die Umlaufzeit).
    let mut ping_gesendet: Option<std::time::Instant> = None;
    // Ruft Link-Vorschauen ab (ADR 0012 §2: der Absender, nicht die Empfänger).
    let holer = sprechwerk_linkvorschau::Holer::neu().ok();
    // Dateien im Entwurf (ADR 0012 §3), in der Reihenfolge des Hinzufügens.
    let mut entwuerfe: Vec<Entwurfsdatei> = Vec::new();
    let mut naechste_nr: u64 = 1;
    // Was gerade lädt oder scheiterte; „bereit" sagt der Zwischenspeicher.
    let mut staende: HashMap<String, Ladestand> = HashMap::new();

    loop {
        let mut auftrag: Option<Auftrag> = None;
        let mut rahmen: Option<anyhow::Result<Option<sprechwerk_protocol::ServerFrame>>> = None;

        match netz.as_mut() {
            Some(verbindung) => match sprache.as_mut() {
                Some(hoeren) => {
                    tokio::select! {
                        a = rx.recv() => auftrag = a,
                        f = verbindung.empfang() => rahmen = Some(f),
                        d = hoeren.recv() => {
                            if let Some(d) = d {
                                if let Some(klar) = entschluessle(&verwaltung, &aktueller, &d) {
                                    sprech.hoeren(klar);
                                }
                            }
                        }
                    }
                }
                None => {
                    tokio::select! {
                        a = rx.recv() => auftrag = a,
                        f = verbindung.empfang() => rahmen = Some(f),
                    }
                }
            },
            None => auftrag = rx.recv().await,
        }

        // ---- Rahmen des Servers ----------------------------------------
        if let Some(ergebnis) = rahmen {
            let mut neu_melden = false;
            match ergebnis {
                Ok(Some(frame)) => {
                    // Schlüsselpaket: gehört nicht ins Modell, sondern in die
                    // Schlüsselverwaltung.
                    if let Some(paket) = crate::verschluesselung::aus_rahmen(&frame) {
                        if let (Some(v), Some(e)) = (netz.as_ref(), eigener.as_ref()) {
                            nimm_schluesselpaket(v, &verwaltung, e, paket);
                        }
                    }
                    // Serverschlüssel: Stand, Bitten, Pakete — Antworten gehen
                    // sofort hinaus, das Modell sieht davon nur das Ergebnis.
                    if let (Some(v), Some(e), Some(b)) =
                        (netz.as_mut(), eigener.as_ref(), bund.as_mut())
                    {
                        serverschluessel_bedienen(v, b, e, &frame).await;
                    }
                    // Jemand bittet um den Schlüssel: die Verteilung neu
                    // anstoßen, damit der nächste Durchlauf ihn bedient.
                    if let sprechwerk_protocol::ServerFrame::SchluesselAnfrage { raum, .. } = &frame
                    {
                        verwaltung.verteilung_erneuern(*raum);
                    }
                    // Umlaufzeit: das Pong trägt die Antwort auf unser Ping.
                    let gemessen = if matches!(frame, sprechwerk_protocol::ServerFrame::Pong) {
                        ping_gesendet
                            .take()
                            .map(|wann| wann.elapsed().as_millis() as u32)
                    } else {
                        None
                    };
                    let mut raum_wechsel = false;
                    if let Ok(mut z) = zustand.lock() {
                        let vorher = z.aktueller_raum;
                        abbilden::verarbeite(&mut z, frame);
                        if let Some(b) = bund.as_ref() {
                            chat_lesen(&mut z, b, &ablage, &staende);
                        }
                        if let Some(ms) = gemessen {
                            z.latenz_ms = Some(ms);
                        }
                        raum_wechsel = z.aktueller_raum != vorher;
                        neu_melden = true;
                    }
                    if let Ok(mut a) = aktueller.lock() {
                        *a = zustand.lock().ok().and_then(|z| z.aktueller_raum);
                    }
                    // Neuer Raum oder neue Mitglieder: Schlüssel prüfen.
                    if let (Some(v), Some(e)) = (netz.as_mut(), eigener.as_ref()) {
                        verteile_schluessel(v, &verwaltung, e).await;
                    }
                    // Und den Stand ins Modell: die Oberfläche muss sagen
                    // können, ob der Schlüssel dieses Raums vorliegt — ohne ihn
                    // hört man in einem verschlüsselten Raum nichts.
                    if let Ok(mut z) = zustand.lock() {
                        z.schluessel = z
                            .raeume
                            .iter()
                            .filter(|r| r.verschluesselt)
                            .filter_map(|r| {
                                verwaltung
                                    .hole(r.id)
                                    .map(|s| sprechwerk_ipc::Schluesselstand {
                                        raum: r.id,
                                        epoche: s.epoche,
                                    })
                            })
                            .collect();
                    }
                    let _ = raum_wechsel;
                }
                Ok(None) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, None);
                        neu_melden = true;
                    }
                    entwuerfe.clear();
                    staende.clear();
                    netz = None;
                    sprache = None;
                    sprech.stoppen();
                }
                Err(fehler) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, Some(format!("Verbindung beendet: {fehler}")));
                        neu_melden = true;
                    }
                    entwuerfe.clear();
                    staende.clear();
                    netz = None;
                    sprache = None;
                    sprech.stoppen();
                }
            }
            if neu_melden {
                melden(&zustand, &melde);
            }
        }

        // ---- Aufträge des Fensters -------------------------------------
        let Some(auftrag) = auftrag else {
            if netz.is_none() {
                break; // Kanal zu und nichts mehr zu tun
            }
            continue;
        };

        match auftrag {
            Auftrag::Verbinden {
                server,
                name,
                passwort,
                erwartet,
                antwort,
            } => {
                if let Ok(mut z) = zustand.lock() {
                    z.verbindung = Some(sprechwerk_ipc::Verbindung::Verbindet {
                        server: server.clone(),
                    });
                    z.meldung = None;
                }
                melden(&zustand, &melde);

                let ziel = match server.parse::<std::net::SocketAddr>() {
                    Ok(z) => z,
                    Err(_) => match tokio::net::lookup_host(&server).await {
                        Ok(mut adressen) => match adressen.next() {
                            Some(a) => a,
                            None => {
                                let _ = antwort.send(Err(format!("{server} nicht auflösbar")));
                                if let Ok(mut z) = zustand.lock() {
                                    abbilden::getrennt(
                                        &mut z,
                                        Some(format!("{server} nicht auflösbar")),
                                    );
                                }
                                melden(&zustand, &melde);
                                continue;
                            }
                        },
                        Err(fehler) => {
                            let _ = antwort.send(Err(format!("{server}: {fehler}")));
                            if let Ok(mut z) = zustand.lock() {
                                abbilden::getrennt(&mut z, Some(format!("{server}: {fehler}")));
                            }
                            melden(&zustand, &melde);
                            continue;
                        }
                    },
                };

                let schluessel = match Schluessel::laden_oder_erzeugen(&schluessel_pfad) {
                    Ok(s) => s,
                    Err(fehler) => {
                        let _ = antwort.send(Err(format!("Schlüssel: {fehler}")));
                        continue;
                    }
                };
                eigener = Some(schluessel.clone());

                match Netz::oeffnen_mit(
                    ziel,
                    &schluessel,
                    &name,
                    passwort.as_deref(),
                    erwartet.as_deref(),
                )
                .await
                {
                    Ok(verbindung) => {
                        let server_name = verbindung.server_name.clone();
                        let raeume = verbindung.raeume.clone();
                        let mitglieder = verbindung.mitglieder.clone();
                        let raum = verbindung.raum;
                        let verlauf = verbindung.verlauf.clone();
                        let ich = verbindung.ich;
                        let rolle = verbindung.rolle;
                        // Was der Server gezeigt hat: gehört ins Modell, sonst
                        // kann niemand den angeheften Wert nachlesen.
                        let zertifikat = verbindung.zertifikat.clone();

                        // Sprache: eingehende Datagramme in einer eigenen Aufgabe
                        // lesen (der Steuerstrom gehört der Leseschleife oben).
                        let quelle = verbindung.datagramm_quelle();
                        let (tx_sprache, rx_sprache) = mpsc::unbounded_channel::<Vec<u8>>();
                        tokio::spawn(async move {
                            // Verbindung zu: Aufgabe beenden.
                            while let Ok(d) = quelle.read_datagram().await {
                                if tx_sprache.send(d.to_vec()).is_err() {
                                    break;
                                }
                            }
                        });

                        // Ausgehende Sprache: der Sprechpfad legt fertige
                        // Datagramme ab, hier gehen sie ins Netz.
                        let sendequelle = verbindung.datagramm_quelle();
                        let sprech_senden = sprech.clone();
                        let sende_verwaltung = verwaltung.clone();
                        let sende_raum = aktueller.clone();
                        tokio::spawn(async move {
                            loop {
                                while let Some(d) = sprech_senden.naechstes_datagramm() {
                                    let d = verschluessle(&sende_verwaltung, &sende_raum, d);
                                    if sendequelle.send_datagram(bytes::Bytes::from(d)).is_err() {
                                        return;
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            }
                        });

                        // Serverschlüssel: gemerkte laden, Stand melden, ggf.
                        // einen anlegen (erster Client auf einem neuen Server).
                        let mut neuer_bund = Bund::laden(
                            Some(schluessel_pfad.with_file_name("serverschluessel.json")),
                            &zertifikat,
                        );
                        let mut verbindung = verbindung;
                        for rahmen in neuer_bund.stand(verbindung.serverschluessel.clone()) {
                            if let Err(fehler) = verbindung.sende(rahmen).await {
                                eprintln!("Serverschlüssel: {fehler}");
                            }
                        }
                        bund = Some(neuer_bund);

                        sprache = Some(rx_sprache);
                        netz = Some(verbindung);
                        // Verschlüsselte Räume: Schlüssel besorgen (oder, als
                        // Wart, erzeugen und verteilen).
                        if let (Some(v), Some(e)) = (netz.as_mut(), eigener.as_ref()) {
                            verteile_schluessel(v, &verwaltung, e).await;
                        }
                        if let Ok(mut z) = zustand.lock() {
                            abbilden::verarbeite(
                                &mut z,
                                sprechwerk_protocol::ServerFrame::Willkommen {
                                    version: sprechwerk_protocol::PROTOKOLL_VERSION,
                                    server_name,
                                    ich,
                                    rolle,
                                    raeume,
                                    mitglieder,
                                    raum,
                                    verlauf,
                                    serverschluessel: Default::default(),
                                },
                            );
                            // **Nach** dem Verarbeiten setzen: die Begrüßung
                            // baut den Stand neu auf (`*zustand =
                            // aus_willkommen(..)`), ein vorher gesetzter Wert
                            // wäre wieder weg.
                            z.zertifikat = Some(zertifikat.clone());
                            if let Some(b) = bund.as_ref() {
                                chat_lesen(&mut z, b, &ablage, &staende);
                            }
                            // Der eigene Name gehört ins Modell (er kommt nicht
                            // über die Meldung zurück).
                            if let Some(sprechwerk_ipc::Verbindung::Verbunden { name: n, .. }) =
                                z.verbindung.as_mut()
                            {
                                *n = name.clone();
                            }
                        }
                        melden(&zustand, &melde);
                        let _ = antwort.send(Ok(name));
                    }
                    Err(fehler) => {
                        let text = format!("{fehler:#}");
                        if let Ok(mut z) = zustand.lock() {
                            abbilden::getrennt(&mut z, Some(text.clone()));
                        }
                        melden(&zustand, &melde);
                        let _ = antwort.send(Err(text));
                    }
                }
            }

            Auftrag::Vorschau { url, vorschau } => {
                if let Ok(mut z) = zustand.lock() {
                    // Nur, wenn der Entwurf noch auf genau diesen Link wartet —
                    // sonst ist die Antwort veraltet.
                    if matches!(&z.entwurf, Linkentwurf::Laedt { url: u } if *u == url) {
                        z.entwurf = match vorschau {
                            Some(vorschau) => Linkentwurf::Fertig { vorschau },
                            None => Linkentwurf::Ohne { url },
                        };
                    }
                }
                melden(&zustand, &melde);
            }

            Auftrag::Vorbereitet { nr, vorschau } => {
                if let Some(e) = entwuerfe.iter_mut().find(|e| e.ansicht.nr == nr) {
                    if let Some((bild, breite, hoehe)) = vorschau {
                        e.ansicht.vorschau = Some(bild);
                        e.breite = Some(breite);
                        e.hoehe = Some(hoehe);
                    }
                    if e.ansicht.stand == Entwurfsstand::Vorbereitet {
                        e.ansicht.stand = Entwurfsstand::Laedt { prozent: 0 };
                    }
                }
                entwurf_zeigen(&zustand, &entwuerfe);
                melden(&zustand, &melde);
            }

            Auftrag::Hochladen {
                nr,
                prozent,
                ergebnis,
            } => {
                if let Some(e) = entwuerfe.iter_mut().find(|e| e.ansicht.nr == nr) {
                    e.ansicht.stand = match ergebnis {
                        None => Entwurfsstand::Laedt { prozent },
                        Some(Ok(())) => Entwurfsstand::Bereit,
                        Some(Err(text)) => Entwurfsstand::Fehler { text },
                    };
                }
                entwurf_zeigen(&zustand, &entwuerfe);
                melden(&zustand, &melde);
            }

            Auftrag::Laden { id, stand } => {
                if stand == Ladestand::Bereit {
                    staende.remove(&id);
                } else {
                    staende.insert(id, stand);
                }
                if let Ok(mut z) = zustand.lock() {
                    abbilden::ladestaende(&mut z, |id| stand_von(&ablage, &staende, id));
                }
                melden(&zustand, &melde);
            }

            Auftrag::Gespeichert { ergebnis } => {
                if let Ok(mut z) = zustand.lock() {
                    z.meldung = Some(match ergebnis {
                        Ok(ziel) => format!("gespeichert: {ziel}"),
                        Err(fehler) => format!("nicht gespeichert: {fehler}"),
                    });
                }
                melden(&zustand, &melde);
            }

            Auftrag::Befehl { befehl, antwort } => {
                // Befehle, die nur das Modell betreffen.
                let nur_modell = match &befehl {
                    Befehl::Vorschau { url } => {
                        if let Ok(mut z) = zustand.lock() {
                            z.entwurf = entwurf_fuer(&z.entwurf, url.as_deref(), |u| {
                                let Some(h) = holer.clone() else {
                                    return false;
                                };
                                let rueck = rueck.clone();
                                let url = u.to_string();
                                tokio::spawn(async move {
                                    let vorschau = h.holen(&url).await.ok();
                                    let _ = rueck.send(Auftrag::Vorschau { url, vorschau });
                                });
                                true
                            });
                        }
                        true
                    }
                    Befehl::AnhangEntfernen { nr } => {
                        entwuerfe.retain(|e| e.ansicht.nr != *nr);
                        entwurf_zeigen(&zustand, &entwuerfe);
                        true
                    }
                    Befehl::BetrachterSchliessen => {
                        if let Ok(mut z) = zustand.lock() {
                            z.betrachter = None;
                        }
                        true
                    }
                    Befehl::VorschauVerwerfen => {
                        if let Ok(mut z) = zustand.lock() {
                            z.entwurf = match &z.entwurf {
                                Linkentwurf::Laedt { url } | Linkentwurf::Ohne { url } => {
                                    Linkentwurf::Ohne { url: url.clone() }
                                }
                                Linkentwurf::Fertig { vorschau } => Linkentwurf::Ohne {
                                    url: vorschau.url.clone(),
                                },
                                Linkentwurf::Keiner => Linkentwurf::Keiner,
                            };
                        }
                        true
                    }
                    _ => false,
                };
                if nur_modell {
                    melden(&zustand, &melde);
                    let _ = antwort.send(Antwort::Ok);
                    continue;
                }
                // Befehle, die ohne Netz beantwortet werden.
                if matches!(befehl, Befehl::Zustand) {
                    let _ = antwort.send(Antwort::Zustand {
                        zustand: zustand.lock().map(|z| z.clone()).unwrap_or_default(),
                    });
                    continue;
                }
                if matches!(befehl, Befehl::Trennen) {
                    if let Some(v) = netz.take() {
                        v.schliessen();
                    }
                    entwuerfe.clear();
                    staende.clear();
                    bund = None;
                    sprache = None;
                    sprech.stoppen();
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, None);
                    }
                    melden(&zustand, &melde);
                    let _ = antwort.send(Antwort::Ok);
                    continue;
                }

                let Some(verbindung) = netz.as_mut() else {
                    let _ = antwort.send(Antwort::Fehler {
                        text: "nicht verbunden".to_string(),
                    });
                    continue;
                };

                // Anhänge: eigene Datenströme, kein Rahmen auf dem Steuerstrom.
                match &befehl {
                    Befehl::AnhaengeHinzufuegen { pfade } => {
                        for pfad in pfade {
                            let nr = naechste_nr;
                            naechste_nr += 1;
                            entwuerfe.push(entwurf_starten(
                                nr,
                                PathBuf::from(pfad),
                                verbindung.leitung(),
                                rueck.clone(),
                            ));
                        }
                        entwurf_zeigen(&zustand, &entwuerfe);
                        melden(&zustand, &melde);
                        let _ = antwort.send(Antwort::Ok);
                        continue;
                    }
                    Befehl::DateiOeffnen { nachricht, datei } => {
                        let ansicht = zustand
                            .lock()
                            .ok()
                            .and_then(|z| abbilden::datei_finden(&z, *nachricht, datei));
                        let Some(ansicht) = ansicht else {
                            let _ = antwort.send(Antwort::Fehler {
                                text: "Datei unbekannt".into(),
                            });
                            continue;
                        };
                        let laedt = matches!(staende.get(datei), Some(Ladestand::Laedt { .. }));
                        if !ablage.liegt_vor(datei) && !laedt {
                            staende.insert(datei.clone(), Ladestand::Laedt { prozent: 0 });
                            laden_starten(
                                datei.clone(),
                                verbindung.leitung(),
                                ablage.clone(),
                                rueck.clone(),
                                None,
                            );
                        }
                        if let Ok(mut z) = zustand.lock() {
                            z.betrachter = Some(sprechwerk_ipc::Betrachter {
                                nachricht: *nachricht,
                                datei: ansicht,
                            });
                            abbilden::ladestaende(&mut z, |id| stand_von(&ablage, &staende, id));
                        }
                        melden(&zustand, &melde);
                        let _ = antwort.send(Antwort::Ok);
                        continue;
                    }
                    Befehl::DateiSpeichern { datei, ziel, .. } => {
                        if ablage.schluessel(datei).is_none() {
                            let _ = antwort.send(Antwort::Fehler {
                                text: "Datei unbekannt".into(),
                            });
                            continue;
                        }
                        if !ablage.liegt_vor(datei) {
                            staende.insert(datei.clone(), Ladestand::Laedt { prozent: 0 });
                        }
                        laden_starten(
                            datei.clone(),
                            verbindung.leitung(),
                            ablage.clone(),
                            rueck.clone(),
                            Some(PathBuf::from(ziel)),
                        );
                        if let Ok(mut z) = zustand.lock() {
                            abbilden::ladestaende(&mut z, |id| stand_von(&ablage, &staende, id));
                        }
                        melden(&zustand, &melde);
                        let _ = antwort.send(Antwort::Ok);
                        continue;
                    }
                    _ => {}
                }

                // Nach dem Senden eines Texts ist der Entwurf leer.
                let mut entwurf_leeren = false;
                let rahmen = match befehl {
                    Befehl::Betreten { raum } => {
                        sprechwerk_protocol::ClientFrame::Betreten { raum }
                    }
                    // Mit Dateien: eine Nachricht der Art `anhang` (ohne Effekt
                    // und Link-Vorschau — die Dateien sind die Vorschau).
                    Befehl::Text { text } if !entwuerfe.is_empty() => {
                        if entwuerfe
                            .iter()
                            .any(|e| e.ansicht.stand != Entwurfsstand::Bereit)
                        {
                            let _ = antwort.send(Antwort::Fehler {
                                text: "Dateien sind noch nicht hochgeladen".into(),
                            });
                            continue;
                        }
                        entwurf_leeren = true;
                        let inhalt = Inhalt::Anhang {
                            text: text.trim_end().to_string(),
                            dateien: entwuerfe.iter().map(datei_im_umschlag).collect(),
                        };
                        let anhaenge = entwuerfe.iter().map(|e| e.id.clone()).collect();
                        match geheim_schreiben(bund.as_ref(), verbindung, &inhalt) {
                            Ok(text) => sprechwerk_protocol::ClientFrame::Text { text, anhaenge },
                            Err(text) => {
                                let _ = antwort.send(Antwort::Fehler { text });
                                continue;
                            }
                        }
                    }
                    Befehl::Text { text } => {
                        // Die Vorschau geht mit, wenn ihr Link noch im Text
                        // steht — weggelöscht heißt: nicht gewollt.
                        let vorschau = zustand.lock().ok().and_then(|z| match &z.entwurf {
                            Linkentwurf::Fertig { vorschau } if text.contains(&vorschau.url) => {
                                Some(vorschau.clone())
                            }
                            _ => None,
                        });
                        entwurf_leeren = true;
                        let inhalt = Inhalt::Text { text, vorschau };
                        match geheim_schreiben(bund.as_ref(), verbindung, &inhalt) {
                            Ok(text) => sprechwerk_protocol::ClientFrame::Text {
                                text,
                                anhaenge: Vec::new(),
                            },
                            Err(text) => {
                                let _ = antwort.send(Antwort::Fehler { text });
                                continue;
                            }
                        }
                    }
                    Befehl::Reagieren { nachricht, zeichen } => {
                        // Schon gesetzt? Dann zurücknehmen — entschieden am
                        // Modell, nicht in der Oberfläche.
                        let weg = zustand
                            .lock()
                            .map(|z| abbilden::meine_reaktion(&z, nachricht, &zeichen))
                            .unwrap_or(false);
                        let inhalt = Inhalt::Reaktion {
                            ziel: nachricht.text(),
                            zeichen,
                            weg,
                        };
                        match geheim_schreiben(bund.as_ref(), verbindung, &inhalt) {
                            Ok(text) => sprechwerk_protocol::ClientFrame::Text {
                                text,
                                anhaenge: Vec::new(),
                            },
                            Err(text) => {
                                let _ = antwort.send(Antwort::Fehler { text });
                                continue;
                            }
                        }
                    }
                    Befehl::NachrichtLoeschen { nachricht } => {
                        sprechwerk_protocol::ClientFrame::NachrichtLoeschen { nachricht }
                    }
                    Befehl::RaumAnlegen { name } => {
                        sprechwerk_protocol::ClientFrame::RaumAnlegen { name }
                    }
                    Befehl::RaumUmbenennen { raum, name } => {
                        sprechwerk_protocol::ClientFrame::RaumUmbenennen { raum, name }
                    }
                    Befehl::RaumLoeschen { raum } => {
                        sprechwerk_protocol::ClientFrame::RaumLoeschen { raum }
                    }
                    Befehl::RolleSetzen { mitglied, rolle } => {
                        sprechwerk_protocol::ClientFrame::RolleSetzen { mitglied, rolle }
                    }
                    Befehl::Stummschalten { mitglied, taub, an } => {
                        sprechwerk_protocol::ClientFrame::Stummschalten { mitglied, taub, an }
                    }
                    Befehl::Verschieben { mitglied, raum } => {
                        sprechwerk_protocol::ClientFrame::Verschieben { mitglied, raum }
                    }
                    Befehl::AusRaumWerfen { mitglied } => {
                        sprechwerk_protocol::ClientFrame::AusRaumWerfen { mitglied }
                    }
                    Befehl::MitgliedTrennen { mitglied, grund } => {
                        sprechwerk_protocol::ClientFrame::Trennen { mitglied, grund }
                    }
                    Befehl::Sperren { mitglied, grund } => {
                        sprechwerk_protocol::ClientFrame::MitgliedSperren { mitglied, grund }
                    }
                    Befehl::Ping => {
                        // Fehlt ein Serverschlüssel, im Takt nachmelden — der
                        // Server bittet dann einen Halter, der online ist.
                        if let Some(meldung) = bund.as_mut().and_then(|b| b.nachmelden()) {
                            let _ = verbindung.sende(meldung).await;
                        }
                        // Zeit **vor** dem Senden stellen: sonst fehlt die
                        // Sendedauer in der Messung.
                        ping_gesendet = Some(std::time::Instant::now());
                        sprechwerk_protocol::ClientFrame::Ping
                    }
                    Befehl::SchluesselAnfordern { raum } => {
                        sprechwerk_protocol::ClientFrame::SchluesselAnfrage { raum }
                    }
                    Befehl::Zaehler => sprechwerk_protocol::ClientFrame::Zaehler,
                    Befehl::Sperrliste => sprechwerk_protocol::ClientFrame::Sperrliste,
                    Befehl::Protokoll { ab, anzahl } => {
                        sprechwerk_protocol::ClientFrame::Protokoll {
                            ab,
                            anzahl,
                            von_hinten: true,
                        }
                    }
                    Befehl::Entsperren { kennung, adresse } => {
                        sprechwerk_protocol::ClientFrame::Entsperren { kennung, adresse }
                    }
                    Befehl::Zustand
                    | Befehl::Verbinden { .. }
                    | Befehl::Trennen
                    | Befehl::Vorschau { .. }
                    | Befehl::VorschauVerwerfen
                    | Befehl::AnhaengeHinzufuegen { .. }
                    | Befehl::AnhangEntfernen { .. }
                    | Befehl::DateiOeffnen { .. }
                    | Befehl::BetrachterSchliessen
                    | Befehl::DateiSpeichern { .. } => unreachable!(),
                };

                match verbindung.sende(rahmen).await {
                    Ok(()) => {
                        if entwurf_leeren {
                            // Verschickt: die Dateien gehören jetzt der
                            // Nachricht (die Aufgaben sind fertig).
                            entwuerfe.clear();
                            if let Ok(mut z) = zustand.lock() {
                                z.entwurf = Linkentwurf::Keiner;
                                z.anhaenge.clear();
                            }
                            melden(&zustand, &melde);
                        }
                        let _ = antwort.send(Antwort::Ok);
                    }
                    Err(fehler) => {
                        let _ = antwort.send(Antwort::Fehler {
                            text: format!("senden: {fehler}"),
                        });
                    }
                }
            }
        }
    }

    if let Some(v) = netz.take() {
        v.schliessen();
    }
}

/// Verschlüsselt ein Sprach-Datagramm, wenn der Raum es verlangt.
///
/// Ohne Schlüssel geht der Rahmen **unverschlüsselt** hinaus — das darf nicht
/// passieren, deshalb wird er dann verworfen.
fn verschluessle(
    verwaltung: &Schluesselverwaltung,
    aktueller: &Arc<Mutex<Option<sprechwerk_domain::RoomId>>>,
    datagramm: Vec<u8>,
) -> Vec<u8> {
    let raum = aktueller.lock().ok().and_then(|r| *r);
    let schluessel = raum.and_then(|r| verwaltung.hole(r).map(|k| (r, k)));
    match schluessel {
        // Ein Rahmen, der sich nicht verschlüsseln lässt, wird verworfen
        // (leeres Feld) — unverschlüsselt hinausgehen wäre ein Leck.
        Some((raum, schluessel)) => sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &datagramm,
            &schluessel,
            raum.text().as_str(),
        )
        .unwrap_or_default(),
        // Kein verschlüsselter Raum: unverändert hinaus.
        None => datagramm,
    }
}

/// Entschlüsselt ein ankommendes Datagramm, wenn es verschlüsselt ist.
fn entschluessle(
    verwaltung: &Schluesselverwaltung,
    aktueller: &Arc<Mutex<Option<sprechwerk_domain::RoomId>>>,
    datagramm: &[u8],
) -> Option<Vec<u8>> {
    use sprechwerk_protocol::{Sprachkopf, CODEC_VERSCHLUESSELT};
    let kopf = Sprachkopf::lesen(datagramm)?.0;
    if kopf.codec != CODEC_VERSCHLUESSELT {
        return Some(datagramm.to_vec());
    }
    let raum = aktueller.lock().ok().and_then(|r| *r)?;
    // Aktueller Schlüssel, sonst der vorige (Übergangsfenster beim Wechsel).
    verwaltung.entschluesseln(raum, datagramm)
}

/// Sorgt für den Raumschlüssel: erzeugen und verteilen (als Wart) oder warten.
///
/// Wird nach jedem Verbinden, jedem Raumwechsel und jedem Mitgliederwechsel
/// aufgerufen — der Wart kann gehen, dann übernimmt der nächste.
async fn verteile_schluessel(
    verbindung: &mut Netz,
    verwaltung: &Schluesselverwaltung,
    eigener: &Schluessel,
) {
    let Some(raum) = verbindung.raum else {
        return;
    };
    let Some(ansicht) = verbindung.raeume.iter().find(|r| r.id == raum) else {
        return;
    };
    if !ansicht.verschluesselt {
        verwaltung.vergiss(raum);
        return;
    }
    let pakete = verwaltung.sorge_fuer(
        raum,
        ansicht.verschluesselt_seit,
        verbindung.ich,
        eigener,
        &verbindung.mitglieder,
    );
    for paket in pakete {
        if let Err(fehler) = verbindung.sende(paket).await {
            eprintln!("Schlüsselpaket nicht gesendet: {fehler}");
        }
    }
}

/// Nimmt ein Schlüsselpaket des Servers an.
fn nimm_schluesselpaket(
    verbindung: &Netz,
    verwaltung: &Schluesselverwaltung,
    eigener: &Schluessel,
    paket: crate::verschluesselung::Paket<'_>,
) {
    if let Err(fehler) = verwaltung.nimm_paket(paket, eigener, &verbindung.mitglieder) {
        eprintln!("Schlüsselpaket abgelehnt: {fehler}");
    }
}

/// Beantwortet die Serverschlüssel-Rahmen (ADR 0012).
async fn serverschluessel_bedienen(
    verbindung: &mut Netz,
    bund: &mut Bund,
    eigener: &Schluessel,
    rahmen: &sprechwerk_protocol::ServerFrame,
) {
    use sprechwerk_protocol::ServerFrame;
    let antworten = match rahmen {
        ServerFrame::Serverschluessel { stand } => bund.stand(stand.clone()),
        ServerFrame::ServerschluesselAnfrage { fuer, kennung, id } => {
            match bund.anfrage(*fuer, kennung, id, eigener) {
                Ok(paket) => vec![paket],
                Err(fehler) => {
                    eprintln!("Serverschlüssel nicht weitergegeben: {fehler}");
                    Vec::new()
                }
            }
        }
        ServerFrame::ServerschluesselPaket {
            von_kennung,
            id,
            daten,
            unterschrift,
            ..
        } => match bund.paket(von_kennung, id, daten, unterschrift, eigener) {
            Ok(meldung) => vec![meldung],
            Err(fehler) => {
                eprintln!("Serverschlüssel-Paket abgelehnt: {fehler}");
                Vec::new()
            }
        },
        _ => Vec::new(),
    };
    for a in antworten.into_iter().chain(bund.nachmelden()) {
        if let Err(fehler) = verbindung.sende(a).await {
            eprintln!("Serverschlüssel: {fehler}");
        }
    }
}

/// Entschlüsselt, was im Modell noch auf seinen Schlüssel wartet, und sagt der
/// Oberfläche, ob geschrieben werden kann.
fn chat_lesen(
    z: &mut Zustand,
    bund: &Bund,
    ablage: &Dateiablage,
    staende: &HashMap<String, Ladestand>,
) {
    use sprechwerk_ipc::NachrichtInhalt;
    abbilden::entschluesseln(z, |n| match bund.lesen(n) {
        Lesbar::Inhalt(inhalt) => {
            // Die Schlüssel der Dateien bleiben hier, im Rust-Teil.
            if let Inhalt::Anhang { dateien, .. } = &inhalt {
                for d in dateien {
                    if let Some(s) = Dateischluessel::aus_hex(&d.schluessel) {
                        ablage.merken(&d.id, s, abbilden::art_und_typ(&d.typ).1);
                    }
                }
            }
            abbilden::inhalt_abbilden(inhalt)
        }
        Lesbar::Unbekannt { art } => NachrichtInhalt::Neuer { art },
        // Bleibt „wartet" und wird beim nächsten Mal erneut versucht.
        Lesbar::SchluesselFehlt => NachrichtInhalt::Wartet,
        Lesbar::Unlesbar => NachrichtInhalt::Unlesbar,
        Lesbar::Leer => NachrichtInhalt::Geloescht,
    });
    abbilden::reaktionen_zaehlen(z);
    abbilden::ladestaende(z, |id| stand_von(ablage, staende, id));
    z.textschluessel = if bund.kann_schreiben() {
        sprechwerk_ipc::Textschluessel::Bereit
    } else {
        sprechwerk_ipc::Textschluessel::Wartet
    };
}

/// Verschlüsselt einen Inhalt für den aktuellen Raum (ADR 0012): ohne
/// Schlüssel geht nichts hinaus — schon gar nicht im Klartext.
fn geheim_schreiben(
    bund: Option<&Bund>,
    verbindung: &Netz,
    inhalt: &Inhalt,
) -> Result<String, String> {
    match (bund, verbindung.raum) {
        (Some(b), Some(raum)) => b
            .verschluesseln(raum, verbindung.ich, inhalt)
            .map_err(|f| f.to_string()),
        _ => Err("in keinem Raum".to_string()),
    }
}

/// Der nächste Stand der Link-Vorschau, wenn sich der erste Link im Entwurf
/// ändert. `abrufen` startet den Abruf und sagt, ob das ging.
///
/// Derselbe Link wird nicht zweimal geholt — auch nicht, wenn man ihn
/// weggeklickt hat (`Ohne`): sonst käme die Vorschau beim nächsten Tastendruck
/// wieder.
fn entwurf_fuer(
    jetzt: &Linkentwurf,
    url: Option<&str>,
    abrufen: impl FnOnce(&str) -> bool,
) -> Linkentwurf {
    let Some(url) = url.filter(|u| abbilden::ist_weblink(u)) else {
        return Linkentwurf::Keiner;
    };
    let gleich = match jetzt {
        Linkentwurf::Laedt { url: u } | Linkentwurf::Ohne { url: u } => u == url,
        Linkentwurf::Fertig { vorschau } => vorschau.url == url,
        Linkentwurf::Keiner => false,
    };
    if gleich {
        return jetzt.clone();
    }
    if abrufen(url) {
        Linkentwurf::Laedt {
            url: url.to_string(),
        }
    } else {
        Linkentwurf::Ohne {
            url: url.to_string(),
        }
    }
}

/// Ladestand einer Datei: was gerade läuft, sonst was im Zwischenspeicher
/// liegt.
fn stand_von(ablage: &Dateiablage, staende: &HashMap<String, Ladestand>, id: &str) -> Ladestand {
    match staende.get(id) {
        Some(s) => s.clone(),
        None if ablage.liegt_vor(id) => Ladestand::Bereit,
        None => Ladestand::Keiner,
    }
}

/// Der Entwurf ins Modell.
fn entwurf_zeigen(zustand: &Arc<Mutex<Zustand>>, entwuerfe: &[Entwurfsdatei]) {
    if let Ok(mut z) = zustand.lock() {
        z.anhaenge = entwuerfe.iter().map(|e| e.ansicht.clone()).collect();
    }
}

/// Eine Datei, wie sie im Umschlag steht — mit ihrem Schlüssel.
fn datei_im_umschlag(e: &Entwurfsdatei) -> Datei {
    Datei {
        id: e.id.clone(),
        name: e.ansicht.name.clone(),
        typ: e.typ.to_string(),
        groesse: e.ansicht.groesse,
        schluessel: e.schluessel.hex(),
        vorschau: e.ansicht.vorschau.clone(),
        breite: e.breite,
        hoehe: e.hoehe,
    }
}

fn prozent(teil: u64, ganz: u64) -> u8 {
    teil.saturating_mul(100)
        .checked_div(ganz)
        .map_or(100, |p| p.min(100) as u8)
}

/// Nimmt eine Datei in den Entwurf: Vorschau rechnen, verschlüsselt
/// hochladen. Alles in einer Aufgabe — das Fenster wartet auf nichts.
fn entwurf_starten(
    nr: u64,
    pfad: PathBuf,
    leitung: sprechwerk_adapters::Leitung,
    rueck: mpsc::UnboundedSender<Auftrag>,
) -> Entwurfsdatei {
    let roh = pfad
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = abbilden::dateiname(&roh);
    let typ = anhang::typ_aus_name(&name);
    let art = abbilden::art_und_typ(typ).0;
    let groesse = std::fs::metadata(&pfad)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len());
    let id = anhang::neue_id();
    let schluessel = Dateischluessel::neu();
    let mut ansicht = Anhangentwurf {
        nr,
        name,
        art,
        groesse: groesse.unwrap_or(0),
        vorschau: None,
        stand: Entwurfsstand::Vorbereitet,
    };
    let Some(groesse) = groesse else {
        ansicht.stand = Entwurfsstand::Fehler {
            text: "keine lesbare Datei".into(),
        };
        return Entwurfsdatei {
            id,
            schluessel,
            typ,
            breite: None,
            hoehe: None,
            ansicht,
            aufgabe: None,
        };
    };
    let (a_id, a_s) = (id.clone(), schluessel.clone());
    let aufgabe = tokio::spawn(async move {
        let mut vorschau = None;
        if art == Dateiart::Bild && groesse <= ANHANG_VORSCHAU_HOECHSTENS {
            if let Ok(bytes) = tokio::fs::read(&pfad).await {
                vorschau = tokio::task::spawn_blocking(move || {
                    sprechwerk_linkvorschau::bild::vorschaubild(
                        &bytes,
                        ANHANG_KANTE,
                        ANHANG_VORSCHAU_BYTES,
                    )
                })
                .await
                .ok()
                .flatten()
                .map(|v| (sprechwerk_linkvorschau::base64(&v.jpeg), v.breite, v.hoehe));
            }
        }
        let _ = rueck.send(Auftrag::Vorbereitet { nr, vorschau });
        let ergebnis = match tokio::fs::File::open(&pfad).await {
            Ok(datei) => {
                let mut zuletzt = 0;
                let rueck_f = rueck.clone();
                anhang::hochladen(&leitung, &a_id, &a_s, datei, groesse, move |n| {
                    let p = prozent(n, groesse);
                    if p != zuletzt {
                        zuletzt = p;
                        let _ = rueck_f.send(Auftrag::Hochladen {
                            nr,
                            prozent: p,
                            ergebnis: None,
                        });
                    }
                })
                .await
                .map_err(|f| format!("{f:#}"))
            }
            Err(f) => Err(f.to_string()),
        };
        let _ = rueck.send(Auftrag::Hochladen {
            nr,
            prozent: 100,
            ergebnis: Some(ergebnis),
        });
    });
    Entwurfsdatei {
        id,
        schluessel,
        typ,
        breite: None,
        hoehe: None,
        ansicht,
        aufgabe: Some(aufgabe),
    }
}

/// Lädt eine Datei in den Zwischenspeicher (falls nötig) und speichert sie,
/// wenn `ziel` gegeben ist, entschlüsselt dorthin.
fn laden_starten(
    id: String,
    leitung: sprechwerk_adapters::Leitung,
    ablage: Arc<Dateiablage>,
    rueck: mpsc::UnboundedSender<Auftrag>,
    ziel: Option<PathBuf>,
) {
    tokio::spawn(async move {
        if !ablage.liegt_vor(&id) {
            let mut zuletzt = 0;
            let rueck_f = rueck.clone();
            let id_f = id.clone();
            let geladen = anhang::herunterladen(&leitung, &id, &ablage.pfad(&id), move |n, g| {
                let p = prozent(n, g);
                if p != zuletzt {
                    zuletzt = p;
                    let _ = rueck_f.send(Auftrag::Laden {
                        id: id_f.clone(),
                        stand: Ladestand::Laedt { prozent: p },
                    });
                }
            })
            .await;
            if let Err(fehler) = geladen {
                let text = format!("{fehler:#}");
                let _ = rueck.send(Auftrag::Laden {
                    id,
                    stand: Ladestand::Fehler { text: text.clone() },
                });
                if ziel.is_some() {
                    let _ = rueck.send(Auftrag::Gespeichert {
                        ergebnis: Err(text),
                    });
                }
                return;
            }
            let _ = rueck.send(Auftrag::Laden {
                id: id.clone(),
                stand: Ladestand::Bereit,
            });
        }
        let Some(ziel) = ziel else {
            return;
        };
        let Some(schluessel) = ablage.schluessel(&id) else {
            let _ = rueck.send(Auftrag::Gespeichert {
                ergebnis: Err("Schlüssel fehlt".into()),
            });
            return;
        };
        let quelle = ablage.pfad(&id);
        let ziel_text = ziel.display().to_string();
        let ablage_b = ablage.clone();
        let id_b = id.clone();
        let ergebnis = tokio::task::spawn_blocking(move || {
            let e = anhang::entschluesseln_in_datei(&quelle, schluessel, &id_b, &ziel);
            if e.is_err() {
                // Verändert: beim nächsten Mal neu laden.
                ablage_b.vergessen(&id_b);
            }
            e
        })
        .await
        .map_err(|f| f.to_string())
        .and_then(|e| e.map_err(|f| format!("{f:#}")));
        let _ = rueck.send(Auftrag::Gespeichert {
            ergebnis: ergebnis.map(|()| ziel_text),
        });
    });
}

fn melden<F: Fn(Zustand)>(zustand: &Arc<Mutex<Zustand>>, melde: &F) {
    if let Ok(z) = zustand.lock() {
        melde(z.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freier_port() -> u16 {
        std::net::UdpSocket::bind("127.0.0.1:0")
            .expect("Port")
            .local_addr()
            .expect("Adresse")
            .port()
    }

    /// Startet den echten Server aus dem Arbeitsbereich.
    ///
    /// Das Programm liegt in `crates/server` (`cargo test --workspace` baut es
    /// mit); fehlt es, sagt der Test, was zu tun ist, statt still zu
    /// überspringen.
    struct Pruefserver {
        kind: std::process::Child,
        verzeichnis: std::path::PathBuf,
        adresse: std::net::SocketAddr,
    }

    impl Drop for Pruefserver {
        fn drop(&mut self) {
            let _ = self.kind.kill();
            let _ = self.kind.wait();
            let _ = std::fs::remove_dir_all(&self.verzeichnis);
        }
    }

    fn server_starten(name: &str) -> Pruefserver {
        let programm =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/sprechwerk");
        assert!(
            programm.exists(),
            "{} fehlt — vorher `cargo build -p sprechwerk-server` im Arbeitsbereich ausführen",
            programm.display()
        );
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-anwendung-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        std::fs::create_dir_all(&verzeichnis).expect("Verzeichnis");
        let port = freier_port();
        let mut kind = std::process::Command::new(&programm)
            .env("SPRECHWERK_DATEN", &verzeichnis)
            .env("SPRECHWERK_PORT", port.to_string())
            .env("SPRECHWERK_BIND", "127.0.0.1")
            .env("SPRECHWERK_NAME", "Anwendungstest")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            // Keine Verwaltung (Port 7444, Browser) für Testserver.
            .env("SPRECHWERK_VERWALTUNG_AKTIV", "0")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Server startet");

        // Auf „lauscht auf" warten; danach die Ausgabe weiterlesen, sonst
        // stirbt der Server am nächsten `println!` (Broken pipe).
        {
            use std::io::BufRead;
            let stdout = kind.stdout.take().expect("stdout");
            let mut leser = std::io::BufReader::new(stdout);
            let mut bereit = false;
            for _ in 0..200 {
                let mut zeile = String::new();
                if leser.read_line(&mut zeile).unwrap_or(0) == 0 {
                    break;
                }
                if zeile.contains("lauscht auf") {
                    bereit = true;
                    break;
                }
            }
            assert!(bereit, "Server meldet sich nicht");
            std::thread::spawn(move || {
                let mut rest = String::new();
                let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
            });
        }

        Pruefserver {
            kind,
            verzeichnis,
            adresse: format!("127.0.0.1:{port}").parse().expect("Adresse"),
        }
    }

    impl Pruefserver {
        fn fingerabdruck(&self) -> String {
            let der = std::fs::read(self.verzeichnis.join("zertifikat.der"))
                .expect("Zertifikat im Datenverzeichnis");
            sprechwerk_protocol::zertifikat_fingerabdruck(&der)
        }
    }

    fn anwendung_fuer(name: &str) -> (Anwendung, std::path::PathBuf) {
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-anwendung-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        let anwendung = Anwendung::starten(
            verzeichnis.join("schluessel"),
            Arc::new(crate::sprech::Sprechbetrieb::neu()),
            Arc::new(crate::verschluesselung::Schluesselverwaltung::neu()),
            Arc::new(Dateiablage::neu(verzeichnis.join("anhaenge"))),
            |_| {},
        );
        (anwendung, verzeichnis)
    }

    async fn bis(frist_s: u64, was: &str, bedingung: impl Fn() -> bool) {
        let ende = std::time::Instant::now() + std::time::Duration::from_secs(frist_s);
        while !bedingung() {
            assert!(
                std::time::Instant::now() < ende,
                "Zeitüberschreitung: {was}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Chat Ende zu Ende, vom Fenster bis zum Fenster (ADR 0012): Ada legt den
    /// Serverschlüssel an, Bosse bekommt ihn von Ada, schreibt — Ada liest den
    /// Klartext, in der Datenbank des Servers steht er nicht.
    /// Eine Seite mit Titel, ausgeliefert von einem winzigen HTTP-Server.
    async fn kleine_seite() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let hoerer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let adresse = format!("http://{}/heim", hoerer.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut s, _)) = hoerer.accept().await {
                let mut puffer = [0u8; 2048];
                let _ = s.read(&mut puffer).await;
                let koerper = r#"<meta property="og:title" content="Vereinsheim">"#;
                let antwort = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{koerper}",
                    koerper.len()
                );
                let _ = s.write_all(antwort.as_bytes()).await;
            }
        });
        adresse
    }

    #[test]
    fn entwurf_holt_jeden_link_nur_einmal() {
        let geholt = std::cell::Cell::new(0);
        let holen = |_: &str| {
            geholt.set(geholt.get() + 1);
            true
        };
        let a = "https://example.org/a";
        let laedt = entwurf_fuer(&Linkentwurf::Keiner, Some(a), holen);
        assert_eq!(laedt, Linkentwurf::Laedt { url: a.into() });
        // Derselbe Link beim nächsten Tastendruck: nicht noch einmal.
        let holen = |_: &str| {
            geholt.set(geholt.get() + 1);
            true
        };
        assert_eq!(entwurf_fuer(&laedt, Some(a), holen), laedt);
        // Weggeklickt bleibt weggeklickt.
        let ohne = Linkentwurf::Ohne { url: a.into() };
        assert_eq!(entwurf_fuer(&ohne, Some(a), |_| unreachable!()), ohne);
        // Kein Link, oder keiner, den man abrufen darf: nichts.
        for nein in [None, Some("javascript:alert(1)"), Some("example.org")] {
            assert_eq!(
                entwurf_fuer(&laedt, nein, |_| unreachable!()),
                Linkentwurf::Keiner
            );
        }
        // Abruf nicht möglich (kein Holer): ohne Vorschau.
        assert_eq!(
            entwurf_fuer(&Linkentwurf::Keiner, Some(a), |_| false),
            Linkentwurf::Ohne { url: a.into() }
        );
        assert_eq!(geholt.get(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chat_ist_ende_zu_ende_verschluesselt() {
        use sprechwerk_ipc::{NachrichtInhalt, Textschluessel};
        let server = server_starten("chat-e2e");
        let (ada, ada_dir) = anwendung_fuer("chat-ada");
        let (bosse, bosse_dir) = anwendung_fuer("chat-bosse");
        let adresse = server.adresse.to_string();

        ada.verbinden(adresse.clone(), "Ada".into(), None, None)
            .await
            .expect("Ada");
        bis(5, "Ada hat den Schlüssel", || {
            ada.zustand().textschluessel == Textschluessel::Bereit
        })
        .await;
        bosse
            .verbinden(adresse, "Bosse".into(), None, None)
            .await
            .expect("Bosse");
        bis(5, "Bosse bekommt den Schlüssel von Ada", || {
            bosse.zustand().textschluessel == Textschluessel::Bereit
        })
        .await;

        let antwort = bosse
            .befehl(Befehl::Text {
                text: "Kassenstand: 1312 Euro".into(),
            })
            .await;
        assert!(matches!(antwort, Antwort::Ok), "{antwort:?}");
        bis(5, "Ada liest Bosses Nachricht", || {
            ada.zustand().nachrichten.iter().any(|n| {
                n.inhalt
                    == NachrichtInhalt::Text {
                        text: "Kassenstand: 1312 Euro".into(),
                        vorschau: None,
                    }
            })
        })
        .await;
        // Ada reagiert — Bosse sieht es an seiner Nachricht, nicht als Beitrag.
        let ziel = ada
            .zustand()
            .nachrichten
            .iter()
            .find(|n| n.verfasser_name == "Bosse")
            .map(|n| n.id)
            .expect("Bosses Nachricht bei Ada");
        let antwort = ada
            .befehl(Befehl::Reagieren {
                nachricht: ziel,
                zeichen: "👍".into(),
            })
            .await;
        assert!(matches!(antwort, Antwort::Ok), "{antwort:?}");
        let reaktionen = |a: &Anwendung| {
            a.zustand()
                .nachrichten
                .iter()
                .find(|n| n.id == ziel)
                .map(|n| n.reaktionen.clone())
                .unwrap_or_default()
        };
        bis(5, "Bosse sieht Adas Reaktion", || {
            reaktionen(&bosse)
                .first()
                .is_some_and(|r| r.zeichen == "👍" && r.anzahl == 1 && !r.ich)
        })
        .await;
        bis(5, "Ada sieht ihre eigene", || {
            reaktionen(&ada).first().is_some_and(|r| r.ich)
        })
        .await;
        // Noch einmal: zurückgenommen.
        ada.befehl(Befehl::Reagieren {
            nachricht: ziel,
            zeichen: "👍".into(),
        })
        .await;
        bis(5, "Reaktion ist zurückgenommen", || {
            reaktionen(&bosse).is_empty() && reaktionen(&ada).is_empty()
        })
        .await;

        // Link-Vorschau: Ada ruft ab, Bosse bekommt sie mit der Nachricht.
        let seite = kleine_seite().await;
        ada.befehl(Befehl::Vorschau {
            url: Some(seite.clone()),
        })
        .await;
        bis(5, "Vorschau ist abgerufen", || {
            matches!(ada.zustand().entwurf, Linkentwurf::Fertig { .. })
        })
        .await;
        ada.befehl(Befehl::Text {
            text: format!("Hier: {seite}"),
        })
        .await;
        assert_eq!(ada.zustand().entwurf, Linkentwurf::Keiner);
        bis(5, "Bosse sieht die Vorschau", || {
            bosse.zustand().nachrichten.iter().any(|n| {
                matches!(&n.inhalt, NachrichtInhalt::Text { vorschau: Some(v), .. }
                    if v.titel == "Vereinsheim" && v.url == seite)
            })
        })
        .await;

        // Der Server hat nur Geheimtext.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        for datei in std::fs::read_dir(&server.verzeichnis).unwrap().flatten() {
            if let Ok(bytes) = std::fs::read(datei.path()) {
                assert!(
                    !bytes.windows(10).any(|w| w == b"Kassenstan")
                        && !bytes.windows(11).any(|w| w == b"Vereinsheim"),
                    "Klartext in {}",
                    datei.path().display()
                );
            }
        }
        // Die Schlüssel liegen beim Client, nur für den Eigentümer lesbar.
        let datei = bosse_dir.join("serverschluessel.json");
        assert!(datei.exists(), "Bosse merkt sich den Schlüssel");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let rechte = std::fs::metadata(&datei).unwrap().permissions().mode() & 0o777;
            assert_eq!(rechte, 0o600);
        }
        let _ = std::fs::remove_dir_all(ada_dir);
        let _ = std::fs::remove_dir_all(bosse_dir);
    }

    /// Anhänge vom Fenster bis zum Fenster (ADR 0012 §3): Ada hängt eine Datei
    /// an, sie lädt verschlüsselt hoch; Bosse sieht sie, öffnet und speichert
    /// sie entschlüsselt. Beim Server liegt weder Name noch Inhalt.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn anhaenge_gehen_ende_zu_ende() {
        use sprechwerk_ipc::{NachrichtInhalt, Textschluessel};
        let server = server_starten("anhang-e2e");
        let (ada, ada_dir) = anwendung_fuer("anhang-ada");
        let (bosse, bosse_dir) = anwendung_fuer("anhang-bosse");
        let adresse = server.adresse.to_string();
        ada.verbinden(adresse.clone(), "Ada".into(), None, None)
            .await
            .expect("Ada");
        bis(5, "Ada hat den Schlüssel", || {
            ada.zustand().textschluessel == Textschluessel::Bereit
        })
        .await;
        bosse
            .verbinden(adresse, "Bosse".into(), None, None)
            .await
            .expect("Bosse");
        bis(5, "Bosse hat den Schlüssel", || {
            bosse.zustand().textschluessel == Textschluessel::Bereit
        })
        .await;

        // Eine Datei über mehrere Blöcke, mit Merkwort.
        std::fs::create_dir_all(&ada_dir).unwrap();
        let quelle = ada_dir.join("Kassenbericht 2026.txt");
        let mut inhalt = b"Kassenbericht-geheim ".repeat(10_000);
        inhalt.extend_from_slice(b"Ende");
        std::fs::write(&quelle, &inhalt).unwrap();

        // Ohne fertigen Upload geht nichts hinaus.
        ada.befehl(Befehl::AnhaengeHinzufuegen {
            pfade: vec![
                quelle.display().to_string(),
                ada_dir.join("gibt-es-nicht").display().to_string(),
            ],
        })
        .await;
        bis(5, "Entwurf zeigt beide", || {
            ada.zustand().anhaenge.len() == 2
        })
        .await;
        bis(5, "Upload ist fertig", || {
            ada.zustand().anhaenge[0].stand == Entwurfsstand::Bereit
        })
        .await;
        assert!(matches!(
            ada.zustand().anhaenge[1].stand,
            Entwurfsstand::Fehler { .. }
        ));
        // Mit der kaputten Datei geht nichts hinaus — sie muss weg.
        assert!(matches!(
            ada.befehl(Befehl::Text {
                text: "Bericht".into(),
            })
            .await,
            Antwort::Fehler { .. }
        ));
        let nr = ada.zustand().anhaenge[1].nr;
        ada.befehl(Befehl::AnhangEntfernen { nr }).await;
        assert_eq!(ada.zustand().anhaenge.len(), 1);
        let antwort = ada
            .befehl(Befehl::Text {
                text: "Bericht anbei".into(),
            })
            .await;
        assert!(matches!(antwort, Antwort::Ok), "{antwort:?}");
        assert!(ada.zustand().anhaenge.is_empty(), "Entwurf ist leer");

        // Bosse sieht die Datei — ohne Schlüssel im Modell.
        bis(5, "Bosse sieht den Anhang", || {
            bosse.zustand().nachrichten.iter().any(|n| {
                matches!(&n.inhalt, NachrichtInhalt::Anhang { text, dateien }
                    if text == "Bericht anbei" && dateien[0].name == "Kassenbericht 2026.txt"
                        && dateien[0].art == Dateiart::Datei
                        && dateien[0].groesse == inhalt.len() as u64)
            })
        })
        .await;
        let (nachricht, datei) = bosse
            .zustand()
            .nachrichten
            .iter()
            .find_map(|n| match &n.inhalt {
                NachrichtInhalt::Anhang { dateien, .. } => Some((n.id, dateien[0].id.clone())),
                _ => None,
            })
            .unwrap();
        let ansicht = bosse
            .zustand()
            .nachrichten
            .iter()
            .find(|n| n.id == nachricht)
            .map(|n| serde_json::to_string(&n.inhalt).unwrap())
            .unwrap();
        assert!(
            !ansicht.contains("schluessel"),
            "kein Schlüssel im Modell: {ansicht}"
        );

        // Speichern: geladen, entschlüsselt, am Ziel.
        let ziel = bosse_dir.join("gespeichert.txt");
        let antwort = bosse
            .befehl(Befehl::DateiSpeichern {
                nachricht,
                datei: datei.clone(),
                ziel: ziel.display().to_string(),
            })
            .await;
        assert!(matches!(antwort, Antwort::Ok), "{antwort:?}");
        bis(5, "Datei ist gespeichert", || {
            std::fs::read(&ziel).is_ok_and(|b| b == inhalt)
        })
        .await;
        bis(5, "Meldung sagt es", || {
            bosse
                .zustand()
                .meldung
                .is_some_and(|m| m.starts_with("gespeichert"))
        })
        .await;

        // Öffnen: liegt schon vor, der Betrachter steht im Modell.
        bosse
            .befehl(Befehl::DateiOeffnen {
                nachricht,
                datei: datei.clone(),
            })
            .await;
        let b = bosse.zustand().betrachter.expect("Betrachter");
        assert_eq!(b.datei.stand, sprechwerk_ipc::Ladestand::Bereit);
        bosse.befehl(Befehl::BetrachterSchliessen).await;
        assert!(bosse.zustand().betrachter.is_none());

        // Beim Server: weder Merkwort noch Dateiname.
        for datei in walk(&server.verzeichnis) {
            if let Ok(bytes) = std::fs::read(&datei) {
                assert!(
                    !bytes.windows(20).any(|w| w == b"Kassenbericht-geheim")
                        && !bytes.windows(13).any(|w| w == b"Kassenbericht"),
                    "Klartext in {}",
                    datei.display()
                );
            }
        }
        let _ = std::fs::remove_dir_all(ada_dir);
        let _ = std::fs::remove_dir_all(bosse_dir);
    }

    fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut aus = Vec::new();
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                aus.extend(walk(&p));
            } else {
                aus.push(p);
            }
        }
        aus
    }

    /// Die ganze Kette vom Fenster bis zum Handschlag: ein angehefteter Wert
    /// entscheidet, ob die Verbindung zustande kommt — und der gesehene Wert
    /// landet im Modell, damit die Oberfläche ihn zeigen kann.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn angeheftes_zertifikat_entscheidet_ueber_die_verbindung() {
        let server = server_starten("zertifikat");
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-anwendung-schluessel-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        let anwendung = Anwendung::starten(
            verzeichnis.join("schluessel"),
            Arc::new(crate::sprech::Sprechbetrieb::neu()),
            Arc::new(crate::verschluesselung::Schluesselverwaltung::neu()),
            Arc::new(Dateiablage::neu(verzeichnis.join("anhaenge"))),
            |_| {},
        );

        // Falscher Wert: die Verbindung darf nicht zustande kommen, und der
        // Grund muss ankommen (nicht „TLS-Fehler").
        let falsch = sprechwerk_protocol::zertifikat_fingerabdruck(b"boeser-server");
        let fehler = anwendung
            .verbinden(server.adresse.to_string(), "Ada".into(), None, Some(falsch))
            .await
            .expect_err("fremdes Zertifikat");
        assert!(
            fehler.contains("anderes Zertifikat"),
            "Meldung nennt den Grund: {fehler}"
        );

        // Richtiger Wert: Verbindung steht, und der geprüfte Wert steht im
        // Modell — sonst könnte die Oberfläche ihn nicht anzeigen.
        let richtig = server.fingerabdruck();
        anwendung
            .verbinden(
                server.adresse.to_string(),
                "Ada".into(),
                None,
                Some(richtig.clone()),
            )
            .await
            .expect("passendes Zertifikat");
        assert_eq!(
            anwendung.zustand().zertifikat,
            Some(richtig),
            "der geprüfte Fingerabdruck gehört ins Modell"
        );

        let _ = std::fs::remove_dir_all(&verzeichnis);
    }
}

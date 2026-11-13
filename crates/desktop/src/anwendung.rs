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

use crate::sprech::Sprechbetrieb;
use crate::verschluesselung::Schluesselverwaltung;
use sprechwerk_adapters::{Schluessel, Verbindung as Netz};
use sprechwerk_ipc::{abbilden, Antwort, Befehl, Zustand};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// Ein Auftrag des Fensters an den Verbindungsthread.
enum Auftrag {
    Verbinden {
        server: String,
        name: String,
        passwort: Option<String>,
        antwort: oneshot::Sender<Result<String, String>>,
    },
    Befehl {
        befehl: Befehl,
        antwort: oneshot::Sender<Antwort>,
    },
}

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
        melde: F,
    ) -> Self
    where
        F: Fn(Zustand) + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
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
                    zustand_thread,
                    sprech_thread,
                    verwaltung,
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
    ) -> Result<String, String> {
        let (tx, rx) = oneshot::channel();
        self.auftrag
            .send(Auftrag::Verbinden {
                server,
                name,
                passwort,
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

/// Die Schleife: Aufträge annehmen, Rahmen des Servers verarbeiten.
async fn lauf<F>(
    mut rx: mpsc::UnboundedReceiver<Auftrag>,
    zustand: Arc<Mutex<Zustand>>,
    sprech: Arc<Sprechbetrieb>,
    verwaltung: Arc<Schluesselverwaltung>,
    schluessel_pfad: PathBuf,
    melde: F,
) where
    F: Fn(Zustand) + Send + 'static,
{
    let mut netz: Option<Netz> = None;
    // Der eigene Anmeldeschlüssel — nötig, um Raumschlüssel zu verteilen und
    // Umschläge zu öffnen.
    let mut eigener: Option<Schluessel> = None;
    // Kommende Sprache aus dem Netz (eigene Aufgabe, eigener Pfad).
    let mut sprache: Option<mpsc::UnboundedReceiver<Vec<u8>>> = None;
    // Raum, in dem wir gerade sitzen — für Ver- und Entschlüsselung im
    // Netzrand (die Aufgaben dort kennen die Verbindung nicht).
    let aktueller: Arc<Mutex<Option<sprechwerk_domain::RoomId>>> = Arc::new(Mutex::new(None));

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
                    // Jemand bittet um den Schlüssel: die Verteilung neu
                    // anstoßen, damit der nächste Durchlauf ihn bedient.
                    if let sprechwerk_protocol::ServerFrame::SchluesselAnfrage { raum, .. } = &frame
                    {
                        verwaltung.verteilung_erneuern(*raum);
                    }
                    let mut raum_wechsel = false;
                    if let Ok(mut z) = zustand.lock() {
                        let vorher = z.aktueller_raum;
                        abbilden::verarbeite(&mut z, frame);
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
                    let _ = raum_wechsel;
                }
                Ok(None) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, None);
                        neu_melden = true;
                    }
                    netz = None;
                    sprache = None;
                    sprech.stoppen();
                }
                Err(fehler) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, Some(format!("Verbindung beendet: {fehler}")));
                        neu_melden = true;
                    }
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
                                    abbilden::getrennt(&mut z, Some(format!("{server} nicht auflösbar")));
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

                match Netz::oeffnen(ziel, &schluessel, &name, passwort.as_deref()).await {
                    Ok(verbindung) => {
                        let server_name = verbindung.server_name.clone();
                        let raeume = verbindung.raeume.clone();
                        let mitglieder = verbindung.mitglieder.clone();
                        let raum = verbindung.raum;
                        let verlauf = verbindung.verlauf.clone();
                        let ich = verbindung.ich;
                        let rolle = verbindung.rolle;

                        // Sprache: eingehende Datagramme in einer eigenen Aufgabe
                        // lesen (der Steuerstrom gehört der Leseschleife oben).
                        let quelle = verbindung.datagramm_quelle();
                        let (tx_sprache, rx_sprache) = mpsc::unbounded_channel::<Vec<u8>>();
                        tokio::spawn(async move {
                            loop {
                                match quelle.read_datagram().await {
                                    Ok(d) => {
                                        if tx_sprache.send(d.to_vec()).is_err() {
                                            break;
                                        }
                                    }
                                    // Verbindung zu: Aufgabe beenden.
                                    Err(_) => break,
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
                                    let d = verschluessle(
                                        &sende_verwaltung,
                                        &sende_raum,
                                        d,
                                    );
                                    if sendequelle.send_datagram(bytes::Bytes::from(d)).is_err() {
                                        return;
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            }
                        });

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
                                },
                            );
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

            Auftrag::Befehl { befehl, antwort } => {
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

                let rahmen = match befehl {
                    Befehl::Betreten { raum } => {
                        sprechwerk_protocol::ClientFrame::Betreten { raum }
                    }
                    Befehl::Text { text } => sprechwerk_protocol::ClientFrame::Text { text },
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
                    Befehl::RaumVerschluesseln { raum, an } => {
                        sprechwerk_protocol::ClientFrame::RaumVerschluesseln { raum, an }
                    }
                    Befehl::Sperren { mitglied, grund } => {
                        sprechwerk_protocol::ClientFrame::MitgliedSperren { mitglied, grund }
                    }
                    Befehl::Protokoll { ab, anzahl } => {
                        sprechwerk_protocol::ClientFrame::Protokoll { ab, anzahl }
                    }
                    Befehl::Entsperren { kennung, adresse } => {
                        sprechwerk_protocol::ClientFrame::Entsperren { kennung, adresse }
                    }
                    Befehl::Zustand | Befehl::Verbinden { .. } | Befehl::Trennen => unreachable!(),
                };

                match verbindung.sende(rahmen).await {
                    Ok(()) => {
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
    match (raum, raum.and_then(|r| verwaltung.hole(r).map(|k| (r, k)))) {
        (_, Some((raum, schluessel))) => {
            match sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
                &datagramm,
                &schluessel,
                raum.text().as_str(),
            ) {
                Ok(geheim) => geheim,
                Err(_) => Vec::new(),
            }
        }
        _ => {
            // Kein verschlüsselter Raum: unverändert hinaus.
            datagramm
        }
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

fn melden<F: Fn(Zustand)>(zustand: &Arc<Mutex<Zustand>>, melde: &F) {
    if let Ok(z) = zustand.lock() {
        melde(z.clone());
    }
}

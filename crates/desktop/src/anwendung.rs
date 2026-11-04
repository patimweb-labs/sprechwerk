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
}

impl Anwendung {
    /// Startet den Verbindungsthread.
    ///
    /// `melde` wird nach jeder Änderung mit dem vollständigen Stand aufgerufen
    /// (im Fenster: Ereignis `zustand`).
    pub fn starten<F>(schluessel_pfad: PathBuf, melde: F) -> Self
    where
        F: Fn(Zustand) + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let zustand = Arc::new(Mutex::new(Zustand::neu()));

        let zustand_thread = zustand.clone();
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
                laufzeit.block_on(lauf(rx, zustand_thread, schluessel_pfad, melde));
            })
            .expect("Verbindungsthread");

        Self { auftrag: tx, zustand }
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
    schluessel_pfad: PathBuf,
    melde: F,
) where
    F: Fn(Zustand) + Send + 'static,
{
    let mut netz: Option<Netz> = None;

    loop {
        let mut auftrag: Option<Auftrag> = None;
        let mut rahmen: Option<anyhow::Result<Option<sprechwerk_protocol::ServerFrame>>> = None;

        match netz.as_mut() {
            Some(verbindung) => {
                tokio::select! {
                    a = rx.recv() => auftrag = a,
                    f = verbindung.empfang() => rahmen = Some(f),
                }
            }
            None => auftrag = rx.recv().await,
        }

        // ---- Rahmen des Servers ----------------------------------------
        if let Some(ergebnis) = rahmen {
            let mut neu_melden = false;
            match ergebnis {
                Ok(Some(frame)) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::verarbeite(&mut z, frame);
                        neu_melden = true;
                    }
                }
                Ok(None) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, None);
                        neu_melden = true;
                    }
                    netz = None;
                }
                Err(fehler) => {
                    if let Ok(mut z) = zustand.lock() {
                        abbilden::getrennt(&mut z, Some(format!("Verbindung beendet: {fehler}")));
                        neu_melden = true;
                    }
                    netz = None;
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

                match Netz::oeffnen(ziel, &schluessel, &name, passwort.as_deref()).await {
                    Ok(verbindung) => {
                        let server_name = verbindung.server_name.clone();
                        let raeume = verbindung.raeume.clone();
                        let mitglieder = verbindung.mitglieder.clone();
                        let ich = verbindung.ich;
                        let rolle = verbindung.rolle;
                        netz = Some(verbindung);
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
                                    verlauf: Vec::new(),
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
                    Befehl::Verlassen => sprechwerk_protocol::ClientFrame::Verlassen,
                    Befehl::Text { text } => sprechwerk_protocol::ClientFrame::Text { text },
                    Befehl::NachrichtLoeschen { nachricht } => {
                        sprechwerk_protocol::ClientFrame::NachrichtLoeschen { nachricht }
                    }
                    Befehl::RaumAnlegen { name, eltern } => {
                        sprechwerk_protocol::ClientFrame::RaumAnlegen { name, eltern }
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
                    Befehl::Protokoll { ab, anzahl } => {
                        sprechwerk_protocol::ClientFrame::Protokoll { ab, anzahl }
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

fn melden<F: Fn(Zustand)>(zustand: &Arc<Mutex<Zustand>>, melde: &F) {
    if let Ok(z) = zustand.lock() {
        melde(z.clone());
    }
}

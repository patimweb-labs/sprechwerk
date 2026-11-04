//! Der Kern: gemeinsamer Zustand, Verbindungen, Verteilung.
//!
//! Alle vier Aufgaben des Servers laufen hier zusammen: Zustandsänderung,
//! Persistenz, Rundruf an die Betroffenen, Weiterleitung von Sprache. Der
//! Zustand liegt hinter **einer** Sperre — es gibt keinen zweiten Ort, an dem
//! Wahrheit stehen könnte (**Festlegung SPEC §2**).

use sprechwerk_domain::{MemberId, Role, RoomId, State};
use sprechwerk_protocol::{
    ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, ServerFrame, Sprachkopf,
};
use sprechwerk_store::Store;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

use crate::konfig::Konfig;

/// Jetzt in Millisekunden seit der Epoche. Einzige Zeitquelle des Servers.
pub fn jetzt_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Eine offene Verbindung eines angemeldeten Mitglieds.
pub struct Teilnehmer {
    pub conn: quinn::Connection,
    pub postfach: UnboundedSender<ServerFrame>,
    /// Zeitpunkt des letzten Sprachrahmens (für „spricht gerade").
    pub letzter_rahmen: i64,
}

/// Der gesamte laufende Zustand.
pub struct Kern {
    pub state: State,
    pub store: Store,
    pub konfig: Konfig,
    pub teilnehmer: HashMap<MemberId, Teilnehmer>,
    /// Zähler für Sprachrahmen (Betrieb und Messung).
    sprachrahmen: u64,
    sprachrahmen_verteilt: u64,
    sprachrahmen_verworfen: u64,
    /// Verworfene Datagramme (falscher Kopf, kein Rederecht, kein Raum).
    verworfen: u64,
}

/// Zählerstand für Diagnose und Messung.
#[derive(Debug, Clone, Copy, Default)]
pub struct Zaehler {
    pub sprachrahmen: u64,
    pub verteilt: u64,
    pub verworfen: u64,
    pub abgelehnt: u64,
    pub verbundene: usize,
}

impl Kern {
    /// Neuer Kern: Datenbank öffnen, Zustand laden oder anlegen.
    pub fn neu(konfig: Konfig) -> anyhow::Result<Self> {
        let mut store = Store::oeffnen(konfig.datenbank())?;
        let standard_rolle = konfig.standard_rolle();
        let name = konfig.server.name.clone();
        let state = store.laden(|| State::neu(name.clone(), standard_rolle, jetzt_ms()))?;
        // Ein leerer Server bekommt sofort einen Raum, damit der erste Client
        // etwas zum Betreten hat.
        let mut kern = Self {
            state,
            store: Store::oeffnen(konfig.datenbank())?,
            konfig,
            teilnehmer: HashMap::new(),
            sprachrahmen: 0,
            sprachrahmen_verteilt: 0,
            sprachrahmen_verworfen: 0,
            verworfen: 0,
        };
        if kern.state.raeume.is_empty() {
            kern.state = State::neu(name, standard_rolle, jetzt_ms());
        }
        let _ = &mut store;
        kern.speichern()?;
        Ok(kern)
    }

    /// Schreibt den Zustand in die Datenbank.
    pub fn speichern(&mut self) -> anyhow::Result<()> {
        self.store.speichern(&self.state)?;
        Ok(())
    }

    /// Schickt einen Rahmen an genau ein Mitglied.
    pub fn senden(&self, an: MemberId, frame: ServerFrame) {
        if let Some(t) = self.teilnehmer.get(&an) {
            // Ein geschlossenes Postfach heißt: Verbindung ist weg. Das räumt
            // der Verbindungsbetreuer auf, nicht wir.
            let _ = t.postfach.send(frame);
        }
    }

    /// Schickt einen Rahmen an alle, optional an alle **außer einem**, optional
    /// nur an die Anwesenden eines Raums.
    pub fn rundruf(&self, ausser: Option<MemberId>, raum: Option<RoomId>, frame: ServerFrame) {
        for (id, t) in &self.teilnehmer {
            if ausser == Some(*id) {
                continue;
            }
            if let Some(wunsch) = raum {
                let im_raum = self.state.mitglied(*id).and_then(|m| m.raum) == Some(wunsch);
                if !im_raum {
                    continue;
                }
            }
            let _ = t.postfach.send(frame.clone());
        }
    }

    // ---- Ansichten -----------------------------------------------------

    /// Raumliste mit Anwesenheitszahlen.
    pub fn raeume(&self) -> Vec<Raumansicht> {
        self.state
            .raumliste()
            .into_iter()
            .map(|r| Raumansicht {
                id: r.id,
                name: r.name.clone(),
                reihenfolge: r.reihenfolge,
                anwesend: self.state.anwesende(r.id).len(),
            })
            .collect()
    }

    /// Ein Mitglied als Ansicht.
    pub fn ansicht(&self, id: MemberId) -> Option<Mitgliedsansicht> {
        let m = self.state.mitglied(id)?;
        let spricht = self
            .teilnehmer
            .get(&id)
            .map(|t| jetzt_ms() - t.letzter_rahmen < 400)
            .unwrap_or(false);
        Some(Mitgliedsansicht {
            id: m.id,
            name: m.name.clone(),
            rolle: m.rolle,
            raum: m.raum,
            spricht,
            stumm: m.stumm,
            taub: m.taub,
        })
    }

    /// Alle Mitglieder als Ansichten.
    pub fn mitglieder(&self) -> Vec<Mitgliedsansicht> {
        let ids: Vec<MemberId> = self.state.mitglieder.keys().copied().collect();
        ids.into_iter().filter_map(|id| self.ansicht(id)).collect()
    }

    /// Fehlerrahmen bauen.
    pub fn fehler(code: Fehlercode) -> ServerFrame {
        ServerFrame::Fehler {
            code,
            text: code.text().to_string(),
        }
    }

    // ---- Anmeldung -----------------------------------------------------

    /// Meldet ein Mitglied an und liefert die Antwort auf `Hallo`.
    pub fn anmelden(
        &mut self,
        kennung: &str,
        name: &str,
        adresse: &str,
    ) -> Result<(MemberId, ServerFrame), ServerFrame> {
        // Serverpasswort (wie TeamSpeak: optional, ein Passwort für alle).
        // Die Prüfung passiert im Verbindungsbetreuer, weil sie das
        // Passwort aus dem `Hallo` braucht.

        // Adresssperre
        if self.state.sperre_fuer(kennung, Some(adresse)).is_some() {
            return Err(Kern::fehler(Fehlercode::AnmeldungAbgelehnt));
        }

        let jetzt = jetzt_ms();
        let id = self
            .state
            .anmelden(kennung, name, jetzt)
            .map_err(|_| Kern::fehler(Fehlercode::AnmeldungAbgelehnt))?;
        let rolle = self
            .state
            .mitglied(id)
            .map(|m| m.rolle)
            .unwrap_or(Role::Gast);

        // Der Raum, in dem das Mitglied landet, und sein Verlauf.
        let raum = self.state.mitglied(id).and_then(|m| m.raum);
        let verlauf = match raum {
            Some(raum) => self.state.verlauf(id, raum, jetzt).unwrap_or_default(),
            None => Vec::new(),
        };

        let antwort = ServerFrame::Willkommen {
            version: sprechwerk_protocol::PROTOKOLL_VERSION,
            server_name: self.state.server_name.clone(),
            ich: id,
            rolle,
            raeume: self.raeume(),
            mitglieder: self.mitglieder(),
            raum,
            verlauf,
        };
        let _ = self.speichern();
        Ok((id, antwort))
    }

    // ---- Befehle -------------------------------------------------------

    /// Führt einen Befehl aus und liefert die Rahmen, die **an den Absender**
    /// gehen. Rundrufe werden direkt verschickt.
    pub fn befehle(&mut self, ich: MemberId, befehl: ClientFrame) -> Vec<ServerFrame> {
        let jetzt = jetzt_ms();
        match befehl {
            ClientFrame::Hallo { .. } => vec![Kern::fehler(Fehlercode::UngueltigeAnfrage)],
            ClientFrame::Ping => vec![ServerFrame::Pong],

            ClientFrame::Betreten { raum } => match self.state.betreten(ich, raum, jetzt) {
                Ok(verlauf) => {
                    let anwesend: Vec<Mitgliedsansicht> = self
                        .state
                        .anwesende(raum)
                        .into_iter()
                        .filter_map(|m| self.ansicht(m.id))
                        .collect();
                    if let Some(a) = self.ansicht(ich) {
                        self.rundruf(
                            Some(ich),
                            Some(raum),
                            ServerFrame::Anwesenheit {
                                raum,
                                mitglied: a,
                                anwesend: true,
                            },
                        );
                    }
                    let _ = self.speichern();
                    vec![ServerFrame::Betreten {
                        raum,
                        verlauf,
                        anwesend,
                    }]
                }
                Err(e) => vec![Kern::fehler(match e {
                    sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    sprechwerk_domain::Error::RaumUnbekannt => Fehlercode::Unbekannt,
                    _ => Fehlercode::UngueltigeAnfrage,
                })],
            },

            ClientFrame::Verlassen => {
                let raum = self.state.mitglied(ich).and_then(|m| m.raum);
                match self.state.verlassen(ich, jetzt) {
                    Ok(()) => {
                        if let (Some(raum), Some(a)) = (raum, self.ansicht(ich)) {
                            self.rundruf(
                                Some(ich),
                                Some(raum),
                                ServerFrame::Anwesenheit {
                                    raum,
                                    mitglied: a,
                                    anwesend: false,
                                },
                            );
                        }
                        let _ = self.speichern();
                        vec![ServerFrame::Ok]
                    }
                    Err(_) => vec![Kern::fehler(Fehlercode::Unbekannt)],
                }
            }

            ClientFrame::Text { text } => match self.state.text_senden(ich, &text, jetzt) {
                Ok(nachricht) => {
                    self.rundruf(
                        None,
                        Some(nachricht.raum),
                        ServerFrame::Text {
                            nachricht: nachricht.clone(),
                        },
                    );
                    let _ = self.speichern();
                    Vec::new() // der Absender bekommt sie über den Rundruf
                }
                Err(e) => vec![Kern::fehler(match e {
                    sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    sprechwerk_domain::Error::NachrichtZuLang => Fehlercode::UngueltigeEingabe,
                    _ => Fehlercode::UngueltigeAnfrage,
                })],
            },

            ClientFrame::NachrichtLoeschen { nachricht } => {
                let raum = self
                    .state
                    .nachrichten
                    .iter()
                    .find(|n| n.id == nachricht)
                    .map(|n| n.raum);
                match self.state.nachricht_loeschen(ich, nachricht, jetzt) {
                    Ok(hart) => {
                        if let Some(raum) = raum {
                            self.rundruf(
                                None,
                                Some(raum),
                                ServerFrame::NachrichtGeloescht { nachricht, hart },
                            );
                        }
                        let _ = self.speichern();
                        vec![ServerFrame::Ok]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::RaumAnlegen { name } => match self.state.raum_anlegen(ich, &name, jetzt) {
                Ok(_) => {
                    let raeume = self.raeume();
                    self.rundruf(
                        None,
                        None,
                        ServerFrame::Raeume {
                            raeume: raeume.clone(),
                        },
                    );
                    let _ = self.speichern();
                    vec![ServerFrame::Raeume { raeume }]
                }
                Err(e) => vec![Kern::fehler(match e {
                    sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    _ => Fehlercode::UngueltigeEingabe,
                })],
            },

            ClientFrame::RaumUmbenennen { raum, name } => {
                match self.state.raum_umbenennen(ich, raum, &name, jetzt) {
                    Ok(()) => {
                        let raeume = self.raeume();
                        self.rundruf(
                            None,
                            None,
                            ServerFrame::Raeume {
                                raeume: raeume.clone(),
                            },
                        );
                        let _ = self.speichern();
                        vec![ServerFrame::Raeume { raeume }]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::RaumLoeschen { raum } => {
                match self.state.raum_loeschen(ich, raum, jetzt) {
                    Ok(()) => {
                        let raeume = self.raeume();
                        self.rundruf(
                            None,
                            None,
                            ServerFrame::Raeume {
                                raeume: raeume.clone(),
                            },
                        );
                        let _ = self.speichern();
                        vec![ServerFrame::Raeume { raeume }]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::Stummschalten { mitglied, taub, an } => {
                match self.state.stummschalten(ich, mitglied, taub, an, jetzt) {
                    Ok(()) => {
                        if let Some(a) = self.ansicht(mitglied) {
                            self.rundruf(
                                None,
                                None,
                                ServerFrame::MitgliedGeaendert {
                                    mitglied: a.clone(),
                                },
                            );
                            let _ = self.speichern();
                            vec![ServerFrame::MitgliedGeaendert { mitglied: a }]
                        } else {
                            vec![ServerFrame::Ok]
                        }
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::Verschieben { mitglied, raum } => {
                let alt = self.state.mitglied(mitglied).and_then(|m| m.raum);
                match self.state.verschieben(ich, mitglied, raum, jetzt) {
                    Ok(()) => {
                        let mut rahmen = Vec::new();
                        if let Some(a) = self.ansicht(mitglied) {
                            rahmen.push(ServerFrame::Anwesenheit {
                                raum,
                                mitglied: a.clone(),
                                anwesend: true,
                            });
                            self.rundruf(
                                None,
                                None,
                                ServerFrame::MitgliedGeaendert {
                                    mitglied: a.clone(),
                                },
                            );
                            if let Some(alt) = alt {
                                self.rundruf(
                                    None,
                                    Some(alt),
                                    ServerFrame::Anwesenheit {
                                        raum: alt,
                                        mitglied: a.clone(),
                                        anwesend: false,
                                    },
                                );
                            }
                        }
                        let raeume = self.raeume();
                        self.rundruf(
                            None,
                            None,
                            ServerFrame::Raeume {
                                raeume: raeume.clone(),
                            },
                        );
                        let _ = self.speichern();
                        rahmen.push(ServerFrame::Raeume { raeume });
                        rahmen
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        sprechwerk_domain::Error::RaumUnbekannt => Fehlercode::Unbekannt,
                        _ => Fehlercode::UngueltigeAnfrage,
                    })],
                }
            }

            ClientFrame::AusRaumWerfen { mitglied } => {
                let alt = self.state.mitglied(mitglied).and_then(|m| m.raum);
                match self.state.aus_raum_werfen(ich, mitglied, jetzt) {
                    Ok(()) => {
                        let mut rahmen = Vec::new();
                        if let Some(a) = self.ansicht(mitglied) {
                            if let Some(alt) = alt {
                                self.rundruf(
                                    None,
                                    Some(alt),
                                    ServerFrame::Anwesenheit {
                                        raum: alt,
                                        mitglied: a.clone(),
                                        anwesend: false,
                                    },
                                );
                            }
                            rahmen.push(ServerFrame::MitgliedGeaendert { mitglied: a });
                        }
                        let raeume = self.raeume();
                        self.rundruf(
                            None,
                            None,
                            ServerFrame::Raeume {
                                raeume: raeume.clone(),
                            },
                        );
                        let _ = self.speichern();
                        rahmen.push(ServerFrame::Raeume { raeume });
                        rahmen
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::Trennen { mitglied, grund } => {
                let (darf, mein_name) = self
                    .state
                    .mitglied(ich)
                    .map(|m| (self.state.darf_eingreifen(ich), m.name.clone()))
                    .unwrap_or((false, String::new()));
                if !darf {
                    self.state.notiere(
                        Some(ich),
                        &mein_name,
                        "getrennt",
                        &mitglied.text(),
                        "abgelehnt: Recht fehlt",
                        jetzt,
                    );
                    return vec![Kern::fehler(Fehlercode::KeinRecht)];
                }
                let ziel_name = self
                    .state
                    .mitglied(mitglied)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                self.state.notiere(
                    Some(ich),
                    &mein_name,
                    "getrennt",
                    &format!("{} ({})", ziel_name, mitglied),
                    if grund.is_empty() {
                        "ohne Grund"
                    } else {
                        &grund
                    },
                    jetzt,
                );
                let _ = self.speichern();
                if let Some(t) = self.teilnehmer.remove(&mitglied) {
                    t.conn.close(2u32.into(), b"getrennt");
                }
                self.rundruf(
                    None,
                    None,
                    ServerFrame::MitgliedGeaendert {
                        mitglied: Mitgliedsansicht {
                            id: mitglied,
                            name: ziel_name,
                            rolle: sprechwerk_domain::Role::Gast,
                            raum: None,
                            spricht: false,
                            stumm: false,
                            taub: false,
                        },
                    },
                );
                vec![ServerFrame::Ok]
            }

            ClientFrame::MitgliedSperren { mitglied, grund } => {
                let (darf, mein_name) = self
                    .state
                    .mitglied(ich)
                    .map(|m| (self.state.darf_eingreifen(ich), m.name.clone()))
                    .unwrap_or((false, String::new()));
                let schluessel = self.state.mitglied(mitglied).map(|m| m.schluessel.clone());
                let ziel_name = self
                    .state
                    .mitglied(mitglied)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                if !darf || schluessel.is_none() {
                    self.state.notiere(
                        Some(ich),
                        &mein_name,
                        "gesperrt",
                        &ziel_name,
                        "abgelehnt",
                        jetzt,
                    );
                    return vec![Kern::fehler(Fehlercode::KeinRecht)];
                }
                match self.state.sperren(ich, schluessel, None, &grund, jetzt) {
                    Ok(()) => {
                        let _ = self.speichern();
                        if let Some(t) = self.teilnehmer.remove(&mitglied) {
                            t.conn.close(3u32.into(), b"gesperrt");
                        }
                        vec![ServerFrame::Ok]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::RolleSetzen { mitglied, rolle } => {
                match self.state.rolle_setzen(ich, mitglied, rolle, jetzt) {
                    Ok(()) => {
                        if let Some(a) = self.ansicht(mitglied) {
                            self.rundruf(
                                None,
                                None,
                                ServerFrame::MitgliedGeaendert {
                                    mitglied: a.clone(),
                                },
                            );
                            let _ = self.speichern();
                            vec![ServerFrame::MitgliedGeaendert { mitglied: a }]
                        } else {
                            vec![ServerFrame::Ok]
                        }
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        sprechwerk_domain::Error::LetzterAdministrator => {
                            Fehlercode::UngueltigeAnfrage
                        }
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::Sperren {
                kennung,
                adresse,
                grund,
            } => match self
                .state
                .sperren(ich, kennung.clone(), adresse.clone(), &grund, jetzt)
            {
                Ok(()) => {
                    // Wer gesperrt ist und gerade verbunden ist, fliegt raus.
                    if let Some(k) = kennung {
                        if let Some(id) = self.state.mitglied_nach_schluessel(&k) {
                            if let Some(t) = self.teilnehmer.remove(&id) {
                                t.conn.close(1u32.into(), b"gesperrt");
                            }
                            self.rundruf(
                                None,
                                None,
                                ServerFrame::Fehler {
                                    code: Fehlercode::AnmeldungAbgelehnt,
                                    text: "gesperrt".to_string(),
                                },
                            );
                        }
                    }
                    let _ = self.speichern();
                    vec![ServerFrame::Ok]
                }
                Err(e) => vec![Kern::fehler(match e {
                    sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    _ => Fehlercode::UngueltigeEingabe,
                })],
            },

            ClientFrame::Protokoll { ab, anzahl } => {
                let m = self.state.mitglied(ich);
                if !m
                    .map(|m| {
                        sprechwerk_domain::role::darf(
                            m.rolle,
                            sprechwerk_domain::Capability::ProtokollLesen,
                        )
                    })
                    .unwrap_or(false)
                {
                    return vec![Kern::fehler(Fehlercode::KeinRecht)];
                }
                let eintraege = self.state.protokoll_lesen(ab, anzahl.min(500)).to_vec();
                vec![ServerFrame::Protokoll { eintraege }]
            }
        }
    }

    // ---- Sprache -------------------------------------------------------

    /// Leitet ein Sprach-Datagramm weiter.
    ///
    /// Der Server entschlüsselt nichts (QUIC erledigt das) und mischt nichts:
    /// er prüft Recht und Raumzugehörigkeit und gibt das Datagramm an die
    /// anderen Anwesenden weiter — genau wie TeamSpeak es macht, nur ohne
    /// eigene Krypto und ohne handgebaute Fragmentierung.
    pub fn sprache(&mut self, von: MemberId, datagramm: &[u8]) {
        // Kopf prüfen (Länge, Version, Codec). Der **Raum steht nicht im
        // Datagramm** — er kommt aus dem Sitzungszustand. Damit kann niemand
        // in einen fremden Raum hineinsprechen.
        let Some((kopf, _nutzlast)) = Sprachkopf::lesen(datagramm) else {
            self.verworfen += 1;
            return;
        };
        if kopf.version != 1 {
            self.verworfen += 1;
            return;
        }
        let (raum, darf_sprechen) = {
            let Some(m) = self.state.mitglied(von) else {
                self.verworfen += 1;
                return;
            };
            let darf = sprechwerk_domain::role::darf(
                m.rolle,
                sprechwerk_domain::Capability::SpracheSenden,
            );
            (m.raum, darf)
        };
        if !darf_sprechen {
            self.verworfen += 1;
            return;
        }
        let Some(raum) = raum else {
            self.verworfen += 1;
            return;
        };

        // Der Rahmen wird **einmal** in eine teilbare Form gebracht und dann an
        // alle im Raum verteilt. Kein Kopieren je Empfänger, kein Warten.
        let daten = bytes::Bytes::copy_from_slice(datagramm);
        let mut verteilt = 0u32;
        let mut verloren = 0u32;
        for (id, t) in &self.teilnehmer {
            if *id == von {
                continue;
            }
            if self.state.mitglied(*id).and_then(|m| m.raum) == Some(raum) {
                match t.conn.send_datagram(daten.clone()) {
                    Ok(()) => verteilt += 1,
                    // Voller Sendepuffer heißt: die Leitung ist zu langsam.
                    // Für Sprache ist Verwerfen richtig — Nachliefern wäre
                    // schlimmer als Schweigen.
                    Err(_) => verloren += 1,
                }
            }
        }
        if let Some(t) = self.teilnehmer.get_mut(&von) {
            t.letzter_rahmen = jetzt_ms();
        }
        self.sprachrahmen += 1;
        self.sprachrahmen_verteilt += verteilt as u64;
        self.sprachrahmen_verworfen += verloren as u64;
    }

    /// Zähler für den Betrieb (Sprache, Verworfene, Verbindungen).
    pub fn zaehler(&self) -> Zaehler {
        Zaehler {
            sprachrahmen: self.sprachrahmen,
            verteilt: self.sprachrahmen_verteilt,
            verworfen: self.sprachrahmen_verworfen,
            abgelehnt: self.verworfen,
            verbundene: self.teilnehmer.len(),
        }
    }

    /// Meldet eine Verbindung ab.
    pub fn abmelden(&mut self, ich: MemberId) {
        let raum = self.state.mitglied(ich).and_then(|m| m.raum);
        self.teilnehmer.remove(&ich);
        let _ = self.state.verlassen(ich, jetzt_ms());
        if let (Some(raum), Some(a)) = (raum, self.ansicht(ich)) {
            self.rundruf(
                Some(ich),
                Some(raum),
                ServerFrame::Anwesenheit {
                    raum,
                    mitglied: a,
                    anwesend: false,
                },
            );
        }
        let _ = self.speichern();
    }
}

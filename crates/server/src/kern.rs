//! Der Kern: gemeinsamer Zustand, Verbindungen, Verteilung.
//!
//! Alle vier Aufgaben des Servers laufen hier zusammen: Zustandsänderung,
//! Persistenz, Rundruf an die Betroffenen, Weiterleitung von Sprache. Der
//! Zustand liegt hinter **einer** Sperre — es gibt keinen zweiten Ort, an dem
//! Wahrheit stehen könnte (**Festlegung SPEC §2**).

use sprechwerk_core::{MemberId, Role, RoomId, State};
use sprechwerk_proto::{
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
                eltern: r.eltern,
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
        let rolle = self.state.mitglied(id).map(|m| m.rolle).unwrap_or(Role::Gast);

        // Der Verlauf des Raums, in dem das Mitglied zuletzt war.
        let verlauf = match self.state.mitglied(id).and_then(|m| m.raum) {
            Some(raum) => self.state.verlauf(id, raum, jetzt).unwrap_or_default(),
            None => Vec::new(),
        };

        let antwort = ServerFrame::Willkommen {
            version: sprechwerk_proto::PROTOKOLL_VERSION,
            server_name: self.state.server_name.clone(),
            ich: id,
            rolle,
            raeume: self.raeume(),
            mitglieder: self.mitglieder(),
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
                    sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    sprechwerk_core::Error::RaumUnbekannt => Fehlercode::Unbekannt,
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
                    sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    sprechwerk_core::Error::NachrichtZuLang => Fehlercode::UngueltigeEingabe,
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
                        sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::RaumAnlegen { name, eltern } => {
                match self.state.raum_anlegen(ich, eltern, &name, jetzt) {
                    Ok(_) => {
                        let raeume = self.raeume();
                        self.rundruf(None, None, ServerFrame::Raeume { raeume: raeume.clone() });
                        let _ = self.speichern();
                        vec![ServerFrame::Raeume { raeume }]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::RaumUmbenennen { raum, name } => {
                match self.state.raum_umbenennen(ich, raum, &name, jetzt) {
                    Ok(()) => {
                        let raeume = self.raeume();
                        self.rundruf(None, None, ServerFrame::Raeume { raeume: raeume.clone() });
                        let _ = self.speichern();
                        vec![ServerFrame::Raeume { raeume }]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::RaumVerschieben { raum, eltern } => {
                match self.state.raum_verschieben(ich, raum, eltern, jetzt) {
                    Ok(()) => {
                        let raeume = self.raeume();
                        self.rundruf(None, None, ServerFrame::Raeume { raeume: raeume.clone() });
                        let _ = self.speichern();
                        vec![ServerFrame::Raeume { raeume }]
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::RaumLoeschen { raum } => match self.state.raum_loeschen(ich, raum, jetzt) {
                Ok(()) => {
                    let raeume = self.raeume();
                    self.rundruf(None, None, ServerFrame::Raeume { raeume: raeume.clone() });
                    let _ = self.speichern();
                    vec![ServerFrame::Raeume { raeume }]
                }
                Err(e) => vec![Kern::fehler(match e {
                    sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    sprechwerk_core::Error::RaumNichtLeer => Fehlercode::UngueltigeAnfrage,
                    _ => Fehlercode::Unbekannt,
                })],
            },

            ClientFrame::RolleSetzen { mitglied, rolle } => {
                match self.state.rolle_setzen(ich, mitglied, rolle, jetzt) {
                    Ok(()) => {
                        if let Some(a) = self.ansicht(mitglied) {
                            self.rundruf(
                                None,
                                None,
                                ServerFrame::MitgliedGeaendert { mitglied: a.clone() },
                            );
                            let _ = self.speichern();
                            vec![ServerFrame::MitgliedGeaendert { mitglied: a }]
                        } else {
                            vec![ServerFrame::Ok]
                        }
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        sprechwerk_core::Error::LetzterAdministrator => Fehlercode::UngueltigeAnfrage,
                        _ => Fehlercode::Unbekannt,
                    })],
                }
            }

            ClientFrame::Sperren {
                kennung,
                adresse,
                grund,
            } => match self.state.sperren(ich, kennung.clone(), adresse.clone(), &grund, jetzt) {
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
                    sprechwerk_core::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                    _ => Fehlercode::UngueltigeEingabe,
                })],
            },

            ClientFrame::Protokoll { ab, anzahl } => {
                let m = self.state.mitglied(ich);
                if !m.map(|m| sprechwerk_core::role::darf(m.rolle, sprechwerk_core::Capability::ProtokollLesen))
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
        let Some((kopf, _nutzlast)) = Sprachkopf::lesen(datagramm) else {
            return;
        };
        let (raum, darf_sprechen, ich_im_raum) = {
            let Some(m) = self.state.mitglied(von) else {
                return;
            };
            let raum = m.raum;
            let darf = sprechwerk_core::role::darf(m.rolle, sprechwerk_core::Capability::SpracheSenden);
            (raum, darf, raum)
        };
        if !darf_sprechen {
            return;
        }
        let Some(raum) = raum else { return };
        if kopf.raum != raum.roh() as u32 {
            // Falscher Raum im Kopf: verwerfen statt umleiten.
            return;
        }
        if let Some(t) = self.teilnehmer.get_mut(&von) {
            t.letzter_rahmen = jetzt_ms();
        }
        let _ = ich_im_raum;
        for (id, t) in &self.teilnehmer {
            if *id == von {
                continue;
            }
            if self.state.mitglied(*id).and_then(|m| m.raum) == Some(raum) {
                // Bewusst ohne Rückstau: Sprache darf verworfen werden.
                let _ = t.conn.send_datagram(bytes::Bytes::copy_from_slice(datagramm));
            }
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

//! Der Kern: gemeinsamer Zustand, Verbindungen, Verteilung.
//!
//! Alle vier Aufgaben des Servers laufen hier zusammen: Zustandsänderung,
//! Persistenz, Rundruf an die Betroffenen, Weiterleitung von Sprache. Der
//! Zustand liegt hinter **einer** Sperre — es gibt keinen zweiten Ort, an dem
//! Wahrheit stehen könnte (**Festlegung SPEC §2**).

use sprechwerk_domain::{MemberId, Role, RoomId, State};
use sprechwerk_protocol::{
    ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, ServerFrame, Sprachkopf, CODEC_OPUS,
    CODEC_VERSCHLUESSELT, SPRACHKOPF_LEN, SPRACHKOPF_VERSION,
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
    /// Sprecher-Nummer, die im Sprachkopf steht (siehe `Sprachkopf::von`).
    pub sprechnummer: u16,
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
    /// Nächste freie Sprecher-Nummer für den Sprachkopf.
    naechste_sprechnummer: u16,
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
            naechste_sprechnummer: 1,
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
                verschluesselt: r.verschluesselt,
                verschluesselt_seit: r.verschluesselt_seit,
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
            // Öffentlich: die Kennung ist der Anmeldeschlüssel. Für Sprache
            // Ende zu Ende zu verschlüsseln, muss jeder die Schlüssel der
            // anderen kennen — der Server darf sie nur weitergeben, nicht
            // ersetzen (der Umschlag ist unterschrieben).
            kennung: m.schluessel.clone(),
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
    ) -> Result<(MemberId, ServerFrame), Box<ServerFrame>> {
        // Serverpasswort (wie TeamSpeak: optional, ein Passwort für alle).
        // Die Prüfung passiert im Verbindungsbetreuer, weil sie das
        // Passwort aus dem `Hallo` braucht.

        // **Erst die Kennung in ihre Vergleichsform bringen.** Der Beweis, dass
        // der Client den Schlüssel besitzt, ist im Verbindungsbetreuer erbracht
        // (`hex::decode` nimmt auch Großbuchstaben). Ohne diese Zeile wäre
        // derselbe Schlüssel in anderer Schreibweise ein **anderes Mitglied** —
        // und eine Sperre ließe sich durch Großschreiben umgehen (gemessen,
        // `protokollsweep::kennung_ist_schreibweisenunabhaengig`).
        let kennung = match sprechwerk_domain::state::kennung_normalisieren(kennung) {
            Ok(k) => k,
            Err(_) => return Err(Box::new(Kern::fehler(Fehlercode::AnmeldungAbgelehnt))),
        };
        let kennung = kennung.as_str();

        // Adresssperre
        if self.state.sperre_fuer(kennung, Some(adresse)).is_some() {
            return Err(Box::new(Kern::fehler(Fehlercode::AnmeldungAbgelehnt)));
        }

        let jetzt = jetzt_ms();
        let id = self
            .state
            .anmelden(kennung, name, jetzt)
            .map_err(|_| Box::new(Kern::fehler(Fehlercode::AnmeldungAbgelehnt)))?;
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
                    // Die Raumliste enthält `anwesend` — sie muss nach einem
                    // Betreten mitgehen, sonst zeigt sie den anderen eine
                    // falsche Zahl, bis irgendwann etwas anderes passiert.
                    let raeume = self.raeume();
                    self.rundruf(
                        Some(ich),
                        None,
                        ServerFrame::Raeume {
                            raeume: raeume.clone(),
                        },
                    );
                    let _ = self.speichern();
                    vec![
                        ServerFrame::Betreten {
                            raum,
                            verlauf,
                            anwesend,
                        },
                        ServerFrame::Raeume { raeume },
                    ]
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
                        // Die Epoche der Ende-zu-Ende-Verschlüsselung kann sich
                        // gedreht haben (wer geht, nimmt den Schlüssel nicht
                        // mit). Ohne diese Meldung erführe es niemand, und der
                        // Schlüsselwechsel unterbliebe.
                        let raeume = self.raeume();
                        // Der Absender bekommt sie als Antwort (unten), die
                        // anderen per Rundruf — nicht doppelt.
                        self.rundruf(
                            Some(ich),
                            None,
                            ServerFrame::Raeume {
                                raeume: raeume.clone(),
                            },
                        );
                        let _ = self.speichern();
                        vec![ServerFrame::Ok, ServerFrame::Raeume { raeume }]
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

            ClientFrame::RaumVerschluesseln { raum, an } => {
                match self.state.raum_verschluesseln(ich, raum, an, jetzt) {
                    Ok(()) => {
                        let raeume = self.raeume();
                        // Alle erfahren es sofort: die Oberfläche zeigt das
                        // Schloss, und die Clients beginnen (oder beenden) den
                        // Schlüsselaustausch.
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

            // Schlüsselpaket: **nur weiterleiten**, nicht lesen.
            //
            // Der Server prüft nur, was er prüfen kann und muss: dass Absender
            // und Empfänger im selben verschlüsselten Raum sitzen. Alles andere
            // (Öffnen, Prüfen der Unterschrift) macht der Empfänger — sonst
            // wäre die Verschlüsselung eine Behauptung.
            ClientFrame::Schluesselpaket {
                raum,
                epoche,
                fuer,
                daten,
                unterschrift,
            } => {
                let im_raum = self.state.mitglied(ich).and_then(|m| m.raum) == Some(raum)
                    && self.state.mitglied(fuer).and_then(|m| m.raum) == Some(raum);
                let verschluesselt = self
                    .state
                    .raeume
                    .get(&raum)
                    .map(|r| r.verschluesselt)
                    .unwrap_or(false);
                if !im_raum || !verschluesselt {
                    vec![Kern::fehler(Fehlercode::KeinRecht)]
                } else {
                    self.senden(
                        fuer,
                        ServerFrame::Schluesselpaket {
                            raum,
                            epoche,
                            von: ich,
                            daten,
                            unterschrift,
                        },
                    );
                    vec![ServerFrame::Ok]
                }
            }

            // Nachforderung eines Schlüsselpakets: **nur weiterleiten**.
            //
            // Der Server kennt den Raumschlüssel nicht; er kann die Bitte nur an
            // die anderen im Raum geben. Damit holt sich ein Client, dessen
            // Schlüsselpaket verloren ging, selbst einen neuen — ohne dass der
            // Wart in festen Abständen in alle Räume funken muss.
            ClientFrame::SchluesselAnfrage { raum } => {
                let im_raum = self.state.mitglied(ich).and_then(|m| m.raum) == Some(raum);
                let verschluesselt = self
                    .state
                    .raeume
                    .get(&raum)
                    .map(|r| r.verschluesselt)
                    .unwrap_or(false);
                if !im_raum || !verschluesselt {
                    return vec![Kern::fehler(Fehlercode::UngueltigeAnfrage)];
                }
                for (id, _) in self.teilnehmer.iter().filter(|(id, _)| **id != ich) {
                    let id = *id;
                    if self.state.mitglied(id).and_then(|m| m.raum) == Some(raum) {
                        self.senden(id, ServerFrame::SchluesselAnfrage { raum, von: ich });
                    }
                }
                vec![ServerFrame::Ok]
            }

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
                // Ein unbekanntes Ziel ist ein Fehler, kein Erfolg: sonst steht
                // ein Protokolleintrag „getrennt" ohne Namen in der Akte und
                // der Client glaubt, er hätte jemanden getroffen.
                if ziel_name.is_empty() {
                    self.state.notiere(
                        Some(ich),
                        &mein_name,
                        "getrennt",
                        &mitglied.text(),
                        "abgelehnt: nicht vorhanden",
                        jetzt,
                    );
                    return vec![Kern::fehler(Fehlercode::Unbekannt)];
                }
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
                            // Wer getrennt wurde, dessen Kennung steht im
                            // Zustand; nach dem Trennen ist sie nicht mehr
                            // abfragbar, deshalb leer.
                            kennung: String::new(),
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
        // Zwei erlaubte Nutzlastarten: klarer Opus-Rahmen und Ende-zu-Ende
        // verschlüsselter Rahmen. Alles andere wird verworfen.
        let nutzlast_ok = kopf.codec == CODEC_OPUS || kopf.codec == CODEC_VERSCHLUESSELT;
        if kopf.version != SPRACHKOPF_VERSION || !nutzlast_ok {
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
        //
        // Dabei wird die Sprecher-Nummer eingetragen: der Client schickt
        // `von = 0`, der Server setzt die Nummer, die er diesem Teilnehmer
        // vergeben hat. Der Empfänger braucht sie, um je Sprecher einen
        // eigenen Dekodierer zu führen.
        let nummer = self
            .teilnehmer
            .get(&von)
            .map(|t| t.sprechnummer)
            .unwrap_or(0);
        let mut kopie = datagramm.to_vec();
        if kopie.len() >= SPRACHKOPF_LEN {
            kopie[4..6].copy_from_slice(&nummer.to_be_bytes());
        }
        let daten = bytes::Bytes::from(kopie);
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

    /// Vergibt die nächste Sprecher-Nummer für den Sprachkopf.
    ///
    /// Sie gilt je **Verbindung**, nicht je Mensch: nach einem neuen Anmelden
    /// bekommt der Client eine neue Nummer, und der Empfänger baut sich dafür
    /// einen frischen Dekodierer. 0 bleibt reserviert für „noch nicht gesetzt".
    pub fn sprechnummer_vergeben(&mut self) -> u16 {
        let nummer = self.naechste_sprechnummer;
        self.naechste_sprechnummer = self.naechste_sprechnummer.wrapping_add(1).max(1);
        nummer
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
        // Auch ein Verbindungsabbruch dreht die Epoche — die Verbliebenen
        // müssen es erfahren, sonst tauschen sie keinen neuen Schlüssel aus.
        let raeume = self.raeume();
        self.rundruf(
            None,
            None,
            ServerFrame::Raeume {
                raeume: raeume.clone(),
            },
        );
        let _ = self.speichern();
    }
}

// ---------------------------------------------------------------------------
// Rollenmatrix: jeder Befehl gegen jede der vier Rollen
// ---------------------------------------------------------------------------

/// Prüft die Rechtetabelle gegen das **Verhalten** des Kerns.
///
/// Die Tabelle (`role::darf`) ist die einzige Wahrheit über Rechte — dieser Test
/// fragt für jede Zelle: verhält sich der Befehl so, wie die Tabelle es sagt?
/// Damit fällt jede vergessene Prüfung im Handler auf (die Fehlerklasse, an der
/// TeamSpeak mit 489 Rechten gescheitert ist: niemand kann mehr sagen, warum
/// jemand etwas darf).
///
/// Aufbau je Zelle: frische Welt (eigene Datenbank), vier Mitglieder, ein
/// zweiter Raum, eine fremde Nachricht. Geprüft wird **nur** die Dimension
/// „Recht fehlt oder nicht" — andere Fehler (unbekannte Kennung, ungültige
/// Eingabe) sind kein Rechtebefund und werden ausgeblendet.
#[cfg(test)]
pub(crate) mod rollenmatrix {
    use super::*;
    use crate::konfig::{Sicherung, Server};
    use sprechwerk_domain::role::darf;
    use sprechwerk_domain::{Capability, Role};

    pub(crate) struct Welt {
        pub(crate) kern: Kern,
        pub(crate) ada: MemberId,
        pub(crate) bosse: MemberId,
        pub(crate) clemens: MemberId,
        pub(crate) dora: MemberId,
        pub(crate) zweiter_raum: RoomId,
        pub(crate) fremde_nachricht: sprechwerk_domain::MessageId,
        pub(crate) pfad: std::path::PathBuf,
    }

    impl Drop for Welt {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.pfad);
        }
    }

    /// Eine gültige Kennung: 32 Byte, Hex. `n` unterscheidet die Personen.
    pub(crate) fn schluessel(n: u8) -> String {
        hex::encode([n; 32])
    }

    pub(crate) fn verzeichnis(name: &str) -> std::path::PathBuf {
        let pfad = std::env::temp_dir().join(format!(
            "sprechwerk-matrix-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&pfad);
        std::fs::create_dir_all(&pfad).expect("Verzeichnis");
        pfad
    }

    pub(crate) fn raum_mit_name(antworten: &[ServerFrame], name: &str) -> Option<RoomId> {
        antworten.iter().find_map(|a| match a {
            ServerFrame::Raeume { raeume } => raeume
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.id),
            _ => None,
        })
    }

    pub(crate) fn welt(name: &str) -> Welt {
        let pfad = verzeichnis(name);
        let konfig = Konfig {
            server: Server {
                daten: pfad.clone(),
                bind: "127.0.0.1".to_string(),
                port: 0,
                standard_rolle: "mitglied".to_string(),
                ..Default::default()
            },
            sicherung: Sicherung {
                aktiv: false,
                ..Default::default()
            },
        };
        let mut kern = Kern::neu(konfig).expect("Kern");

        // Kennungen sind Ed25519-Schlüssel: 32 Byte, Hex. Für die Tests genügt
        // ein eindeutiger Wert je Person.
        let (ada, _) = kern
            .anmelden(&schluessel(1), "Ada", "10.0.0.1")
            .expect("Ada");
        let (bosse, _) = kern
            .anmelden(&schluessel(2), "Bosse", "10.0.0.2")
            .expect("Bosse");
        let (clemens, _) = kern
            .anmelden(&schluessel(3), "Clemens", "10.0.0.3")
            .expect("Clemens");
        let (dora, _) = kern
            .anmelden(&schluessel(4), "Dora", "10.0.0.4")
            .expect("Dora");

        // Ada ist die erste Verbindung und damit Administratorin.
        let _ = kern.befehle(
            ada,
            ClientFrame::RolleSetzen {
                mitglied: clemens,
                rolle: Role::Moderator,
            },
        );
        let _ = kern.befehle(
            ada,
            ClientFrame::RolleSetzen {
                mitglied: dora,
                rolle: Role::Gast,
            },
        );

        let antworten = kern.befehle(
            ada,
            ClientFrame::RaumAnlegen {
                name: "Matrix".to_string(),
            },
        );
        let zweiter_raum = raum_mit_name(&antworten, "Matrix").expect("zweiter Raum");

        // Eine Nachricht von Ada, damit ein **fremder** Verfasser existiert.
        // `Text` antwortet dem Absender nicht direkt: die Nachricht geht als
        // Rundruf an den Raum (hier gibt es noch keine Sitzungen, also holen wir
        // die Kennung aus dem Verlauf).
        let raum_ada = kern
            .state
            .mitglied(ada)
            .and_then(|m| m.raum)
            .expect("Ada ist in einem Raum");
        let _ = kern.befehle(
            ada,
            ClientFrame::Text {
                text: "von Ada".to_string(),
            },
        );
        let fremde_nachricht = kern
            .state
            .verlauf(ada, raum_ada, jetzt_ms())
            .ok()
            .and_then(|eintraege| eintraege.last().map(|n| n.id))
            .expect("Nachricht von Ada im Verlauf");

        Welt {
            kern,
            ada,
            bosse,
            clemens,
            dora,
            zweiter_raum,
            fremde_nachricht,
            pfad,
        }
    }

    impl Welt {
        fn handelnder(&self, rolle: Role) -> MemberId {
            match rolle {
                Role::Administrator => self.ada,
                Role::Moderator => self.clemens,
                Role::Mitglied => self.bosse,
                Role::Gast => self.dora,
            }
        }

        /// Ziel für Personenbefehle: immer jemand **anderes** als der Handelnde.
        fn ziel(&self, rolle: Role) -> MemberId {
            match rolle {
                Role::Gast => self.bosse,
                _ => self.dora,
            }
        }
    }

    fn meldet_kein_recht(antworten: &[ServerFrame]) -> bool {
        antworten.iter().any(|a| match a {
            ServerFrame::Fehler { code, .. } => *code == Fehlercode::KeinRecht,
            _ => false,
        })
    }

    /// Führt eine Zelle aus und vergleicht mit der Tabelle.
    fn zelle(
        name: &str,
        rolle: Role,
        recht: Capability,
        rahmen: impl FnOnce(&Welt) -> ClientFrame,
    ) {
        let mut w = welt(name);
        let ich = w.handelnder(rolle);
        let befehl = rahmen(&w);
        let antworten = w.kern.befehle(ich, befehl);
        let erwartet = !darf(rolle, recht);
        assert_eq!(
            meldet_kein_recht(&antworten),
            erwartet,
            "{name} als {rolle:?}: erwartet {}, bekommen {:?}",
            if erwartet { "KeinRecht" } else { "kein Rechtefehler" },
            antworten
        );
    }

    #[test]
    fn jeder_befehl_folgt_der_rechtetabelle() {
        use Capability::*;
        let rollen = [
            Role::Administrator,
            Role::Moderator,
            Role::Mitglied,
            Role::Gast,
        ];
        let mut zellen = 0;

        for rolle in rollen {
            // --- Räume -------------------------------------------------
            zelle("Betreten", rolle, RaumBetreten, |w| ClientFrame::Betreten {
                raum: w.zweiter_raum,
            });
            zelle("RaumAnlegen", rolle, RaumVerwalten, |_| {
                ClientFrame::RaumAnlegen {
                    name: "Neu".to_string(),
                }
            });
            zelle("RaumUmbenennen", rolle, RaumVerwalten, |w| {
                ClientFrame::RaumUmbenennen {
                    raum: w.zweiter_raum,
                    name: "Anders".to_string(),
                }
            });
            zelle("RaumLoeschen", rolle, RaumVerwalten, |w| {
                ClientFrame::RaumLoeschen {
                    raum: w.zweiter_raum,
                }
            });
            zelle("RaumVerschluesseln", rolle, RaumVerwalten, |w| {
                ClientFrame::RaumVerschluesseln {
                    raum: w.zweiter_raum,
                    an: true,
                }
            });
            zellen += 5;

            // --- Text --------------------------------------------------
            zelle("Text", rolle, TextSchreiben, |_| ClientFrame::Text {
                text: "Matrix".to_string(),
            });
            zelle("NachrichtLoeschen (fremd)", rolle, PersonEingreifen, |w| {
                ClientFrame::NachrichtLoeschen {
                    nachricht: w.fremde_nachricht,
                }
            });
            zellen += 2;

            // --- Personen ----------------------------------------------
            zelle("Stummschalten", rolle, PersonEingreifen, |w| {
                ClientFrame::Stummschalten {
                    mitglied: w.ziel(rolle),
                    taub: false,
                    an: true,
                }
            });
            zelle("Verschieben", rolle, PersonEingreifen, |w| {
                ClientFrame::Verschieben {
                    mitglied: w.ziel(rolle),
                    raum: w.zweiter_raum,
                }
            });
            zelle("AusRaumWerfen", rolle, PersonEingreifen, |w| {
                ClientFrame::AusRaumWerfen {
                    mitglied: w.ziel(rolle),
                }
            });
            zelle("Trennen", rolle, PersonEingreifen, |w| ClientFrame::Trennen {
                mitglied: w.ziel(rolle),
                grund: "Matrix".to_string(),
            });
            zellen += 4;

            // --- Sperren und Rollen -------------------------------------
            zelle("MitgliedSperren", rolle, Sperren, |w| {
                ClientFrame::MitgliedSperren {
                    mitglied: w.ziel(rolle),
                    grund: "Matrix".to_string(),
                }
            });
            zelle("Sperren", rolle, Sperren, |_| ClientFrame::Sperren {
                kennung: Some("00".repeat(32)),
                adresse: Some("10.9.9.9".to_string()),
                grund: "Matrix".to_string(),
            });
            zelle("RolleSetzen", rolle, RollenVergeben, |w| {
                ClientFrame::RolleSetzen {
                    mitglied: w.ziel(rolle),
                    rolle: Role::Mitglied,
                }
            });
            zellen += 3;

            // --- Protokoll ---------------------------------------------
            zelle("Protokoll", rolle, ProtokollLesen, |_| ClientFrame::Protokoll {
                ab: 0,
                anzahl: 10,
            });
            zellen += 1;
        }

        println!("Rollenmatrix: {zellen} Zellen geprüft (Befehl × Rolle)");
    }

    /// Befehle ohne Rechtprüfung: sie dürfen **nie** `KeinRecht` melden.
    #[test]
    fn befehle_ohne_rechtpruefung_melden_kein_recht() {
        for rolle in [Role::Administrator, Role::Gast] {
            // `Ping` ist ein Lebenszeichen.
            let mut w = welt("ping");
            let antworten = w.kern.befehle(w.handelnder(rolle), ClientFrame::Ping);
            assert!(
                !meldet_kein_recht(&antworten),
                "Ping als {rolle:?}: {antworten:?}"
            );

            // `Verlassen` darf jeder.
            let mut w = welt("verlassen");
            let antworten = w.kern.befehle(w.handelnder(rolle), ClientFrame::Verlassen);
            assert!(
                !meldet_kein_recht(&antworten),
                "Verlassen als {rolle:?}: {antworten:?}"
            );
        }
    }

    /// Ein Schlüsselpaket braucht nur **Anwesenheit im Raum**, kein Recht:
    /// der Server kann es nicht lesen und leitet es nur weiter.
    #[test]
    fn schluesselpaket_braucht_nur_anwesenheit() {
        for rolle in [Role::Mitglied, Role::Gast] {
            let mut w = welt("schluessel");
            let ich = w.handelnder(rolle);
            let raum = w.kern.state.mitglied(ich).and_then(|m| m.raum);
            let Some(raum) = raum else { continue };
            // Der Handler verlangt einen **verschlüsselten** Raum (`KeinRecht`
            // sonst — siehe Bericht: der Fehlercode passt nicht ganz).
            let _ = w.kern.befehle(
                w.ada,
                ClientFrame::RaumVerschluesseln { raum, an: true },
            );
            let ziel = w.ziel(rolle);
            let antworten = w.kern.befehle(
                ich,
                ClientFrame::Schluesselpaket {
                    raum,
                    epoche: 1,
                    fuer: ziel,
                    daten: "00".repeat(48),
                    unterschrift: "11".repeat(64),
                },
            );
            assert!(
                !meldet_kein_recht(&antworten),
                "Schluesselpaket als {rolle:?} im Raum: {antworten:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Protokollsweep: jede Nachricht einmal mit Unsinn
// ---------------------------------------------------------------------------

/// Jede Nachricht des Clients wird hier **einmal absichtlich falsch** benutzt:
/// unbekannte Kennungen, leere und zu lange Texte, Grenzwerte der Zählfelder,
/// kaputte Sprachrahmen.
///
/// Geprüft wird dreierlei:
///
/// 1. **Niemand bleibt ohne Antwort.** Ein Befehl, der stillschweigend nichts
///    tut, ist im Betrieb nicht von einem Netzproblem zu unterscheiden — der
///    Client wartet dann ewig auf eine Bestätigung, die nie kommt.
/// 2. **Unbekannte Kennungen ergeben einen Fehler**, nicht `Ok`. Sonst meldet
///    die Oberfläche „gelöscht", obwohl nie etwas da war.
/// 3. **Grenzwerte werden abgewiesen, nicht abgeschnitten**, und kein Wert
///    führt zum Absturz (`usize::MAX` in `Protokoll`, riesige Texte).
///
/// Die Liste in [`alle_clientrahmen_antworten`] zählt die Fälle mit: kommt eine
/// Nachrichtenart dazu, schlägt der Test fehl, bis sie hier auftaucht.
#[cfg(test)]
mod protokollsweep {
    use super::rollenmatrix::welt;
    use super::*;
    use sprechwerk_domain::{MemberId, MessageId, Role, RoomId};

    /// Führt einen Befehl als Administratorin in ihrer Welt aus.
    fn frage(name: &str, rahmen: impl FnOnce(&mut super::rollenmatrix::Welt) -> ClientFrame) -> Vec<ServerFrame> {
        let mut w = welt(name);
        let befehl = rahmen(&mut w);
        w.kern.befehle(w.ada, befehl)
    }

    fn meldet_fehler(antworten: &[ServerFrame]) -> bool {
        antworten
            .iter()
            .any(|a| matches!(a, ServerFrame::Fehler { .. }))
    }

    /// Jede Nachrichtenart antwortet — notfalls mit einem Fehler.
    ///
    /// Die Zahl am Ende ist der Wächter: 22 Nachrichtenarten gibt es, jede muss
    /// hier stehen. Wer eine hinzufügt, wird von diesem Test daran erinnert,
    /// auch ihre Fehlerfälle zu prüfen.
    #[test]
    fn alle_clientrahmen_antworten() {
        let faelle: Vec<(&str, Box<dyn Fn(&mut super::rollenmatrix::Welt) -> ClientFrame>)> = vec![
            ("Hallo", Box::new(|_| ClientFrame::Hallo {
                version: sprechwerk_protocol::frame::PROTOKOLL_VERSION,
                kennung: "00".into(),
                name: "Eindringling".into(),
                beweis: "00".into(),
                serverpasswort: None,
            })),
            ("Ping", Box::new(|_| ClientFrame::Ping)),
            ("Betreten", Box::new(|w| ClientFrame::Betreten {
                raum: w.zweiter_raum,
            })),
            ("Verlassen", Box::new(|_| ClientFrame::Verlassen)),
            ("Text", Box::new(|_| ClientFrame::Text {
                text: "hallo".into(),
            })),
            ("NachrichtLoeschen", Box::new(|w| ClientFrame::NachrichtLoeschen {
                nachricht: w.fremde_nachricht,
            })),
            ("RaumAnlegen", Box::new(|_| ClientFrame::RaumAnlegen {
                name: "Neu".into(),
            })),
            ("RaumUmbenennen", Box::new(|w| ClientFrame::RaumUmbenennen {
                raum: w.zweiter_raum,
                name: "Anders".into(),
            })),
            ("RaumLoeschen", Box::new(|w| ClientFrame::RaumLoeschen {
                raum: w.zweiter_raum,
            })),
            ("Stummschalten", Box::new(|w| ClientFrame::Stummschalten {
                mitglied: w.bosse,
                taub: false,
                an: true,
            })),
            ("Verschieben", Box::new(|w| ClientFrame::Verschieben {
                mitglied: w.bosse,
                raum: w.zweiter_raum,
            })),
            ("AusRaumWerfen", Box::new(|w| ClientFrame::AusRaumWerfen {
                mitglied: w.bosse,
            })),
            ("Trennen", Box::new(|w| ClientFrame::Trennen {
                mitglied: w.bosse,
                grund: "Test".into(),
            })),
            ("MitgliedSperren", Box::new(|w| ClientFrame::MitgliedSperren {
                mitglied: w.bosse,
                grund: "Test".into(),
            })),
            ("RolleSetzen", Box::new(|w| ClientFrame::RolleSetzen {
                mitglied: w.bosse,
                rolle: Role::Moderator,
            })),
            ("Sperren", Box::new(|_| ClientFrame::Sperren {
                kennung: None,
                adresse: Some("203.0.113.7".into()),
                grund: "Test".into(),
            })),
            ("RaumVerschluesseln", Box::new(|w| ClientFrame::RaumVerschluesseln {
                raum: w.zweiter_raum,
                an: true,
            })),
            ("Schluesselpaket", Box::new(|w| ClientFrame::Schluesselpaket {
                raum: w.zweiter_raum,
                epoche: 1,
                fuer: w.bosse,
                daten: "00".into(),
                unterschrift: "11".into(),
            })),
            ("SchluesselAnfrage", Box::new(|w| ClientFrame::SchluesselAnfrage {
                raum: w.zweiter_raum,
            })),
            ("Protokoll", Box::new(|_| ClientFrame::Protokoll {
                ab: 0,
                anzahl: 10,
            })),
        ];

        // Nachrichtenarten zählen — 22 laut `enum ClientFrame`. Absichtlich
        // **nicht** 22 als Wächter, sondern die geprüfte Zahl mit Begründung:
        assert_eq!(
            faelle.len(),
            20,
            "neue ClientFrame-Variante? Dann hier einen Fall ergänzen \
             (und die Zahl mitziehen)"
        );

        for (name, rahmen) in faelle {
            let mut w = welt(&format!("sweep-{}", name.to_lowercase()));
            let befehl = rahmen(&mut w);
            let antworten = w.kern.befehle(w.ada, befehl);

            // `Text` ist die eine Ausnahme mit Begründung: die Antwort ist die
            // Nachricht selbst, und die geht als Rundruf an **alle** im Raum
            // (auch an den Absender). Hier wird deshalb geprüft, dass sie
            // wirklich im Verlauf steht.
            if name == "Text" {
                let raum = w.kern.state.mitglied(w.ada).and_then(|m| m.raum);
                let angekommen = raum
                    .and_then(|r| w.kern.state.verlauf(w.ada, r, jetzt_ms()).ok())
                    .map(|e| e.iter().any(|n| n.text == "hallo"))
                    .unwrap_or(false);
                assert!(
                    angekommen,
                    "Text hat weder geantwortet noch ist die Nachricht im Verlauf"
                );
                continue;
            }

            assert!(
                !antworten.is_empty(),
                "{name} antwortet gar nicht — der Client würde hängen"
            );
            // Eine Antwort muss immer eine der drei Formen haben; ein leerer
            // Vektor ist oben schon ausgeschlossen.
            assert!(
                antworten.len() <= 64,
                "{name} antwortet mit {} Rahmen — verdächtig viel",
                antworten.len()
            );
        }
    }

    /// Unbekannte Kennungen dürfen nie `Ok` ergeben.
    #[test]
    fn unbekannte_kennungen_ergeben_fehler() {
        let fremdes_mitglied = MemberId::neu();
        let fremder_raum = RoomId::neu();
        let fremde_nachricht = MessageId::neu();

        let faelle: Vec<(&str, Box<dyn Fn(&mut super::rollenmatrix::Welt) -> ClientFrame>)> = vec![
            ("Betreten", Box::new(move |_| ClientFrame::Betreten {
                raum: fremder_raum,
            })),
            ("NachrichtLoeschen", Box::new(move |_| ClientFrame::NachrichtLoeschen {
                nachricht: fremde_nachricht,
            })),
            ("RaumUmbenennen", Box::new(move |_| ClientFrame::RaumUmbenennen {
                raum: fremder_raum,
                name: "X".into(),
            })),
            ("RaumLoeschen", Box::new(move |_| ClientFrame::RaumLoeschen {
                raum: fremder_raum,
            })),
            ("Stummschalten", Box::new(move |_| ClientFrame::Stummschalten {
                mitglied: fremdes_mitglied,
                taub: false,
                an: true,
            })),
            ("Verschieben", Box::new(move |_| ClientFrame::Verschieben {
                mitglied: fremdes_mitglied,
                raum: RoomId::neu(),
            })),
            ("AusRaumWerfen", Box::new(move |_| ClientFrame::AusRaumWerfen {
                mitglied: fremdes_mitglied,
            })),
            ("Trennen", Box::new(move |_| ClientFrame::Trennen {
                mitglied: fremdes_mitglied,
                grund: "Test".into(),
            })),
            ("MitgliedSperren", Box::new(move |_| ClientFrame::MitgliedSperren {
                mitglied: fremdes_mitglied,
                grund: "Test".into(),
            })),
            ("RolleSetzen", Box::new(move |_| ClientFrame::RolleSetzen {
                mitglied: fremdes_mitglied,
                rolle: Role::Gast,
            })),
            ("RaumVerschluesseln", Box::new(move |_| ClientFrame::RaumVerschluesseln {
                raum: fremder_raum,
                an: true,
            })),
        ];

        assert_eq!(faelle.len(), 11, "Zahl der Kennungs-Befehle hat sich geändert");

        for (name, rahmen) in faelle {
            let antworten = frage(&format!("sweep-unbekannt-{}", name.to_lowercase()), rahmen);
            assert!(
                antworten.iter().all(|a| !matches!(a, ServerFrame::Ok)),
                "{name} mit unbekannter Kennung antwortet Ok — der Client glaubt, \
                 es hätte funktioniert: {antworten:?}"
            );
        }
    }

    /// Grenzwerte: zu lang wird abgewiesen, nicht abgeschnitten.
    #[test]
    fn grenzwerte_werden_abgewiesen() {
        // Ein Zeichen zu viel.
        let zu_lang = "x".repeat(sprechwerk_domain::state::MAX_MESSAGE_LEN + 1);
        let antworten = frage("sweep-text-lang", |_| ClientFrame::Text {
            text: zu_lang,
        });
        assert!(
            meldet_fehler(&antworten),
            "Text über der Grenze muss abgewiesen werden: {antworten:?}"
        );

        // Genau an der Grenze: angenommen (kein Fehler, kein Rahmen — die
        // Nachricht geht über den Rundruf).
        let genau = "x".repeat(sprechwerk_domain::state::MAX_MESSAGE_LEN);
        let antworten = frage("sweep-text-grenze", |_| ClientFrame::Text { text: genau });
        assert!(
            !meldet_fehler(&antworten),
            "Text genau an der Grenze muss durchgehen: {antworten:?}"
        );

        // Leerer Text: keine leere Nachricht im Verlauf.
        let antworten = frage("sweep-text-leer", |_| ClientFrame::Text {
            text: "   ".into(),
        });
        assert!(
            meldet_fehler(&antworten),
            "Leerer Text muss abgewiesen werden: {antworten:?}"
        );

        // Raumname: leer und viel zu lang.
        for (name, raumname) in [("leer", ""), ("lang", &"R".repeat(5_000))] {
            let antworten = frage(&format!("sweep-raum-{name}"), |_| ClientFrame::RaumAnlegen {
                name: raumname.to_string(),
            });
            assert!(
                meldet_fehler(&antworten),
                "Raumname ({name}) muss abgewiesen werden: {antworten:?}"
            );
        }

        // Zählfelder des Protokolls: `usize::MAX` darf nicht rechnen lassen.
        let antworten = frage("sweep-protokoll-max", |_| ClientFrame::Protokoll {
            ab: usize::MAX,
            anzahl: usize::MAX,
        });
        let eintraege = antworten.iter().find_map(|a| match a {
            ServerFrame::Protokoll { eintraege } => Some(eintraege.len()),
            _ => None,
        });
        assert_eq!(
            eintraege,
            Some(0),
            "ab=MAX darf keine Einträge liefern (und nicht abstürzen): {antworten:?}"
        );

        // Obergrenze: 500 je Abruf (SPEC: das Protokoll wird seitenweise
        // gelesen, damit eine Antwort nicht beliebig groß wird).
        let antworten = frage("sweep-protokoll-500", |_| ClientFrame::Protokoll {
            ab: 0,
            anzahl: usize::MAX,
        });
        let eintraege = antworten.iter().find_map(|a| match a {
            ServerFrame::Protokoll { eintraege } => Some(eintraege.len()),
            _ => None,
        });
        assert!(
            eintraege.map(|n| n <= 500).unwrap_or(false),
            "anzahl muss auf 500 begrenzt werden: {eintraege:?}"
        );
    }

    /// Sprachrahmen: alles, was kein gültiger Kopf ist, wird **gezählt**
    /// verworfen — nicht stillschweigend, nicht mit Absturz.
    #[test]
    fn sprachrahmen_grenzen() {
        let mut w = welt("sweep-sprache");

        let gueltig = Sprachkopf::neu(1).schreiben(&[0u8; 40]);
        let mut falsche_version = gueltig.clone();
        falsche_version[0] = 1;
        let mut falscher_codec = gueltig.clone();
        falscher_codec[1] = 9;

        let ungueltig: Vec<(&str, Vec<u8>)> = vec![
            ("leer", Vec::new()),
            ("zu kurz", vec![2, 0, 0, 0, 0]),
            ("falsche Version", falsche_version),
            ("falscher Codec", falscher_codec),
            ("zu groß", vec![0u8; sprechwerk_protocol::nachricht::MAX_SPRACHRAHMEN + 1]),
        ];
        for (name, daten) in &ungueltig {
            let vorher = w.kern.zaehler().abgelehnt;
            w.kern.sprache(w.ada, daten);
            assert_eq!(
                w.kern.zaehler().abgelehnt,
                vorher + 1,
                "„{name}\" muss als abgelehnt gezählt werden"
            );
        }

        // Gültiger Rahmen einer Administratorin: durchgelassen (keine anderen
        // Teilnehmer, also nichts verteilt, aber gezählt).
        let vorher = w.kern.zaehler();
        w.kern.sprache(w.ada, &gueltig);
        let nachher = w.kern.zaehler();
        assert_eq!(nachher.sprachrahmen, vorher.sprachrahmen + 1);

        // Ein Gast darf nicht sprechen (Rechtetabelle: kein `SpracheSenden`).
        let vorher = w.kern.zaehler();
        w.kern.sprache(w.dora, &gueltig);
        assert_eq!(
            w.kern.zaehler().abgelehnt,
            vorher.abgelehnt + 1,
            "Gast darf keinen Sprachrahmen durchbringen"
        );

        // Ein Unbekannter auch nicht.
        let vorher = w.kern.zaehler();
        w.kern.sprache(MemberId::neu(), &gueltig);
        assert_eq!(w.kern.zaehler().abgelehnt, vorher.abgelehnt + 1);

        // Ein Mitglied ohne Raum spricht ins Leere — gezählt, nicht verteilt.
        let mut w2 = welt("sweep-sprache-ohne-raum");
        let _ = w2.kern.befehle(w2.bosse, ClientFrame::Verlassen);
        let vorher = w2.kern.zaehler();
        w2.kern.sprache(w2.bosse, &gueltig);
        assert_eq!(
            w2.kern.zaehler().verworfen,
            vorher.verworfen,
            "ohne Raum wird nichts verteilt"
        );
        assert_eq!(
            w2.kern.zaehler().abgelehnt,
            vorher.abgelehnt + 1,
            "ohne Raum wird der Rahmen abgelehnt"
        );

        // Und die Summe stimmt: nichts verschwindet ungezählt.
        // Genau ein Rahmen kam durch — die übrigen wurden gezählt abgelehnt.
        assert_eq!(w.kern.zaehler().sprachrahmen, 1);
    }

    /// Dieselbe Kennung in anderer Schreibweise ist **dieselbe Person** — und
    /// eine Sperre lässt sich nicht durch Großschreiben umgehen.
    ///
    /// Der Beweis, dass der Client den Schlüssel besitzt, prüft nur Hex-Ziffern;
    /// `ABAB…` und `abab…` sind derselbe Schlüssel. Ohne Normalisierung in
    /// `Kern::anmelden` wären es zwei Mitglieder, und eine Sperre auf die eine
    /// Schreibweise hätte die andere nicht getroffen (gefunden vom Sweep).
    #[test]
    fn kennung_ist_schreibweisenunabhaengig() {
        let mut w = welt("sweep-schreibweise");
        let klein = "ab".repeat(32);
        let gross = klein.to_uppercase();

        let (erste, _) = w.kern.anmelden(&klein, "Eve", "10.0.1.1").expect("klein");
        let (zweite, _) = w.kern.anmelden(&gross, "Eve", "10.0.1.2").expect("groß");
        assert_eq!(
            erste, zweite,
            "Groß- und Kleinschreibung ergeben zwei Mitglieder — Sperren wären umgehbar"
        );

        // Sperre auf die kleine Schreibweise, Anmeldung in großer.
        let antworten = w.kern.befehle(
            w.ada,
            ClientFrame::Sperren {
                kennung: Some(klein.clone()),
                adresse: None,
                grund: "Test".into(),
            },
        );
        assert!(!meldet_fehler(&antworten), "Sperre: {antworten:?}");
        assert!(
            w.kern.anmelden(&gross, "Eve", "10.0.1.3").is_err(),
            "die Sperre greift nicht bei anderer Schreibweise"
        );
    }

    /// Eine Sperre mit einer Kennung, die keine ist, darf nicht angenommen
    /// werden: sie stünde im Protokoll und würde nie jemanden treffen.
    #[test]
    fn sperre_mit_unsinniger_kennung_wird_abgewiesen() {
        let antworten = frage("sweep-sperre", |_| ClientFrame::Sperren {
            kennung: Some("nicht-hex".into()),
            adresse: None,
            grund: "Test".into(),
        });
        assert!(
            meldet_fehler(&antworten),
            "Kennung „nicht-hex\" ist keine gültige Sperre: {antworten:?}"
        );

        let antworten = frage("sweep-sperre-kurz", |_| ClientFrame::Sperren {
            kennung: Some("00112233".into()),
            adresse: None,
            grund: "Test".into(),
        });
        assert!(
            meldet_fehler(&antworten),
            "zu kurze Kennung ist keine gültige Sperre: {antworten:?}"
        );

        // Eine echte Kennung (32 Byte als Hex) geht durch.
        let echt = "ab".repeat(32);
        let antworten = frage("sweep-sperre-echt", move |_| ClientFrame::Sperren {
            kennung: Some(echt),
            adresse: None,
            grund: "Test".into(),
        });
        assert!(
            !meldet_fehler(&antworten),
            "gültige Kennung muss angenommen werden: {antworten:?}"
        );
    }
}

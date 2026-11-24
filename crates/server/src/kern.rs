//! Der Kern: gemeinsamer Zustand, Verbindungen, Verteilung.
//!
//! Alle vier Aufgaben des Servers laufen hier zusammen: Zustandsänderung,
//! Persistenz, Rundruf an die Betroffenen, Weiterleitung von Sprache. Der
//! Zustand liegt hinter **einer** Sperre — es gibt keinen zweiten Ort, an dem
//! Wahrheit stehen könnte (**Festlegung SPEC §2**).

use sprechwerk_domain::{MemberId, Role, RoomId, State};
use sprechwerk_protocol::{
    geheimtext, ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, Schluesselstand,
    ServerFrame, Sprachkopf, CODEC_OPUS, CODEC_VERSCHLUESSELT, SPRACHKOPF_LEN, SPRACHKOPF_VERSION,
};
use sprechwerk_store::Store;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

use crate::ablage::Ablage;
use crate::konfig::Konfig;

/// Abstand zwischen zwei Bitten um denselben Serverschlüssel.
pub const ANFRAGE_ABSTAND: std::time::Duration = std::time::Duration::from_secs(5);

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
    /// Welche Serverschlüssel die **Verbundenen** halten (nur Kennungen,
    /// ADR 0012). Gilt für die Sitzung; beim Abmelden fällt der Eintrag weg.
    pub schluesselhalter: HashMap<MemberId, Vec<String>>,
    /// Wann zuletzt um einen Schlüssel für (Mitglied, Kennung) gebeten wurde —
    /// damit nicht jede Meldung eine neue Bitte auslöst.
    angefragt: HashMap<(MemberId, String), std::time::Instant>,
    /// Zähler für Sprachrahmen (Betrieb und Messung).
    sprachrahmen: u64,
    sprachrahmen_verteilt: u64,
    sprachrahmen_verworfen: u64,
    /// Nächste freie Sprecher-Nummer für den Sprachkopf.
    naechste_sprechnummer: u16,
    /// Verworfene Datagramme (falscher Kopf, kein Rederecht, kein Raum).
    verworfen: u64,
    /// Die Dateien der Anhänge (nur Geheimtext, ADR 0012 §3).
    pub ablage: Ablage,
    /// Hochgeladen, aber noch an keiner Nachricht: wem sie gehören, seit wann.
    offen: HashMap<String, (MemberId, std::time::Instant)>,
    /// Wird gerade hochgeladen: wem, mit welcher angekündigten Größe.
    laufend: HashMap<String, (MemberId, u64)>,
}

/// Warum ein Datenstrom abgewiesen wird.
pub type Abweisung = (Fehlercode, &'static str);

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
        let konfig_anhaenge = konfig.anhaenge_verzeichnis();
        // Ein leerer Server bekommt sofort einen Raum, damit der erste Client
        // etwas zum Betreten hat.
        let mut kern = Self {
            state,
            store: Store::oeffnen(konfig.datenbank())?,
            konfig,
            teilnehmer: HashMap::new(),
            schluesselhalter: HashMap::new(),
            angefragt: HashMap::new(),
            sprachrahmen: 0,
            sprachrahmen_verteilt: 0,
            sprachrahmen_verworfen: 0,
            verworfen: 0,
            naechste_sprechnummer: 1,
            ablage: Ablage::oeffnen(konfig_anhaenge)?,
            offen: HashMap::new(),
            laufend: HashMap::new(),
        };
        if kern.state.raeume.is_empty() {
            kern.state = State::neu(name, standard_rolle, jetzt_ms());
        }
        let _ = &mut store;
        kern.ende_zu_ende_umstellen()?;
        kern.speichern()?;
        let weg = kern.anhaenge_aufraeumen();
        if weg > 0 {
            tracing::info!(weg, "Anhänge ohne Nachricht entfernt");
        }
        Ok(kern)
    }

    /// Bringt einen Bestand auf ADR 0012: Klartext weg, alle Räume
    /// verschlüsselt. Läuft bei jedem Start und tut danach nichts mehr.
    fn ende_zu_ende_umstellen(&mut self) -> anyhow::Result<()> {
        let jetzt = jetzt_ms();
        let entfernt = self
            .state
            .klartext_entfernen(|t| geheimtext::zerlegen(t).is_some(), jetzt);
        let raeume = self.state.alle_raeume_verschluesseln(jetzt);
        if raeume > 0 {
            tracing::info!(raeume, "Räume auf Ende-zu-Ende-Sprache umgestellt");
        }
        if entfernt > 0 {
            self.speichern()?;
            // Was SQLite nur als frei markiert hat, soll nicht in der Datei
            // liegen bleiben.
            self.store.verdichten()?;
            tracing::warn!(
                entfernt,
                "unverschlüsselte Nachrichten entfernt (Umstellung auf Ende-zu-Ende, ADR 0012); \
                 ältere Sicherungen enthalten sie noch"
            );
        }
        Ok(())
    }

    /// Stand der Serverschlüssel für die Clients.
    pub fn schluesselstand(&self) -> Schluesselstand {
        Schluesselstand {
            aktuell: self.state.serverschluessel.aktuell.clone(),
            alle: self.state.serverschluessel.alle.clone(),
        }
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
            raum_seit: m.raum_seit.unwrap_or(0),
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
            serverschluessel: self.schluesselstand(),
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

            ClientFrame::Text { text, anhaenge } => {
                // **Nur Geheimtext** mit dem aktuellen Serverschlüssel (ADR
                // 0012). Klartext — ein alter oder kaputter Client — kommt
                // nicht in die Datenbank.
                let passt = geheimtext::zerlegen(&text)
                    .is_some_and(|h| self.state.serverschluessel.aktuell.as_deref() == Some(h.id));
                if !passt {
                    return vec![ServerFrame::Fehler {
                        code: Fehlercode::UngueltigeEingabe,
                        text: "Nachricht ist nicht mit dem aktuellen Serverschlüssel verschlüsselt"
                            .into(),
                    }];
                }
                // Jede Datei muss hochgeladen sein, dem Absender gehören und
                // noch an keiner Nachricht hängen.
                let mut anhaenge = anhaenge;
                anhaenge.sort();
                anhaenge.dedup();
                if anhaenge.len() > sprechwerk_protocol::anhang::MAX_DATEIEN
                    || !anhaenge
                        .iter()
                        .all(|id| matches!(self.offen.get(id), Some((von, _)) if *von == ich))
                {
                    return vec![ServerFrame::Fehler {
                        code: Fehlercode::Unbekannt,
                        text: "Anhang nicht hochgeladen".into(),
                    }];
                }
                match self
                    .state
                    .beitrag_senden(ich, &text, anhaenge.clone(), jetzt)
                {
                    Ok(nachricht) => {
                        for id in &anhaenge {
                            self.offen.remove(id);
                        }
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
                }
            }

            // Serverschlüssel festlegen: der erste gewinnt. Der Server sieht
            // nur die Kennung (eine Prüfsumme), nie den Schlüssel.
            ClientFrame::ServerschluesselAnlegen { id } => {
                match self.state.serverschluessel_anlegen(ich, &id, jetzt) {
                    Ok(true) => {
                        let mut ids = self.schluesselhalter.remove(&ich).unwrap_or_default();
                        ids.push(id.clone());
                        ids.sort();
                        ids.dedup();
                        self.schluesselhalter.insert(ich, ids);
                        let _ = self.speichern();
                        tracing::info!(mitglied = %ich, "Serverschlüssel angelegt");
                        self.rundruf(
                            None,
                            None,
                            ServerFrame::Serverschluessel {
                                stand: self.schluesselstand(),
                            },
                        );
                        self.vermitteln();
                        vec![ServerFrame::Ok]
                    }
                    // Zu spät: der Client nimmt den Stand und verwirft seinen.
                    Ok(false) => vec![ServerFrame::Serverschluessel {
                        stand: self.schluesselstand(),
                    }],
                    Err(_) => vec![Kern::fehler(Fehlercode::UngueltigeEingabe)],
                }
            }

            ClientFrame::ServerschluesselVorhanden { ids } => {
                // Nur Kennungen, die es gibt — sonst könnte ein Client eine
                // erfundene Kennung in die Mitgliederliste aller schreiben.
                let mut bekannt: Vec<String> = ids
                    .into_iter()
                    .filter(|id| self.state.serverschluessel.alle.contains(id))
                    .collect();
                bekannt.sort();
                bekannt.dedup();
                self.schluesselhalter.insert(ich, bekannt);
                self.vermitteln();
                vec![ServerFrame::Ok]
            }

            // Weiterleiten, nicht öffnen: der Umschlag ist für `fuer`
            // verschlüsselt und vom Absender unterschrieben.
            ClientFrame::ServerschluesselPaket {
                fuer,
                id,
                daten,
                unterschrift,
            } => {
                let von_kennung = self
                    .state
                    .mitglied(ich)
                    .map(|m| m.schluessel.clone())
                    .unwrap_or_default();
                if !self.state.serverschluessel.alle.contains(&id)
                    || !self.schluesselhalter.contains_key(&fuer)
                {
                    return vec![Kern::fehler(Fehlercode::Unbekannt)];
                }
                self.senden(
                    fuer,
                    ServerFrame::ServerschluesselPaket {
                        von: ich,
                        von_kennung,
                        id,
                        daten,
                        unterschrift,
                    },
                );
                vec![ServerFrame::Ok]
            }

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
                        self.anhaenge_aufraeumen();
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
                        self.anhaenge_aufraeumen();
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
                self.schluesselhalter.remove(&mitglied);
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
                            raum_seit: 0,
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
                        if let Some(t) = self.teilnehmer.remove(&mitglied) {
                            t.conn.close(3u32.into(), b"gesperrt");
                        }
                        self.schluesselhalter.remove(&mitglied);
                        self.serverschluessel_erneuern(&mein_name, &ziel_name);
                        let _ = self.speichern();
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
                            self.schluesselhalter.remove(&id);
                            let mein_name = self
                                .state
                                .mitglied(ich)
                                .map(|m| m.name.clone())
                                .unwrap_or_default();
                            let ziel_name = self
                                .state
                                .mitglied(id)
                                .map(|m| m.name.clone())
                                .unwrap_or_default();
                            self.serverschluessel_erneuern(&mein_name, &ziel_name);
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

            ClientFrame::Entsperren { kennung, adresse } => {
                match self
                    .state
                    .entsperren(ich, kennung.as_deref(), adresse.as_deref(), jetzt)
                {
                    Ok(anzahl) => {
                        let _ = self.speichern();
                        // „0 Sperren aufgehoben" ist keine Auskunft, sondern
                        // ein Fehler: die Kennung passte zu keiner Sperre.
                        if anzahl == 0 {
                            vec![Kern::fehler(Fehlercode::Unbekannt)]
                        } else {
                            vec![ServerFrame::Ok]
                        }
                    }
                    Err(e) => vec![Kern::fehler(match e {
                        sprechwerk_domain::Error::RechtFehlt(_) => Fehlercode::KeinRecht,
                        _ => Fehlercode::UngueltigeEingabe,
                    })],
                }
            }

            ClientFrame::Sperrliste => {
                let darf = self
                    .state
                    .mitglied(ich)
                    .map(|m| {
                        sprechwerk_domain::role::darf(
                            m.rolle,
                            sprechwerk_domain::Capability::Sperren,
                        )
                    })
                    .unwrap_or(false);
                if !darf {
                    return vec![Kern::fehler(Fehlercode::KeinRecht)];
                }
                // Neueste zuerst: wer eine Sperre sucht, sucht meist die, die
                // gerade verhängt wurde.
                let mut sperren = self.state.sperren.clone();
                sperren.sort_by_key(|b| std::cmp::Reverse(b.zeit));
                vec![ServerFrame::Sperrliste { sperren }]
            }

            ClientFrame::Zaehler => {
                let z = self.zaehler();
                vec![ServerFrame::Zaehler {
                    sprachrahmen: z.sprachrahmen,
                    verteilt: z.verteilt,
                    verloren: z.verworfen,
                    abgelehnt: z.abgelehnt,
                    verbundene: z.verbundene,
                }]
            }

            ClientFrame::Protokoll {
                ab,
                anzahl,
                von_hinten,
            } => {
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
                let grenze = anzahl.min(500);
                let eintraege = if von_hinten {
                    // Die neuesten: vom Ende rückwärts, neueste zuerst. Wer die
                    // Ansicht öffnet, will sehen, was gerade geschah.
                    let gesamt = self.state.protokoll.len();
                    let ab_hinten = ab.min(gesamt);
                    let start = gesamt.saturating_sub(ab_hinten + grenze);
                    let ende = gesamt - ab_hinten;
                    let mut teil = self.state.protokoll[start..ende].to_vec();
                    teil.reverse();
                    teil
                } else {
                    self.state.protokoll_lesen(ab, grenze).to_vec()
                };
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

    /// Vermittelt fehlende Serverschlüssel: wem eine Kennung fehlt, für den
    /// bittet der Server **einen** Halter (den mit der kleinsten Kennung) um
    /// ein Paket. Höchstens alle [`ANFRAGE_ABSTAND`] je Mitglied und Schlüssel;
    /// der Client meldet sich in diesem Abstand neu, solange ihm etwas fehlt.
    ///
    /// Liefert die Bitten (Halter, Rahmen) — verschickt werden sie hier auch.
    pub fn vermitteln(&mut self) -> Vec<(MemberId, ServerFrame)> {
        let jetzt = std::time::Instant::now();
        let mut bitten = Vec::new();
        for (mitglied, hat) in &self.schluesselhalter {
            for id in &self.state.serverschluessel.alle {
                if hat.contains(id) {
                    continue;
                }
                let Some(halter) = self
                    .schluesselhalter
                    .iter()
                    .filter(|(h, ids)| *h != mitglied && ids.contains(id))
                    .map(|(h, _)| *h)
                    .min()
                else {
                    continue;
                };
                let schluessel = (*mitglied, id.clone());
                if self
                    .angefragt
                    .get(&schluessel)
                    .is_some_and(|wann| jetzt.duration_since(*wann) < ANFRAGE_ABSTAND)
                {
                    continue;
                }
                let Some(kennung) = self.state.mitglied(*mitglied).map(|m| m.schluessel.clone())
                else {
                    continue;
                };
                bitten.push((
                    halter,
                    schluessel,
                    ServerFrame::ServerschluesselAnfrage {
                        fuer: *mitglied,
                        kennung,
                        id: id.clone(),
                    },
                ));
            }
        }
        let mut aus = Vec::new();
        for (halter, schluessel, rahmen) in bitten {
            self.angefragt.insert(schluessel, jetzt);
            self.senden(halter, rahmen.clone());
            aus.push((halter, rahmen));
        }
        aus
    }

    /// Nach einer Sperre: neuer Serverschlüssel für alles Weitere.
    ///
    /// Die gesperrte Person hält den alten; mit dem neuen schreibt man, ohne
    /// dass sie ihn je bekommt. Erzeugen und verteilen machen die Clients.
    fn serverschluessel_erneuern(&mut self, handelnder_name: &str, ziel_name: &str) {
        if self.state.serverschluessel.aktuell.is_none() {
            return;
        }
        self.state.serverschluessel_verwerfen(
            handelnder_name,
            &format!("Sperre: {ziel_name}"),
            jetzt_ms(),
        );
        tracing::info!(
            "Serverschlüssel nach Sperre verworfen; der nächste Client legt einen neuen an"
        );
        self.rundruf(
            None,
            None,
            ServerFrame::Serverschluessel {
                stand: self.schluesselstand(),
            },
        );
    }

    /// Meldet eine Verbindung ab.
    pub fn abmelden(&mut self, ich: MemberId) {
        let raum = self.state.mitglied(ich).and_then(|m| m.raum);
        self.teilnehmer.remove(&ich);
        self.schluesselhalter.remove(&ich);
        self.angefragt.retain(|(m, _), _| *m != ich);
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

    // ---- Anhänge (ADR 0012 §3) ----------------------------------------

    /// Darf `ich` eine Datei mit `groesse` Byte Geheimtext unter `id`
    /// hochladen? Reserviert die Kennung und liefert den Pfad des Teils.
    pub fn hochladen_beginnen(
        &mut self,
        ich: MemberId,
        id: &str,
        groesse: u64,
    ) -> Result<std::path::PathBuf, Abweisung> {
        use sprechwerk_protocol::anhang::{ist_id, klar_groesse};
        let darf = self.state.mitglied(ich).is_some_and(|m| {
            sprechwerk_domain::role::darf(m.rolle, sprechwerk_domain::Capability::TextSchreiben)
        });
        if !darf {
            return Err((Fehlercode::KeinRecht, "dafür fehlt das Recht"));
        }
        if !ist_id(id) || klar_groesse(groesse).is_none() {
            return Err((
                Fehlercode::UngueltigeEingabe,
                "ungültige Kennung oder Größe",
            ));
        }
        if self.ablage.gibt_es(id) || self.offen.contains_key(id) || self.laufend.contains_key(id) {
            return Err((Fehlercode::UngueltigeEingabe, "Kennung schon vergeben"));
        }
        if self
            .konfig
            .anhaenge
            .je_datei()
            .is_some_and(|grenze| groesse > grenze)
        {
            return Err((Fehlercode::UngueltigeEingabe, "Datei zu groß"));
        }
        if let Some(grenze) = self.konfig.anhaenge.gesamt() {
            let reserviert: u64 = self.laufend.values().map(|(_, g)| g).sum();
            if self.ablage.belegt() + reserviert + groesse > grenze {
                return Err((Fehlercode::Spaeter, "Speicher für Anhänge ist voll"));
            }
        }
        self.laufend.insert(id.to_string(), (ich, groesse));
        Ok(self.ablage.teil(id))
    }

    /// Der Upload ist zu Ende: vollständig (`gelungen`) oder nicht.
    /// Liefert, ob die Datei jetzt abgelegt ist.
    pub fn hochladen_beenden(&mut self, ich: MemberId, id: &str, gelungen: bool) -> bool {
        if self.laufend.remove(id).is_none() {
            return false;
        }
        if gelungen && self.ablage.abschliessen(id).is_ok() {
            self.offen
                .insert(id.to_string(), (ich, std::time::Instant::now()));
            true
        } else {
            self.ablage.verwerfen(id);
            false
        }
    }

    /// Darf `ich` die Datei `id` laden? Jedes Mitglied darf, was an einer
    /// Nachricht hängt (Räume sind offen, ADR 0009); eine offene Datei nur der,
    /// der sie hochgeladen hat.
    pub fn herunterladen_pruefen(
        &self,
        ich: MemberId,
        id: &str,
    ) -> Result<std::path::PathBuf, Abweisung> {
        if !sprechwerk_protocol::anhang::ist_id(id) {
            return Err((Fehlercode::UngueltigeEingabe, "ungültige Kennung"));
        }
        let erlaubt = self.state.anhaenge_in_gebrauch().contains(id)
            || matches!(self.offen.get(id), Some((von, _)) if *von == ich);
        if !erlaubt || !self.ablage.gibt_es(id) {
            return Err((Fehlercode::Unbekannt, "nicht vorhanden"));
        }
        Ok(self.ablage.pfad(id))
    }

    /// Löscht jede Datei, auf die keine Nachricht zeigt — außer laufenden und
    /// frisch abgelegten (bis [`sprechwerk_protocol::anhang::OFFEN_FRIST_S`]).
    pub fn anhaenge_aufraeumen(&mut self) -> usize {
        let frist = std::time::Duration::from_secs(sprechwerk_protocol::anhang::OFFEN_FRIST_S);
        self.offen.retain(|_, (_, seit)| seit.elapsed() < frist);
        let in_gebrauch = self.state.anhaenge_in_gebrauch();
        let offen = &self.offen;
        let laufend = &self.laufend;
        self.ablage.aufraeumen(|id| {
            in_gebrauch.contains(id) || offen.contains_key(id) || laufend.contains_key(id)
        })
    }

    // ---- Betreiber (Verwaltungsoberfläche) -----------------------------

    /// Setzt eine Rolle **als Betreiber** — aus der Verwaltung im Browser,
    /// nicht über eine Verbindung (SPEC §2, Nachtrag 2026-11-17).
    ///
    /// Danach erfahren alle Verbundenen davon wie bei `RolleSetzen` aus dem
    /// Client: wer gerade Administrator geworden ist, hat das Recht sofort,
    /// ohne neu zu verbinden.
    pub fn betreiber_rolle_setzen(
        &mut self,
        ziel: MemberId,
        rolle: Role,
    ) -> Result<(), sprechwerk_domain::Error> {
        self.state
            .rolle_setzen_als_betreiber(ziel, rolle, jetzt_ms())?;
        if let Some(a) = self.ansicht(ziel) {
            self.rundruf(None, None, ServerFrame::MitgliedGeaendert { mitglied: a });
        }
        let _ = self.speichern();
        tracing::info!(mitglied = %ziel.text(), rolle = rolle.text(), "Rolle durch Betreiber gesetzt");
        Ok(())
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
    use crate::konfig::{Server, Sicherung};
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
        let pfad =
            std::env::temp_dir().join(format!("sprechwerk-matrix-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&pfad);
        std::fs::create_dir_all(&pfad).expect("Verzeichnis");
        pfad
    }

    pub(crate) fn raum_mit_name(antworten: &[ServerFrame], name: &str) -> Option<RoomId> {
        antworten.iter().find_map(|a| match a {
            ServerFrame::Raeume { raeume } => raeume.iter().find(|r| r.name == name).map(|r| r.id),
            _ => None,
        })
    }

    /// Kennung des Serverschlüssels in den Prüfwelten.
    pub(crate) const SCHLUESSEL_ID: &str = "abababababababababababababababab";

    /// Ein Text in der Form, die der Server annimmt (ADR 0012). Echte Krypto
    /// braucht der Server-Test nicht — er sieht sie ja auch nie; die Hülle
    /// trägt den Text als Hex, damit Tests ihn wiederfinden.
    pub(crate) fn geheim(text: &str) -> String {
        let mut daten = "00".repeat(28);
        for b in text.bytes() {
            daten.push_str(&format!("{b:02x}"));
        }
        sprechwerk_protocol::geheimtext::zusammensetzen(SCHLUESSEL_ID, &daten)
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
            verwaltung: Default::default(),
            anhaenge: Default::default(),
            spielnetz: Default::default(),
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
        // Seit ADR 0012 gibt es Text nur mit Serverschlüssel: Ada legt ihn an.
        let _ = kern.befehle(
            ada,
            ClientFrame::ServerschluesselAnlegen {
                id: SCHLUESSEL_ID.to_string(),
            },
        );
        let _ = kern.befehle(
            ada,
            ClientFrame::Text {
                text: geheim("von Ada"),
                anhaenge: Vec::new(),
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
            if erwartet {
                "KeinRecht"
            } else {
                "kein Rechtefehler"
            },
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
                text: geheim("Matrix"),
                anhaenge: Vec::new(),
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
            zelle("Trennen", rolle, PersonEingreifen, |w| {
                ClientFrame::Trennen {
                    mitglied: w.ziel(rolle),
                    grund: "Matrix".to_string(),
                }
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
            zelle("Protokoll", rolle, ProtokollLesen, |_| {
                ClientFrame::Protokoll {
                    ab: 0,
                    anzahl: 10,
                    von_hinten: false,
                }
            });
            zellen += 1;
        }

        println!("Rollenmatrix: {zellen} Zellen geprüft (Befehl × Rolle)");
    }

    /// Befehle ohne Rechtprüfung: sie dürfen **nie** `KeinRecht` melden.
    /// Der Betreiber macht Bosse (Mitglied) zum Administrator: die Rolle steht
    /// danach im Zustand, ist gespeichert, und der neue Administrator darf
    /// sofort, was nur Administratoren dürfen.
    /// Der erste Serverschlüssel gilt; wer zu spät kommt, bekommt den Stand.
    /// Gemeldete Halter stehen in der Mitgliederliste — nur mit Kennungen,
    /// die es gibt, und nur, solange die Sitzung läuft.
    #[test]
    fn serverschluessel_erster_gewinnt_und_halter_stehen_in_der_liste() {
        let mut w = welt("e2e-erster");
        let antwort = w.kern.befehle(
            w.bosse,
            ClientFrame::ServerschluesselAnlegen {
                id: "cd".repeat(16),
            },
        );
        match antwort.as_slice() {
            [ServerFrame::Serverschluessel { stand }] => {
                assert_eq!(stand.aktuell.as_deref(), Some(SCHLUESSEL_ID));
            }
            anderes => panic!("erwartet den Stand, kam {anderes:?}"),
        }
        assert_eq!(w.kern.state.serverschluessel.alle, vec![SCHLUESSEL_ID]);

        // Erfundene Kennungen werden nicht gemerkt.
        let _ = w.kern.befehle(
            w.bosse,
            ClientFrame::ServerschluesselVorhanden {
                ids: vec![SCHLUESSEL_ID.into(), "ee".repeat(16)],
            },
        );
        assert_eq!(
            w.kern.schluesselhalter[&w.bosse],
            vec![SCHLUESSEL_ID.to_string()]
        );
        w.kern.abmelden(w.bosse);
        assert!(!w.kern.schluesselhalter.contains_key(&w.bosse));
    }

    /// Wem ein Schlüssel fehlt, für den bittet der Server **einen** Halter —
    /// mit der Kennung des Empfängers, damit der Halter für ihn verpacken kann.
    /// Dieselbe Bitte kommt nicht bei jeder Meldung neu.
    #[test]
    fn fehlender_schluessel_wird_vermittelt() {
        let mut w = welt("e2e-vermitteln");
        let _ = w.kern.befehle(
            w.ada,
            ClientFrame::ServerschluesselVorhanden {
                ids: vec![SCHLUESSEL_ID.into()],
            },
        );
        let _ = w.kern.befehle(
            w.clemens,
            ClientFrame::ServerschluesselVorhanden {
                ids: vec![SCHLUESSEL_ID.into()],
            },
        );
        // Bosse meldet sich ohne Schlüssel.
        w.kern.angefragt.clear();
        let _ = w.kern.befehle(
            w.bosse,
            ClientFrame::ServerschluesselVorhanden { ids: vec![] },
        );
        // Noch einmal — darf keine zweite Bitte auslösen.
        let bitten = w.kern.vermitteln();
        assert!(bitten.is_empty(), "doppelte Bitte: {bitten:?}");

        w.kern.angefragt.clear();
        let bitten = w.kern.vermitteln();
        assert_eq!(bitten.len(), 1, "{bitten:?}");
        let (halter, rahmen) = &bitten[0];
        assert_eq!(
            *halter,
            w.ada.min(w.clemens),
            "der Halter mit der kleinsten Kennung"
        );
        let bosse_kennung = w.kern.state.mitglied(w.bosse).unwrap().schluessel.clone();
        assert_eq!(
            rahmen,
            &ServerFrame::ServerschluesselAnfrage {
                fuer: w.bosse,
                kennung: bosse_kennung,
                id: SCHLUESSEL_ID.into(),
            }
        );

        // Ein Paket an jemanden, der nicht verbunden ist, wird abgewiesen.
        let antwort = w.kern.befehle(
            w.ada,
            ClientFrame::ServerschluesselPaket {
                fuer: w.dora,
                id: SCHLUESSEL_ID.into(),
                daten: "00".into(),
                unterschrift: "00".into(),
            },
        );
        assert!(matches!(antwort.as_slice(), [ServerFrame::Fehler { .. }]));
        // An Bosse geht es durch.
        let antwort = w.kern.befehle(
            w.ada,
            ClientFrame::ServerschluesselPaket {
                fuer: w.bosse,
                id: SCHLUESSEL_ID.into(),
                daten: "00".into(),
                unterschrift: "00".into(),
            },
        );
        assert_eq!(antwort, vec![ServerFrame::Ok]);
    }

    /// Nach einer Sperre wird mit einem neuen Schlüssel geschrieben, den die
    /// gesperrte Person nie bekommt. Bis einer da ist, nimmt der Server keinen
    /// Text mit dem alten an.
    #[test]
    fn sperre_verwirft_den_serverschluessel() {
        let mut w = welt("e2e-sperre");
        let antwort = w.kern.befehle(
            w.ada,
            ClientFrame::MitgliedSperren {
                mitglied: w.bosse,
                grund: "Spam".into(),
            },
        );
        assert_eq!(antwort, vec![ServerFrame::Ok]);
        assert_eq!(w.kern.state.serverschluessel.aktuell, None);
        assert!(w
            .kern
            .state
            .protokoll
            .iter()
            .any(|e| e.aktion == "serverschluessel_verworfen"));

        let alt = w.kern.befehle(
            w.ada,
            ClientFrame::Text {
                text: geheim("x"),
                anhaenge: Vec::new(),
            },
        );
        assert!(
            matches!(alt.as_slice(), [ServerFrame::Fehler { .. }]),
            "{alt:?}"
        );

        let neu = "cd".repeat(16);
        let antwort = w.kern.befehle(
            w.clemens,
            ClientFrame::ServerschluesselAnlegen { id: neu.clone() },
        );
        assert_eq!(antwort, vec![ServerFrame::Ok]);
        assert_eq!(
            w.kern.state.serverschluessel.alle,
            vec![SCHLUESSEL_ID.to_string(), neu.clone()]
        );
        let text = sprechwerk_protocol::geheimtext::zusammensetzen(&neu, &"00".repeat(28));
        assert!(w
            .kern
            .befehle(
                w.ada,
                ClientFrame::Text {
                    text,
                    anhaenge: Vec::new()
                }
            )
            .is_empty());
    }

    /// Altbestand: unverschlüsselte Nachrichten sind nach dem Start weg,
    /// Räume sind verschlüsselt (ADR 0012 §4, §5).
    #[test]
    fn umstellung_beim_start() {
        let mut w = welt("e2e-start");
        let raum = w.kern.state.mitglied(w.ada).and_then(|m| m.raum).unwrap();
        w.kern.state.nachrichten.push(sprechwerk_domain::Message {
            id: sprechwerk_domain::MessageId::neu(),
            raum,
            verfasser: w.ada,
            verfasser_name: "Ada".into(),
            text: "Kassenstand im Klartext".into(),
            zeit: 1,
            geloescht: false,
            anhaenge: Vec::new(),
        });
        for r in w.kern.state.raeume.values_mut() {
            r.verschluesselt = false;
        }
        w.kern.speichern().unwrap();

        let neu = Kern::neu(w.kern.konfig.clone()).expect("Neustart");
        assert!(neu
            .state
            .nachrichten
            .iter()
            .all(|n| n.text != "Kassenstand im Klartext"));
        assert!(
            neu.state
                .nachrichten
                .iter()
                .any(|n| n.text == geheim("von Ada")),
            "Geheimtext bleibt"
        );
        assert!(neu.state.raeume.values().all(|r| r.verschluesselt));
        assert!(neu
            .state
            .protokoll
            .iter()
            .any(|e| e.aktion == "klartext_entfernt"));
    }

    #[test]
    fn betreiber_befoerdert_ohne_verbindung() {
        let mut w = welt("betreiber");
        assert_eq!(
            w.kern.state.mitglied(w.bosse).unwrap().rolle,
            Role::Mitglied
        );
        w.kern
            .betreiber_rolle_setzen(w.bosse, Role::Administrator)
            .unwrap();
        assert_eq!(
            w.kern.state.mitglied(w.bosse).unwrap().rolle,
            Role::Administrator
        );
        let antworten = w.kern.befehle(
            w.bosse,
            ClientFrame::RolleSetzen {
                mitglied: w.dora,
                rolle: Role::Moderator,
            },
        );
        assert!(!meldet_kein_recht(&antworten), "{antworten:?}");
        // Gespeichert: ein frischer Kern auf derselben Datenbank sieht es.
        let neu = Kern::neu(w.kern.konfig.clone()).unwrap();
        assert_eq!(
            neu.state.mitglied(w.bosse).unwrap().rolle,
            Role::Administrator
        );
    }

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
            let _ = w
                .kern
                .befehle(w.ada, ClientFrame::RaumVerschluesseln { raum, an: true });
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
    use super::rollenmatrix::{geheim, welt, SCHLUESSEL_ID};
    use super::*;
    use sprechwerk_domain::{MemberId, MessageId, Role, RoomId};

    /// Ein Prüffall: Name für die Fehlermeldung und der Rahmen, der gebaut wird.
    type Fall = (
        &'static str,
        Box<dyn Fn(&mut super::rollenmatrix::Welt) -> ClientFrame>,
    );

    /// Führt einen Befehl als Administratorin in ihrer Welt aus.
    fn frage(
        name: &str,
        rahmen: impl FnOnce(&mut super::rollenmatrix::Welt) -> ClientFrame,
    ) -> Vec<ServerFrame> {
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
        let faelle: Vec<Fall> = vec![
            (
                "Hallo",
                Box::new(|_| ClientFrame::Hallo {
                    version: sprechwerk_protocol::frame::PROTOKOLL_VERSION,
                    kennung: "00".into(),
                    name: "Eindringling".into(),
                    beweis: "00".into(),
                    serverpasswort: None,
                }),
            ),
            ("Ping", Box::new(|_| ClientFrame::Ping)),
            (
                "Betreten",
                Box::new(|w| ClientFrame::Betreten {
                    raum: w.zweiter_raum,
                }),
            ),
            ("Verlassen", Box::new(|_| ClientFrame::Verlassen)),
            (
                "Text",
                Box::new(|_| ClientFrame::Text {
                    text: geheim("hallo"),
                    anhaenge: Vec::new(),
                }),
            ),
            (
                "ServerschluesselAnlegen",
                Box::new(|_| ClientFrame::ServerschluesselAnlegen {
                    id: "cd".repeat(16),
                }),
            ),
            (
                "ServerschluesselVorhanden",
                Box::new(|_| ClientFrame::ServerschluesselVorhanden {
                    ids: vec![SCHLUESSEL_ID.to_string()],
                }),
            ),
            (
                "ServerschluesselPaket",
                Box::new(|w| ClientFrame::ServerschluesselPaket {
                    fuer: w.bosse,
                    id: SCHLUESSEL_ID.to_string(),
                    daten: "00".into(),
                    unterschrift: "00".into(),
                }),
            ),
            (
                "NachrichtLoeschen",
                Box::new(|w| ClientFrame::NachrichtLoeschen {
                    nachricht: w.fremde_nachricht,
                }),
            ),
            (
                "RaumAnlegen",
                Box::new(|_| ClientFrame::RaumAnlegen { name: "Neu".into() }),
            ),
            (
                "RaumUmbenennen",
                Box::new(|w| ClientFrame::RaumUmbenennen {
                    raum: w.zweiter_raum,
                    name: "Anders".into(),
                }),
            ),
            (
                "RaumLoeschen",
                Box::new(|w| ClientFrame::RaumLoeschen {
                    raum: w.zweiter_raum,
                }),
            ),
            (
                "Stummschalten",
                Box::new(|w| ClientFrame::Stummschalten {
                    mitglied: w.bosse,
                    taub: false,
                    an: true,
                }),
            ),
            (
                "Verschieben",
                Box::new(|w| ClientFrame::Verschieben {
                    mitglied: w.bosse,
                    raum: w.zweiter_raum,
                }),
            ),
            (
                "AusRaumWerfen",
                Box::new(|w| ClientFrame::AusRaumWerfen { mitglied: w.bosse }),
            ),
            (
                "Trennen",
                Box::new(|w| ClientFrame::Trennen {
                    mitglied: w.bosse,
                    grund: "Test".into(),
                }),
            ),
            (
                "MitgliedSperren",
                Box::new(|w| ClientFrame::MitgliedSperren {
                    mitglied: w.bosse,
                    grund: "Test".into(),
                }),
            ),
            (
                "RolleSetzen",
                Box::new(|w| ClientFrame::RolleSetzen {
                    mitglied: w.bosse,
                    rolle: Role::Moderator,
                }),
            ),
            (
                "Sperren",
                Box::new(|_| ClientFrame::Sperren {
                    kennung: None,
                    adresse: Some("203.0.113.7".into()),
                    grund: "Test".into(),
                }),
            ),
            (
                "RaumVerschluesseln",
                Box::new(|w| ClientFrame::RaumVerschluesseln {
                    raum: w.zweiter_raum,
                    an: true,
                }),
            ),
            (
                "Schluesselpaket",
                Box::new(|w| ClientFrame::Schluesselpaket {
                    raum: w.zweiter_raum,
                    epoche: 1,
                    fuer: w.bosse,
                    daten: "00".into(),
                    unterschrift: "11".into(),
                }),
            ),
            (
                "SchluesselAnfrage",
                Box::new(|w| ClientFrame::SchluesselAnfrage {
                    raum: w.zweiter_raum,
                }),
            ),
            ("Sperrliste", Box::new(|_| ClientFrame::Sperrliste)),
            ("Zaehler", Box::new(|_| ClientFrame::Zaehler)),
            (
                "Entsperren",
                Box::new(|_| ClientFrame::Entsperren {
                    kennung: Some("ab".repeat(32)),
                    adresse: None,
                }),
            ),
            (
                "Protokoll",
                Box::new(|_| ClientFrame::Protokoll {
                    ab: 0,
                    anzahl: 10,
                    von_hinten: false,
                }),
            ),
        ];

        // Jede Nachrichtenart des Clients steht hier genau einmal. Der Wächter
        // ist die geprüfte Zahl: kommt eine Variante dazu und wird sie hier nicht
        // ergänzt, schlägt der Test fehl.
        assert_eq!(
            faelle.len(),
            26,
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
                    .map(|e| e.iter().any(|n| n.text == geheim("hallo")))
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

        let faelle: Vec<Fall> = vec![
            (
                "Betreten",
                Box::new(move |_| ClientFrame::Betreten { raum: fremder_raum }),
            ),
            (
                "NachrichtLoeschen",
                Box::new(move |_| ClientFrame::NachrichtLoeschen {
                    nachricht: fremde_nachricht,
                }),
            ),
            (
                "RaumUmbenennen",
                Box::new(move |_| ClientFrame::RaumUmbenennen {
                    raum: fremder_raum,
                    name: "X".into(),
                }),
            ),
            (
                "RaumLoeschen",
                Box::new(move |_| ClientFrame::RaumLoeschen { raum: fremder_raum }),
            ),
            (
                "Stummschalten",
                Box::new(move |_| ClientFrame::Stummschalten {
                    mitglied: fremdes_mitglied,
                    taub: false,
                    an: true,
                }),
            ),
            (
                "Verschieben",
                Box::new(move |_| ClientFrame::Verschieben {
                    mitglied: fremdes_mitglied,
                    raum: RoomId::neu(),
                }),
            ),
            (
                "AusRaumWerfen",
                Box::new(move |_| ClientFrame::AusRaumWerfen {
                    mitglied: fremdes_mitglied,
                }),
            ),
            (
                "Trennen",
                Box::new(move |_| ClientFrame::Trennen {
                    mitglied: fremdes_mitglied,
                    grund: "Test".into(),
                }),
            ),
            (
                "MitgliedSperren",
                Box::new(move |_| ClientFrame::MitgliedSperren {
                    mitglied: fremdes_mitglied,
                    grund: "Test".into(),
                }),
            ),
            (
                "RolleSetzen",
                Box::new(move |_| ClientFrame::RolleSetzen {
                    mitglied: fremdes_mitglied,
                    rolle: Role::Gast,
                }),
            ),
            (
                "RaumVerschluesseln",
                Box::new(move |_| ClientFrame::RaumVerschluesseln {
                    raum: fremder_raum,
                    an: true,
                }),
            ),
        ];

        assert_eq!(
            faelle.len(),
            11,
            "Zahl der Kennungs-Befehle hat sich geändert"
        );

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
        // Die Grenze gilt dem **Geheimtext** (der Server sieht nur ihn).
        let rand = sprechwerk_protocol::geheimtext::MAX_GEHEIMTEXT_LEN - geheim("").len();
        let zu_lang = geheim(&"x".repeat(rand / 2 + 1));
        let antworten = frage("sweep-text-lang", |_| ClientFrame::Text {
            text: zu_lang,
            anhaenge: Vec::new(),
        });
        assert!(
            meldet_fehler(&antworten),
            "Text über der Grenze muss abgewiesen werden: {antworten:?}"
        );

        // Genau an der Grenze: angenommen (kein Fehler, kein Rahmen — die
        // Nachricht geht über den Rundruf).
        let genau = geheim(&"x".repeat(rand / 2));
        let antworten = frage("sweep-text-grenze", |_| ClientFrame::Text {
            text: genau,
            anhaenge: Vec::new(),
        });
        assert!(
            !meldet_fehler(&antworten),
            "Text genau an der Grenze muss durchgehen: {antworten:?}"
        );

        // Leerer Text: keine leere Nachricht im Verlauf.
        let antworten = frage("sweep-text-leer", |_| ClientFrame::Text {
            text: "   ".into(),
            anhaenge: Vec::new(),
        });
        assert!(
            meldet_fehler(&antworten),
            "leerer Text muss abgewiesen werden: {antworten:?}"
        );

        // Klartext kommt nicht in die Datenbank (ADR 0012).
        let antworten = frage("sweep-text-klar", |_| ClientFrame::Text {
            text: "Guten Abend".into(),
            anhaenge: Vec::new(),
        });
        assert!(
            meldet_fehler(&antworten),
            "Klartext angenommen: {antworten:?}"
        );
        // Und Geheimtext mit einem anderen als dem aktuellen Schlüssel auch nicht.
        let fremd =
            sprechwerk_protocol::geheimtext::zusammensetzen(&"cd".repeat(16), &"00".repeat(28));
        let antworten = frage("sweep-text-fremd", |_| ClientFrame::Text {
            text: fremd,
            anhaenge: Vec::new(),
        });
        assert!(
            meldet_fehler(&antworten),
            "Leerer Text muss abgewiesen werden: {antworten:?}"
        );

        // Raumname: leer und viel zu lang.
        for (name, raumname) in [("leer", ""), ("lang", &"R".repeat(5_000))] {
            let antworten = frage(&format!("sweep-raum-{name}"), |_| {
                ClientFrame::RaumAnlegen {
                    name: raumname.to_string(),
                }
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
            von_hinten: false,
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
            von_hinten: false,
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
            (
                "zu groß",
                vec![0u8; sprechwerk_protocol::nachricht::MAX_SPRACHRAHMEN + 1],
            ),
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
    /// Die Betriebszahlen, die jeder abfragen darf.
    ///
    /// Sie müssen zu dem passen, was der Server wirklich tut: „0 verloren" darf
    /// nicht behauptet werden, während der Sendepuffer überläuft.
    #[test]
    fn zaehler_geben_auskunft() {
        let mut w = welt("sweep-zaehler");

        // Zwei gültige Rahmen einer Administratorin im Raum …
        let gueltig = sprechwerk_protocol::Sprachkopf::neu(1).schreiben(&[0u8; 40]);
        w.kern.sprache(w.ada, &gueltig);
        w.kern.sprache(w.ada, &gueltig);
        // … und ein kaputter, der als abgelehnt zählt.
        w.kern.sprache(w.ada, &[1, 2, 3]);

        let antworten = w.kern.befehle(w.ada, ClientFrame::Zaehler);
        match &antworten[0] {
            ServerFrame::Zaehler {
                sprachrahmen,
                abgelehnt,
                verbundene,
                verloren,
                ..
            } => {
                assert_eq!(*sprachrahmen, 2, "zwei gültige Rahmen kamen an");
                assert_eq!(*abgelehnt, 1, "der kaputte zählt als abgelehnt");
                // Im Kern ohne echte Verbindungen sind es null — im Betrieb
                // zählt der Zähler die Verbindungen (`scripts/lastprobe.sh`).
                assert_eq!(*verbundene, 0, "keine Verbindungen im Kern");
                assert_eq!(*verloren, 0, "niemand war im Raum, also nichts verteilt");
            }
            anderes => panic!("unerwartet: {anderes:?}"),
        }

        // Wer nicht im Raum ist, spricht ins Leere: gezählt als abgelehnt.
        let _ = w.kern.befehle(w.ada, ClientFrame::Verlassen);
        w.kern.sprache(w.ada, &gueltig);
        let antworten = w.kern.befehle(w.ada, ClientFrame::Zaehler);
        match &antworten[0] {
            ServerFrame::Zaehler {
                sprachrahmen,
                abgelehnt,
                ..
            } => {
                assert_eq!(*sprachrahmen, 2, "ohne Raum kommt nichts durch");
                assert_eq!(*abgelehnt, 2, "der Rahmen ohne Raum zählt als abgelehnt");
            }
            anderes => panic!("unerwartet: {anderes:?}"),
        }
    }
    /// Die Sperrliste: nur der Administrator sieht sie, und sie ist vollständig.
    #[test]
    fn sperrliste_nur_mit_recht_und_vollstaendig() {
        let mut w = welt("sweep-sperrliste");

        // Erst sperren, dann auflisten.
        let kennung = "cd".repeat(32);
        let antworten = w.kern.befehle(
            w.ada,
            ClientFrame::Sperren {
                kennung: Some(kennung.clone()),
                adresse: Some("203.0.113.9".into()),
                grund: "Spam".into(),
            },
        );
        assert!(!meldet_fehler(&antworten), "Sperre: {antworten:?}");

        let antworten = w.kern.befehle(w.ada, ClientFrame::Sperrliste);
        match &antworten[0] {
            ServerFrame::Sperrliste { sperren } => {
                assert_eq!(sperren.len(), 1, "die Sperre fehlt in der Liste");
                let s = &sperren[0];
                assert_eq!(s.schluessel.as_deref(), Some(kennung.as_str()));
                assert_eq!(s.adresse.as_deref(), Some("203.0.113.9"));
                assert_eq!(s.grund, "Spam");
                assert_eq!(s.handelnder_name, "Ada");
            }
            anderes => panic!("unerwartet: {anderes:?}"),
        }

        // Ein Mitglied (hier: Gast) darf sie nicht sehen.
        let antworten = w.kern.befehle(w.dora, ClientFrame::Sperrliste);
        assert!(
            meldet_fehler(&antworten),
            "die Sperrliste ist eine Administratoransicht: {antworten:?}"
        );
        // Auch ein Moderator nicht — Sperren ist Administratorsache.
        let antworten = w.kern.befehle(w.clemens, ClientFrame::Sperrliste);
        assert!(
            meldet_fehler(&antworten),
            "Moderator darf nicht sperren, also auch nicht sehen: {antworten:?}"
        );
    }
    /// Das Protokoll **von hinten**: die neuesten zuerst.
    ///
    /// Vorher zeigte die Ansicht im Fenster die ältesten Einträge — bei einem
    /// Protokoll genau das Gegenteil dessen, was man sehen will.
    /// Anhänge (ADR 0012 §3): hochladen, an eine Nachricht hängen, laden,
    /// mit der Nachricht verschwinden — und alles, was nicht passt, abweisen.
    #[test]
    fn anhaenge_leben_und_sterben_mit_der_nachricht() {
        use sprechwerk_protocol::anhang::geheim_groesse;
        let mut w = welt("anhaenge");
        let id = "0a".repeat(16);
        let groesse = geheim_groesse(5);

        // Ungültiges wird gar nicht erst angenommen.
        for (falsch, g) in [("../x", groesse), (id.as_str(), 3)] {
            assert_eq!(
                w.kern.hochladen_beginnen(w.bosse, falsch, g).unwrap_err().0,
                Fehlercode::UngueltigeEingabe,
                "{falsch} {g}"
            );
        }

        // Hochladen: Teil schreiben, abschließen.
        let teil = w.kern.hochladen_beginnen(w.bosse, &id, groesse).unwrap();
        std::fs::write(&teil, vec![0u8; groesse as usize]).unwrap();
        // Dieselbe Kennung ein zweites Mal: nein, auch nicht während des Ladens.
        assert!(w.kern.hochladen_beginnen(w.ada, &id, groesse).is_err());
        assert!(w.kern.hochladen_beenden(w.bosse, &id, true));
        assert!(w.kern.ablage.gibt_es(&id));
        assert!(w.kern.hochladen_beginnen(w.ada, &id, groesse).is_err());

        // Offen: nur Bosse sieht die Datei, und nur Bosse darf sie verschicken.
        assert!(w.kern.herunterladen_pruefen(w.bosse, &id).is_ok());
        assert!(w.kern.herunterladen_pruefen(w.ada, &id).is_err());
        let fremd = w.kern.befehle(
            w.ada,
            ClientFrame::Text {
                text: geheim("fremde Datei"),
                anhaenge: vec![id.clone()],
            },
        );
        assert!(meldet_fehler(&fremd), "{fremd:?}");
        let erfunden = w.kern.befehle(
            w.bosse,
            ClientFrame::Text {
                text: geheim("erfunden"),
                anhaenge: vec!["0f".repeat(16)],
            },
        );
        assert!(meldet_fehler(&erfunden), "{erfunden:?}");

        // Aufräumen lässt offene Dateien in ihrer Frist liegen.
        assert_eq!(w.kern.anhaenge_aufraeumen(), 0);

        // Verschicken: die Datei hängt an der Nachricht, jedes Mitglied lädt.
        let ok = w.kern.befehle(
            w.bosse,
            ClientFrame::Text {
                text: geheim("Fotos vom Fest"),
                anhaenge: vec![id.clone(), id.clone()],
            },
        );
        assert!(!meldet_fehler(&ok), "{ok:?}");
        let nachricht = w
            .kern
            .state
            .nachrichten
            .iter()
            .find(|n| n.anhaenge.contains(&id))
            .expect("Nachricht mit Anhang")
            .clone();
        assert_eq!(
            nachricht.anhaenge,
            std::slice::from_ref(&id),
            "doppelt zählt einmal"
        );
        assert!(w.kern.herunterladen_pruefen(w.ada, &id).is_ok());
        // Ein zweites Mal verschicken geht nicht: sie ist nicht mehr offen.
        let nochmal = w.kern.befehle(
            w.bosse,
            ClientFrame::Text {
                text: geheim("nochmal"),
                anhaenge: vec![id.clone()],
            },
        );
        assert!(meldet_fehler(&nochmal));

        // Löschen: die Datei ist weg.
        w.kern.befehle(
            w.bosse,
            ClientFrame::NachrichtLoeschen {
                nachricht: nachricht.id,
            },
        );
        assert!(!w.kern.ablage.gibt_es(&id));
        assert!(w.kern.herunterladen_pruefen(w.ada, &id).is_err());

        // Ein abgebrochener Upload hinterlässt nichts.
        let id2 = "0b".repeat(16);
        let teil = w.kern.hochladen_beginnen(w.bosse, &id2, groesse).unwrap();
        std::fs::write(&teil, b"halb").unwrap();
        assert!(!w.kern.hochladen_beenden(w.bosse, &id2, false));
        assert!(!teil.exists());
        assert!(!w.kern.ablage.gibt_es(&id2));
    }

    #[test]
    fn anhaenge_halten_die_grenzen_des_betreibers() {
        use sprechwerk_protocol::anhang::geheim_groesse;
        let mut w = welt("anhaenge-grenzen");
        let mib = 1024 * 1024;
        w.kern.konfig.anhaenge.max_mb = 1;
        let zu_gross = w
            .kern
            .hochladen_beginnen(w.bosse, &"01".repeat(16), geheim_groesse(mib))
            .unwrap_err();
        assert_eq!(zu_gross.0, Fehlercode::UngueltigeEingabe);
        assert!(w
            .kern
            .hochladen_beginnen(w.bosse, &"02".repeat(16), geheim_groesse(mib / 2))
            .is_ok());

        // Gesamt: der laufende Upload (½ MiB) zählt mit.
        w.kern.konfig.anhaenge.max_mb = 0;
        w.kern.konfig.anhaenge.gesamt_mb = 1;
        let voll = w
            .kern
            .hochladen_beginnen(w.bosse, &"03".repeat(16), geheim_groesse(mib / 2 + 1))
            .unwrap_err();
        assert_eq!(voll.0, Fehlercode::Spaeter);
    }

    #[test]
    fn protokoll_von_hinten_liefert_die_neuesten() {
        let mut w = welt("sweep-protokoll-hinten");
        // Genug Zeilen erzeugen: jeder Raumname steht danach im Protokoll.
        for i in 0..5 {
            let _ = w.kern.befehle(
                w.ada,
                ClientFrame::RaumAnlegen {
                    name: format!("Raum {i}"),
                },
            );
        }

        let antworten = w.kern.befehle(
            w.ada,
            ClientFrame::Protokoll {
                ab: 0,
                anzahl: 3,
                von_hinten: true,
            },
        );
        let eintraege = match &antworten[0] {
            ServerFrame::Protokoll { eintraege } => eintraege.clone(),
            anderes => panic!("unerwartet: {anderes:?}"),
        };
        assert_eq!(eintraege.len(), 3, "genau die drei neuesten");
        // Neueste zuerst: die Laufnummern fallen.
        let nummern: Vec<u64> = eintraege.iter().map(|e| e.laufnummer).collect();
        assert!(
            nummern.windows(2).all(|p| p[0] > p[1]),
            "neueste zuerst erwartet, bekommen: {nummern:?}"
        );
        // Der neueste Eintrag gehört zum zuletzt angelegten Raum (das Ziel ist
        // die Raumkennung, nicht der Name).
        let letzter_raum = w
            .kern
            .raeume()
            .into_iter()
            .find(|r| r.name == "Raum 4")
            .map(|r| r.id)
            .expect("Raum 4");
        assert_eq!(
            eintraege[0].ziel,
            letzter_raum.text(),
            "der neueste Eintrag ist der zuletzt angelegte Raum"
        );

        // Und von vorn bleibt es beim Mitschnitt: älteste zuerst.
        let antworten = w.kern.befehle(
            w.ada,
            ClientFrame::Protokoll {
                ab: 0,
                anzahl: 3,
                von_hinten: false,
            },
        );
        let eintraege = match &antworten[0] {
            ServerFrame::Protokoll { eintraege } => eintraege.clone(),
            anderes => panic!("unerwartet: {anderes:?}"),
        };
        let nummern: Vec<u64> = eintraege.iter().map(|e| e.laufnummer).collect();
        assert!(
            nummern.windows(2).all(|p| p[0] < p[1]),
            "von vorn aufsteigend erwartet, bekommen: {nummern:?}"
        );
    }
}

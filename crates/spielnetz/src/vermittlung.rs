//! Der Zustandsautomat des Servers (ADR 0016 §6).
//!
//! Ereignisse hinein, Aktionen heraus — keine Verbindung, kein Strom, keine Uhr
//! außer der, die der Aufrufer mitgibt. Was der Server-Kern meldet, ist ein
//! [`Ereignis`]; was er tun soll, eine [`Aktion`]. Damit ist der ganze Ablauf
//! prüfbar, ohne einen Server zu starten.
//!
//! # Wer darf was
//!
//! Die Rechteprüfung (`Capability::Spielnetz`, ADR 0016 §8) macht der Kern, und
//! er sagt sie beim [`Ereignis::Verbunden`] mit (`darf`). Alles andere — Raum,
//! Grenzen, Gastgeberschaft — prüft dieser Automat. Er kennt weder Rollen noch
//! Räume, nur Kennungen und Raumzeichenketten.
//!
//! # Der Server weiß, wer wo ist
//!
//! Ein Angebot hängt **nicht** am Rahmen, sondern am Raum seines Gastgebers
//! (AGENTS.md §9: der Raum steht nie im Kopf). Verlässt der Gastgeber den Raum,
//! endet seine Freigabe; wechselt ein Teilnehmer den Raum, endet seine
//! Teilnahme. Beides läuft hier zusammen, damit kein I/O-Code diese Regel
//! wiederholen muss.
//!
//! # Reihenfolge
//!
//! Alle Listen sind `BTreeMap`s, alle Kennungen damit sortiert. Zwei gleiche
//! Ereignisfolgen ergeben dieselben Aktionen in derselben Reihenfolge — sonst
//! wären die Tests instabil und Fehlersuche ein Ratespiel.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::protokoll::{
    Angebot, AngebotId, Art, ClientRahmen, Fehler, Grenzen, Grund, Kanal, ServerRahmen, Teilnehmer,
};

/// Was der Server-Kern meldet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ereignis {
    /// Spielnetz-Verbindung angemeldet (Server hat Beweis, Sitzung und Recht
    /// geprüft).
    Verbunden {
        kennung: String,
        name: String,
        raum: Option<String>,
        darf: bool,
    },
    /// Die Verbindung ist weg.
    Getrennt { kennung: String },
    /// Der Kern meldet einen Raumwechsel (auch für Mitglieder ohne
    /// Spielnetz-Verbindung — die werden ignoriert).
    Raumwechsel {
        kennung: String,
        raum: Option<String>,
    },
    /// Ein Steuerrahmen vom Client. [`ClientRahmen::Hallo`] gehört nicht hierher.
    Rahmen {
        kennung: String,
        rahmen: ClientRahmen,
    },
}

/// Was der Server tun soll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aktion {
    /// Einen Rahmen an einen Client schicken (über dessen Steuerstrom).
    Senden { an: String, rahmen: ServerRahmen },
    /// Laufende Ströme und Kanäle dieses Paares abbauen. `teilnehmer = None`
    /// heißt: alle Teilnehmer des Angebots.
    Trennen {
        angebot: AngebotId,
        teilnehmer: Option<String>,
    },
}

/// Ein Mitglied, wie der Automat es braucht.
struct Mitglied {
    name: String,
    raum: Option<String>,
    darf: bool,
}

impl Mitglied {
    fn in_raum(&self, raum: &str) -> bool {
        self.raum.as_deref() == Some(raum)
    }
}

/// Eine Freigabe samt ihrer Datenebene.
struct Eintrag {
    angebot: Angebot,
    /// Offene TCP-Ströme je Teilnehmer.
    offene_stroeme: BTreeMap<String, u8>,
    /// Zuletzt vergebene Stromnummer je Teilnehmer (fortlaufend ab 1).
    naechste_stromnummer: BTreeMap<String, u32>,
    /// Kanal beim Gastgeber je (Teilnehmer, dessen lokaler Kanal). Ein Spiel
    /// öffnet oft mehrere UDP-Sockets; jeder bekommt beim Gastgeber einen
    /// eigenen Kanal, sonst landeten alle Antworten beim ersten.
    kanal_hin: BTreeMap<(String, Kanal), Kanal>,
    /// Teilnehmer und sein eigener Kanal je Kanal beim Gastgeber.
    kanal_her: BTreeMap<Kanal, (String, Kanal)>,
    /// Nächster freier Kanal beim Gastgeber.
    naechster_kanal: Kanal,
}

impl Eintrag {
    fn neu(angebot: Angebot) -> Self {
        Self {
            angebot,
            offene_stroeme: BTreeMap::new(),
            naechste_stromnummer: BTreeMap::new(),
            kanal_hin: BTreeMap::new(),
            kanal_her: BTreeMap::new(),
            naechster_kanal: 1,
        }
    }

    fn ist_teilnehmer(&self, kennung: &str) -> bool {
        self.angebot.teilnehmer.iter().any(|t| t.kennung == kennung)
    }

    /// Räumt alles auf, was zu einem Teilnehmer gehört. Die Angebotsliste
    /// selbst bleibt Sache des Aufrufers.
    fn teilnehmer_austragen(&mut self, kennung: &str) {
        self.angebot.teilnehmer.retain(|t| t.kennung != kennung);
        self.offene_stroeme.remove(kennung);
        self.naechste_stromnummer.remove(kennung);
        // Alle Kanäle dieses Teilnehmers, nicht nur der erste.
        self.kanal_hin
            .retain(|(teilnehmer, _), _| teilnehmer != kennung);
        self.kanal_her
            .retain(|_, (teilnehmer, _)| teilnehmer != kennung);
    }
}

/// Token-Eimer für die Durchsatzgrenze einer Freigabe.
struct Eimer {
    /// Vorrat in Byte.
    stand: f64,
    /// Zeitpunkt der letzten Auffüllung.
    letzte: Instant,
}

/// Der Zustandsautomat.
pub struct Vermittlung {
    grenzen: Grenzen,
    mitglieder: BTreeMap<String, Mitglied>,
    angebote: BTreeMap<AngebotId, Eintrag>,
    naechste_id: AngebotId,
    drossel: BTreeMap<AngebotId, Eimer>,
}

impl Vermittlung {
    /// Ein frischer Automat. Die Nummern der Freigaben beginnen bei 1, damit
    /// `0` „keine“ heißen kann.
    pub fn neu(grenzen: Grenzen) -> Self {
        Self {
            grenzen,
            mitglieder: BTreeMap::new(),
            angebote: BTreeMap::new(),
            naechste_id: 1,
            drossel: BTreeMap::new(),
        }
    }

    /// Die Grenzen, die dieser Automat durchsetzt.
    pub fn grenzen(&self) -> Grenzen {
        self.grenzen
    }

    /// Verarbeitet ein Ereignis und liefert die Aktionen in fester Reihenfolge.
    pub fn verarbeite(&mut self, e: Ereignis) -> Vec<Aktion> {
        match e {
            Ereignis::Verbunden {
                kennung,
                name,
                raum,
                darf,
            } => {
                self.mitglieder.insert(
                    kennung.clone(),
                    Mitglied {
                        name,
                        raum: raum.clone(),
                        darf,
                    },
                );
                let liste = self.liste(raum.as_deref());
                vec![
                    senden(
                        &kennung,
                        ServerRahmen::Willkommen {
                            grenzen: self.grenzen,
                        },
                    ),
                    senden(&kennung, ServerRahmen::Angebote { liste }),
                ]
            }
            Ereignis::Getrennt { kennung } => self.getrennt(&kennung),
            Ereignis::Raumwechsel { kennung, raum } => self.raumwechsel(&kennung, raum),
            Ereignis::Rahmen { kennung, rahmen } => {
                if !self.mitglieder.contains_key(&kennung) {
                    return vec![senden(&kennung, fehler(Fehler::NichtAngemeldet))];
                }
                match rahmen {
                    // Hier ist der Beweis längst geführt: ein zweites Hallo ist
                    // keine Anmeldung, sondern eine falsche Anfrage.
                    ClientRahmen::Hallo { .. } => {
                        vec![senden(&kennung, fehler(Fehler::UngueltigeAnfrage))]
                    }
                    ClientRahmen::Anbieten { name, port, art } => {
                        self.anbieten(&kennung, name, port, art)
                    }
                    ClientRahmen::Zurueckziehen { angebot } => {
                        self.zurueckziehen(&kennung, angebot)
                    }
                    ClientRahmen::Beitreten { angebot } => self.beitreten(&kennung, angebot),
                    ClientRahmen::Verlassen { angebot } => self.verlassen(&kennung, angebot),
                    ClientRahmen::Entfernen {
                        angebot,
                        kennung: ziel,
                    } => self.entfernen(&kennung, angebot, &ziel),
                }
            }
        }
    }

    // ---------------------------------------------------------------- Steuerung

    fn anbieten(&mut self, kennung: &str, name: String, port: u16, art: Art) -> Vec<Aktion> {
        let (darf, raum, gastgeber_name) = match self.mitglieder.get(kennung) {
            Some(m) => (m.darf, m.raum.clone(), m.name.clone()),
            None => return vec![senden(kennung, fehler(Fehler::NichtAngemeldet))],
        };
        if !darf {
            return vec![senden(kennung, fehler(Fehler::KeinRecht))];
        }
        let Some(raum) = raum else {
            return vec![senden(kennung, fehler(Fehler::KeinRaum))];
        };
        // Der Name erscheint im Fenster aller im Raum: keine Steuerzeichen,
        // nicht leer nach dem Trimmen.
        let name = name.trim().to_string();
        let zeichen = name.chars().count();
        if port == 0 || zeichen == 0 || zeichen > 64 || name.chars().any(char::is_control) {
            return vec![senden(kennung, fehler(Fehler::UngueltigeAnfrage))];
        }
        let eigene = self
            .angebote
            .values()
            .filter(|e| e.angebot.gastgeber == kennung)
            .count();
        if eigene >= usize::from(self.grenzen.freigaben_je_mitglied) {
            return vec![senden(kennung, fehler(Fehler::ZuVieleFreigaben))];
        }

        let id = self.naechste_id;
        self.naechste_id = self.naechste_id.saturating_add(1);
        let angebot = Angebot {
            id,
            name,
            port,
            art,
            gastgeber: kennung.to_string(),
            gastgeber_name,
            teilnehmer: Vec::new(),
        };
        self.angebote.insert(id, Eintrag::neu(angebot));

        let mut aktionen = vec![senden(kennung, ServerRahmen::Angeboten { angebot: id })];
        aktionen.extend(self.raum_liste(&raum));
        aktionen
    }

    fn zurueckziehen(&mut self, kennung: &str, angebot: AngebotId) -> Vec<Aktion> {
        let Some(eintrag) = self.angebote.get(&angebot) else {
            return vec![senden(kennung, fehler(Fehler::UnbekanntesAngebot))];
        };
        if eintrag.angebot.gastgeber != kennung {
            return vec![senden(kennung, fehler(Fehler::NichtGastgeber))];
        }
        let raum = self.raum_von_angebot(angebot);
        let mut aktionen = Vec::new();
        for teilnehmer in self.teilnehmer_von(angebot) {
            aktionen.push(senden(
                &teilnehmer,
                ServerRahmen::Beendet {
                    angebot,
                    grund: Grund::Zurueckgezogen,
                },
            ));
        }
        aktionen.push(Aktion::Trennen {
            angebot,
            teilnehmer: None,
        });
        self.angebote.remove(&angebot);
        self.drossel.remove(&angebot);
        if let Some(raum) = raum {
            aktionen.extend(self.raum_liste(&raum));
        }
        aktionen
    }

    fn beitreten(&mut self, kennung: &str, angebot: AngebotId) -> Vec<Aktion> {
        let Some(eintrag) = self.angebote.get(&angebot) else {
            return vec![senden(kennung, fehler(Fehler::UnbekanntesAngebot))];
        };
        let (darf, raum, name) = match self.mitglieder.get(kennung) {
            Some(m) => (m.darf, m.raum.clone(), m.name.clone()),
            None => return vec![senden(kennung, fehler(Fehler::NichtAngemeldet))],
        };
        let raum_des_angebots = self.raum_von_angebot(angebot);
        if raum != raum_des_angebots {
            return vec![senden(kennung, fehler(Fehler::AndererRaum))];
        }
        if eintrag.angebot.gastgeber == kennung {
            return vec![senden(kennung, fehler(Fehler::EigenesAngebot))];
        }
        if !darf {
            return vec![senden(kennung, fehler(Fehler::KeinRecht))];
        }
        // Doppeltes Beitreten ist kein Fehler: ein verlorener Rahmen darf den
        // Zustand nicht kaputt machen.
        if eintrag.ist_teilnehmer(kennung) {
            return vec![senden(kennung, ServerRahmen::Beigetreten { angebot })];
        }
        if eintrag.angebot.teilnehmer.len() >= usize::from(self.grenzen.teilnehmer_je_freigabe) {
            return vec![senden(kennung, fehler(Fehler::ZuVieleTeilnehmer))];
        }

        if let Some(eintrag) = self.angebote.get_mut(&angebot) {
            // Der Name gehört zur Teilnahme, nicht zum Rahmen: der Server hat
            // ihn bei der Anmeldung bekommen (ADR 0016 §0 P4).
            eintrag.angebot.teilnehmer.push(Teilnehmer {
                kennung: kennung.to_string(),
                name,
            });
            eintrag
                .angebot
                .teilnehmer
                .sort_by(|a, b| a.kennung.cmp(&b.kennung));
        }
        let mut aktionen = vec![senden(kennung, ServerRahmen::Beigetreten { angebot })];
        if let Some(raum) = raum {
            aktionen.extend(self.raum_liste(&raum));
        }
        aktionen
    }

    fn verlassen(&mut self, kennung: &str, angebot: AngebotId) -> Vec<Aktion> {
        let Some(eintrag) = self.angebote.get(&angebot) else {
            return vec![senden(kennung, fehler(Fehler::UnbekanntesAngebot))];
        };
        if eintrag.angebot.gastgeber == kennung {
            // Der Gastgeber nimmt seine eigene Freigabe zurück, er verlässt sie
            // nicht („Verlassen“ setzt eine Teilnahme voraus).
            return vec![senden(kennung, fehler(Fehler::EigenesAngebot))];
        }
        if !eintrag.ist_teilnehmer(kennung) {
            return vec![senden(kennung, fehler(Fehler::UnbekanntesAngebot))];
        }
        let raum = self.raum_von_angebot(angebot);
        self.teilnehmer_austragen(angebot, kennung);
        let mut aktionen = vec![
            senden(
                kennung,
                ServerRahmen::Beendet {
                    angebot,
                    grund: Grund::Verlassen,
                },
            ),
            Aktion::Trennen {
                angebot,
                teilnehmer: Some(kennung.to_string()),
            },
        ];
        if let Some(raum) = raum {
            aktionen.extend(self.raum_liste(&raum));
        }
        aktionen
    }

    fn entfernen(&mut self, kennung: &str, angebot: AngebotId, ziel: &str) -> Vec<Aktion> {
        let Some(eintrag) = self.angebote.get(&angebot) else {
            return vec![senden(kennung, fehler(Fehler::UnbekanntesAngebot))];
        };
        if eintrag.angebot.gastgeber != kennung {
            return vec![senden(kennung, fehler(Fehler::NichtGastgeber))];
        }
        if !eintrag.ist_teilnehmer(ziel) {
            return vec![senden(kennung, fehler(Fehler::UngueltigeAnfrage))];
        }
        let raum = self.raum_von_angebot(angebot);
        self.teilnehmer_austragen(angebot, ziel);
        let mut aktionen = vec![
            senden(
                ziel,
                ServerRahmen::Beendet {
                    angebot,
                    grund: Grund::Entfernt,
                },
            ),
            Aktion::Trennen {
                angebot,
                teilnehmer: Some(ziel.to_string()),
            },
        ];
        if let Some(raum) = raum {
            aktionen.extend(self.raum_liste(&raum));
        }
        aktionen
    }

    fn getrennt(&mut self, kennung: &str) -> Vec<Aktion> {
        let Some(mitglied) = self.mitglieder.remove(kennung) else {
            // Nie verbunden (oder schon abgemeldet): nichts zu tun.
            return Vec::new();
        };
        let mut aktionen = Vec::new();

        // Eigene Freigaben enden — der Gastgeber ist weg.
        for angebot in self.eigene_angebote(kennung) {
            for teilnehmer in self.teilnehmer_von(angebot) {
                aktionen.push(senden(
                    &teilnehmer,
                    ServerRahmen::Beendet {
                        angebot,
                        grund: Grund::GastgeberWeg,
                    },
                ));
            }
            aktionen.push(Aktion::Trennen {
                angebot,
                teilnehmer: None,
            });
            self.angebote.remove(&angebot);
            self.drossel.remove(&angebot);
        }

        // Eigene Teilnahmen enden still: er ist nicht mehr da, um es zu lesen.
        for angebot in self.teilnahmen(kennung) {
            self.teilnehmer_austragen(angebot, kennung);
            aktionen.push(Aktion::Trennen {
                angebot,
                teilnehmer: Some(kennung.to_string()),
            });
        }

        if let Some(raum) = mitglied.raum {
            aktionen.extend(self.raum_liste(&raum));
        }
        aktionen
    }

    fn raumwechsel(&mut self, kennung: &str, raum: Option<String>) -> Vec<Aktion> {
        let Some(alt) = self.mitglieder.get(kennung).map(|m| m.raum.clone()) else {
            // Kein Mitglied (oder keine Spielnetz-Verbindung): ignorieren.
            return Vec::new();
        };
        if alt == raum {
            return Vec::new();
        }
        let mut aktionen = Vec::new();

        // Eigene Freigaben enden wie beim Zurückziehen, nur mit anderem Grund.
        for angebot in self.eigene_angebote(kennung) {
            for teilnehmer in self.teilnehmer_von(angebot) {
                aktionen.push(senden(
                    &teilnehmer,
                    ServerRahmen::Beendet {
                        angebot,
                        grund: Grund::RaumVerlassen,
                    },
                ));
            }
            aktionen.push(Aktion::Trennen {
                angebot,
                teilnehmer: None,
            });
            self.angebote.remove(&angebot);
            self.drossel.remove(&angebot);
        }

        // Eigene Teilnahmen enden mit Nachricht an ihn.
        for angebot in self.teilnahmen(kennung) {
            self.teilnehmer_austragen(angebot, kennung);
            aktionen.push(senden(
                kennung,
                ServerRahmen::Beendet {
                    angebot,
                    grund: Grund::RaumVerlassen,
                },
            ));
            aktionen.push(Aktion::Trennen {
                angebot,
                teilnehmer: Some(kennung.to_string()),
            });
        }

        if let Some(m) = self.mitglieder.get_mut(kennung) {
            m.raum = raum.clone();
        }

        // Alten Raum informieren, dann den neuen — der Umziehende ist schon
        // umgestellt und bekommt deshalb die Liste des neuen Raums.
        if let Some(alt) = alt {
            aktionen.extend(self.raum_liste(&alt));
        }
        if let Some(neu) = &raum {
            aktionen.extend(self.raum_liste(neu));
        }
        aktionen
    }

    // ---------------------------------------------------------------- Datenebene

    /// Ein Teilnehmer öffnet einen TCP-Strom. `Ok`: Gastgeber und die vom
    /// Server vergebene Stromnummer.
    pub fn strom_oeffnen(
        &mut self,
        von: &str,
        angebot: AngebotId,
    ) -> Result<(String, u32), Fehler> {
        let grenze = usize::from(self.grenzen.stroeme_je_teilnehmer);
        let Some(eintrag) = self.angebote.get_mut(&angebot) else {
            return Err(Fehler::UnbekanntesAngebot);
        };
        if !eintrag.ist_teilnehmer(von) {
            return Err(Fehler::UnbekanntesAngebot);
        }
        let offen = eintrag.offene_stroeme.get(von).copied().unwrap_or(0);
        if usize::from(offen) >= grenze {
            return Err(Fehler::ZuVieleStroeme);
        }
        let nummer = eintrag
            .naechste_stromnummer
            .entry(von.to_string())
            .or_insert(1);
        let strom = *nummer;
        *nummer = nummer.saturating_add(1);
        let zaehler = eintrag.offene_stroeme.entry(von.to_string()).or_insert(0);
        *zaehler = zaehler.saturating_add(1);
        Ok((eintrag.angebot.gastgeber.clone(), strom))
    }

    /// Ein TCP-Strom ist zu Ende (egal wie). Gibt seinen Platz wieder frei.
    pub fn strom_geschlossen(&mut self, von: &str, angebot: AngebotId) {
        if let Some(eintrag) = self.angebote.get_mut(&angebot) {
            if let Some(zaehler) = eintrag.offene_stroeme.get_mut(von) {
                *zaehler = zaehler.saturating_sub(1);
            }
        }
    }

    /// Ein Datagramm vom Teilnehmer. `Ok`: Gastgeber, Kanal beim Gastgeber und —
    /// beim ersten Mal — die Aktion, die dem Gastgeber den Kanal nennt.
    pub fn datagramm_hin(
        &mut self,
        von: &str,
        angebot: AngebotId,
        kanal: Kanal,
    ) -> Option<(String, Kanal, Option<Aktion>)> {
        let grenze = self.grenzen.stroeme_je_teilnehmer;
        let eintrag = self.angebote.get_mut(&angebot)?;
        if !eintrag.ist_teilnehmer(von) {
            return None;
        }
        let gastgeber = eintrag.angebot.gastgeber.clone();
        let schluessel = (von.to_string(), kanal);
        if let Some(vorhanden) = eintrag.kanal_hin.get(&schluessel).copied() {
            return Some((gastgeber, vorhanden, None));
        }
        // Dieselbe Grenze wie für TCP-Ströme: ein Teilnehmer kann den
        // Gastgeber nicht mit Kanälen fluten.
        let eigene = eintrag.kanal_hin.keys().filter(|(t, _)| t == von).count();
        if eigene >= usize::from(grenze) {
            return None;
        }
        // Kanalnummern sind `u16`; bei 65535 wäre eine Umnummerierung fällig —
        // die gibt es nicht, also lieber verwerfen als doppelt vergeben.
        if eintrag.naechster_kanal == Kanal::MAX {
            return None;
        }
        let neu = eintrag.naechster_kanal;
        eintrag.naechster_kanal += 1;
        eintrag.kanal_hin.insert(schluessel, neu);
        eintrag.kanal_her.insert(neu, (von.to_string(), kanal));
        let aktion = senden(
            &gastgeber,
            ServerRahmen::Kanal {
                angebot,
                kanal: neu,
                von: von.to_string(),
            },
        );
        Some((gastgeber, neu, Some(aktion)))
    }

    /// Ein Datagramm vom Gastgeber. `Ok`: Teilnehmer und dessen Kanal.
    pub fn datagramm_her(
        &self,
        gastgeber: &str,
        angebot: AngebotId,
        kanal: Kanal,
    ) -> Option<(String, Kanal)> {
        let eintrag = self.angebote.get(&angebot)?;
        if eintrag.angebot.gastgeber != gastgeber {
            return None;
        }
        eintrag
            .kanal_her
            .get(&kanal)
            .map(|(teilnehmer, sein_kanal)| (teilnehmer.clone(), *sein_kanal))
    }

    /// Durchsatzgrenze einer Freigabe (Token-Eimer, `kbit_je_freigabe`;
    /// `0` = unbegrenzt). `false` heißt: dieses Datagramm verwerfen.
    pub fn durchsatz(&mut self, angebot: AngebotId, bytes: usize, jetzt: Instant) -> bool {
        if !self.angebote.contains_key(&angebot) {
            // Unbekannte Freigabe: hier gibt es nichts zu drosseln. Ob der
            // Verkehr überhaupt hierher gehört, prüft die Wegewahl davor.
            return true;
        }
        let kbit = self.grenzen.kbit_je_freigabe;
        if kbit == 0 {
            return true;
        }
        // kbit/s → Byte/s (1000 Bit je kbit, 8 Bit je Byte).
        let rate = f64::from(kbit) * 125.0;
        // Ein Eimer, der kleiner als ein Datagramm ist, ließe gar nichts durch —
        // deshalb mindestens Platz für eines.
        let kapazitaet = rate.max(1500.0);

        let eimer = self.drossel.entry(angebot).or_insert(Eimer {
            stand: kapazitaet,
            letzte: jetzt,
        });
        let vergangen = jetzt.saturating_duration_since(eimer.letzte).as_secs_f64();
        eimer.stand = (eimer.stand + vergangen * rate).min(kapazitaet);
        eimer.letzte = jetzt;

        let platz = eimer.stand >= bytes as f64;
        if platz {
            eimer.stand -= bytes as f64;
        }
        platz
    }

    /// Ist `kennung` (noch) Teilnehmer dieser Freigabe? Die Datenebene fragt
    /// das nach jedem `await`, damit ein Rauswurf dazwischen nicht verloren geht.
    pub fn ist_teilnehmer(&self, angebot: AngebotId, kennung: &str) -> bool {
        self.angebote
            .get(&angebot)
            .is_some_and(|e| e.ist_teilnehmer(kennung))
    }

    /// Alle Angebote eines Raums, sortiert nach Nummer (für Tests und Anzeige).
    pub fn angebote_im_raum(&self, raum: &str) -> Vec<Angebot> {
        self.liste(Some(raum))
    }

    // ------------------------------------------------------------------ Helfer

    /// Alle Angebote eines Raums. Ein Angebot gehört zu dem Raum, in dem sein
    /// Gastgeber **jetzt** ist.
    fn liste(&self, raum: Option<&str>) -> Vec<Angebot> {
        let Some(raum) = raum else {
            return Vec::new();
        };
        self.angebote
            .values()
            .filter(|e| {
                self.mitglieder
                    .get(&e.angebot.gastgeber)
                    .is_some_and(|m| m.in_raum(raum))
            })
            .map(|e| e.angebot.clone())
            .collect()
    }

    /// Die vollständige Angebotsliste an **alle** in diesem Raum (sortiert).
    fn raum_liste(&self, raum: &str) -> Vec<Aktion> {
        let liste = self.liste(Some(raum));
        self.mitglieder
            .iter()
            .filter(|(_, m)| m.in_raum(raum))
            .map(|(kennung, _)| {
                senden(
                    kennung,
                    ServerRahmen::Angebote {
                        liste: liste.clone(),
                    },
                )
            })
            .collect()
    }

    fn raum_von_angebot(&self, angebot: AngebotId) -> Option<String> {
        let eintrag = self.angebote.get(&angebot)?;
        self.mitglieder
            .get(&eintrag.angebot.gastgeber)
            .and_then(|m| m.raum.clone())
    }

    fn teilnehmer_von(&self, angebot: AngebotId) -> Vec<String> {
        self.angebote
            .get(&angebot)
            .map(|e| {
                e.angebot
                    .teilnehmer
                    .iter()
                    .map(|t| t.kennung.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn eigene_angebote(&self, kennung: &str) -> Vec<AngebotId> {
        self.angebote
            .iter()
            .filter(|(_, e)| e.angebot.gastgeber == kennung)
            .map(|(id, _)| *id)
            .collect()
    }

    fn teilnahmen(&self, kennung: &str) -> Vec<AngebotId> {
        self.angebote
            .iter()
            .filter(|(_, e)| e.ist_teilnehmer(kennung))
            .map(|(id, _)| *id)
            .collect()
    }

    fn teilnehmer_austragen(&mut self, angebot: AngebotId, kennung: &str) {
        if let Some(eintrag) = self.angebote.get_mut(&angebot) {
            eintrag.teilnehmer_austragen(kennung);
        }
    }
}

/// Eine Sende-Aktion.
fn senden(an: &str, rahmen: ServerRahmen) -> Aktion {
    Aktion::Senden {
        an: an.to_string(),
        rahmen,
    }
}

/// Ein Fehlerrahmen mit dem stabilen Text seines Codes.
fn fehler(code: Fehler) -> ServerRahmen {
    ServerRahmen::Fehler {
        code,
        text: code.text().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const A: &str = "aa";
    const B: &str = "bb";
    const C: &str = "cc";
    const RAUM: &str = "verein";

    fn frisch() -> Vermittlung {
        Vermittlung::neu(Grenzen::default())
    }

    fn verbinden(
        v: &mut Vermittlung,
        kennung: &str,
        raum: Option<&str>,
        darf: bool,
    ) -> Vec<Aktion> {
        v.verarbeite(Ereignis::Verbunden {
            kennung: kennung.to_string(),
            name: kennung.to_uppercase(),
            raum: raum.map(str::to_string),
            darf,
        })
    }

    /// Verbindet und wirft die Begrüßung weg (die meisten Tests wollen sie nicht sehen).
    fn da(v: &mut Vermittlung, kennung: &str, raum: &str) {
        let _ = verbinden(v, kennung, Some(raum), true);
    }

    fn rahmen(v: &mut Vermittlung, kennung: &str, rahmen: ClientRahmen) -> Vec<Aktion> {
        v.verarbeite(Ereignis::Rahmen {
            kennung: kennung.to_string(),
            rahmen,
        })
    }

    fn anbieten(v: &mut Vermittlung, kennung: &str, name: &str) -> Vec<Aktion> {
        rahmen(
            v,
            kennung,
            ClientRahmen::Anbieten {
                name: name.to_string(),
                port: 25565,
                art: Art::Tcp,
            },
        )
    }

    fn gesendet(aktionen: &[Aktion], an: &str) -> Vec<ServerRahmen> {
        aktionen
            .iter()
            .filter_map(|a| match a {
                Aktion::Senden { an: ziel, rahmen } if ziel == an => Some(rahmen.clone()),
                _ => None,
            })
            .collect()
    }

    fn fehlercode(aktionen: &[Aktion], an: &str) -> Option<Fehler> {
        gesendet(aktionen, an).iter().find_map(|r| match r {
            ServerRahmen::Fehler { code, .. } => Some(*code),
            _ => None,
        })
    }

    fn beendet(aktionen: &[Aktion], an: &str) -> Option<(AngebotId, Grund)> {
        gesendet(aktionen, an).iter().find_map(|r| match r {
            ServerRahmen::Beendet { angebot, grund } => Some((*angebot, *grund)),
            _ => None,
        })
    }

    fn angeboten(aktionen: &[Aktion], an: &str) -> Option<AngebotId> {
        gesendet(aktionen, an).iter().find_map(|r| match r {
            ServerRahmen::Angeboten { angebot } => Some(*angebot),
            _ => None,
        })
    }

    fn angebote(aktionen: &[Aktion], an: &str) -> Option<Vec<Angebot>> {
        gesendet(aktionen, an).iter().find_map(|r| match r {
            ServerRahmen::Angebote { liste } => Some(liste.clone()),
            _ => None,
        })
    }

    fn trennungen(aktionen: &[Aktion]) -> Vec<(AngebotId, Option<String>)> {
        aktionen
            .iter()
            .filter_map(|a| match a {
                Aktion::Trennen {
                    angebot,
                    teilnehmer,
                } => Some((*angebot, teilnehmer.clone())),
                _ => None,
            })
            .collect()
    }

    /// Die Kennungen der Teilnehmer einer Freigabe (der Name steht daneben).
    fn kennungen(angebot: &Angebot) -> Vec<String> {
        angebot
            .teilnehmer
            .iter()
            .map(|t| t.kennung.clone())
            .collect()
    }

    // -------------------------------------------------------------- Anmeldung

    #[test]
    fn verbunden_schickt_willkommen_und_liste() {
        let mut v = frisch();
        let aktionen = verbinden(&mut v, A, Some(RAUM), true);
        assert_eq!(aktionen.len(), 2);
        assert_eq!(
            aktionen[0],
            senden(
                A,
                ServerRahmen::Willkommen {
                    grenzen: Grenzen::default()
                }
            )
        );
        assert_eq!(angebote(&aktionen, A), Some(Vec::new()));
    }

    #[test]
    fn verbunden_ohne_raum_bekommt_leere_liste() {
        let mut v = frisch();
        let aktionen = verbinden(&mut v, A, None, true);
        assert_eq!(angebote(&aktionen, A), Some(Vec::new()));
    }

    #[test]
    fn unbekannter_rahmen_ist_nicht_angemeldet() {
        let mut v = frisch();
        let aktionen = anbieten(&mut v, A, "Minecraft");
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::NichtAngemeldet));
    }

    #[test]
    fn zweites_hallo_ist_eine_ungueltige_anfrage() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        let aktionen = rahmen(
            &mut v,
            A,
            ClientRahmen::Hallo {
                version: 1,
                kennung: A.to_string(),
                beweis: "00".to_string(),
            },
        );
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::UngueltigeAnfrage));
    }

    // --------------------------------------------------------------- Anbieten

    #[test]
    fn anbieten_ohne_recht_wird_abgelehnt() {
        let mut v = frisch();
        let _ = verbinden(&mut v, A, Some(RAUM), false);
        let aktionen = anbieten(&mut v, A, "Minecraft");
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::KeinRecht));
        assert!(v.angebote_im_raum(RAUM).is_empty());
    }

    #[test]
    fn anbieten_ohne_raum_wird_abgelehnt() {
        let mut v = frisch();
        let _ = verbinden(&mut v, A, None, true);
        let aktionen = anbieten(&mut v, A, "Minecraft");
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::KeinRaum));
    }

    #[test]
    fn anbieten_prueft_port_und_name() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        let faelle = [
            ("Minecraft".to_string(), 0, false),
            (String::new(), 25565, false),
            ("x".repeat(65), 25565, false),
            ("x".repeat(64), 25565, true),
            ("Minecraft".to_string(), 25565, true),
        ];
        for (name, port, erwartet_ok) in faelle {
            let aktionen = rahmen(
                &mut v,
                A,
                ClientRahmen::Anbieten {
                    name: name.clone(),
                    port,
                    art: Art::Tcp,
                },
            );
            let code = fehlercode(&aktionen, A);
            assert_eq!(
                code,
                if erwartet_ok {
                    None
                } else {
                    Some(Fehler::UngueltigeAnfrage)
                },
                "Name {name:?}, Port {port}"
            );
        }
    }

    #[test]
    fn anbieten_bekommt_eine_fortlaufende_nummer() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        let erste = anbieten(&mut v, A, "Minecraft");
        assert_eq!(angeboten(&erste, A), Some(1));
        let zweite = anbieten(&mut v, A, "Sprachserver");
        assert_eq!(angeboten(&zweite, A), Some(2));
        // Die Nummern stehen auch in der Liste, die der Raum bekommt.
        assert_eq!(
            angebote(&zweite, A).map(|l| l.iter().map(|a| a.id).collect::<Vec<_>>()),
            Some(vec![1, 2])
        );
    }

    #[test]
    fn zu_viele_freigaben_werden_abgelehnt() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        assert!(fehlercode(&anbieten(&mut v, A, "Minecraft"), A).is_none());
        assert!(fehlercode(&anbieten(&mut v, A, "Sprachserver"), A).is_none());
        let dritte = anbieten(&mut v, A, "Drittes");
        assert_eq!(fehlercode(&dritte, A), Some(Fehler::ZuVieleFreigaben));
        assert_eq!(v.angebote_im_raum(RAUM).len(), 2);
    }

    #[test]
    fn angebot_geht_an_alle_im_raum_aber_nicht_in_anderen() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        da(&mut v, B, RAUM);
        da(&mut v, C, "anderer");
        let aktionen = anbieten(&mut v, A, "Minecraft");

        // A bekommt Nummer und Liste, B die Liste, C nichts.
        assert_eq!(angebote(&aktionen, A).map(|l| l.len()), Some(1));
        let liste_b = angebote(&aktionen, B).expect("B bekommt die Liste");
        assert_eq!(liste_b.len(), 1);
        assert_eq!(liste_b[0].gastgeber, A);
        assert_eq!(liste_b[0].gastgeber_name, A.to_uppercase());
        assert_eq!(liste_b[0].port, 25565);
        assert_eq!(liste_b[0].art, Art::Tcp);
        assert!(angebote(&aktionen, C).is_none());
        // Reihenfolge deterministisch: A vor B (sortiert).
        assert_eq!(
            aktionen
                .iter()
                .filter_map(|a| match a {
                    Aktion::Senden { an, .. } => Some(an.clone()),
                    Aktion::Trennen { .. } => None,
                })
                .collect::<Vec<_>>(),
            vec![A.to_string(), A.to_string(), B.to_string()]
        );
    }

    // ---------------------------------------------------------------- Beitreten

    #[test]
    fn beitreten_zu_unbekanntem_angebot() {
        let mut v = frisch();
        da(&mut v, B, RAUM);
        let aktionen = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 9 });
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::UnbekanntesAngebot));
    }

    #[test]
    fn beitreten_aus_anderem_raum() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, "anderer");
        let aktionen = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::AndererRaum));
    }

    #[test]
    fn beitreten_zur_eigenen_freigabe() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let aktionen = rahmen(&mut v, A, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::EigenesAngebot));
    }

    #[test]
    fn beitreten_ohne_recht() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let _ = verbinden(&mut v, B, Some(RAUM), false);
        let aktionen = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::KeinRecht));
    }

    #[test]
    fn beitreten_traegt_den_namen_des_teilnehmers_ein() {
        // P4: die Oberfläche braucht den Namen zum Anzeigen, die Kennung zum
        // Entfernen — beides steht im Angebot.
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        let _ = rahmen(&mut v, C, ClientRahmen::Beitreten { angebot: 1 });
        let _ = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        let angebot = &v.angebote_im_raum(RAUM)[0];
        assert_eq!(
            angebot.teilnehmer,
            vec![
                Teilnehmer {
                    kennung: B.to_string(),
                    name: B.to_uppercase(),
                },
                Teilnehmer {
                    kennung: C.to_string(),
                    name: C.to_uppercase(),
                },
            ],
            "nach Kennung sortiert, jeder mit seinem Namen"
        );
    }

    #[test]
    fn beitreten_ueber_der_grenze() {
        let mut v = Vermittlung::neu(Grenzen {
            teilnehmer_je_freigabe: 1,
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        assert!(fehlercode(
            &rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 }),
            B
        )
        .is_none());
        let aktionen = rahmen(&mut v, C, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, C), Some(Fehler::ZuVieleTeilnehmer));
    }

    #[test]
    fn beitreten_ist_doppelt_idempotent() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        let erste = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(
            gesendet(&erste, B),
            vec![
                ServerRahmen::Beigetreten { angebot: 1 },
                ServerRahmen::Angebote {
                    liste: v.angebote_im_raum(RAUM)
                },
            ]
        );
        assert_eq!(
            angebote(&erste, A).map(|l| kennungen(&l[0])),
            Some(vec![B.to_string()])
        );

        let zweite = rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(
            gesendet(&zweite, B),
            vec![ServerRahmen::Beigetreten { angebot: 1 }],
            "beim zweiten Mal nur die Bestätigung, keine neue Liste"
        );
        assert!(trennungen(&zweite).is_empty());
        assert_eq!(kennungen(&v.angebote_im_raum(RAUM)[0]), vec![B.to_string()]);
    }

    // ------------------------------------------------------- Verlassen/Entfernen

    #[test]
    fn verlassen_beendet_die_teilnahme() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        let aktionen = rahmen(&mut v, B, ClientRahmen::Verlassen { angebot: 1 });
        assert_eq!(beendet(&aktionen, B), Some((1, Grund::Verlassen)));
        assert_eq!(trennungen(&aktionen), vec![(1, Some(B.to_string()))]);
        assert_eq!(
            angebote(&aktionen, A).map(|l| kennungen(&l[0])),
            Some(Vec::new())
        );
        assert!(v.angebote_im_raum(RAUM)[0].teilnehmer.is_empty());
    }

    #[test]
    fn verlassen_ohne_teilnahme_ist_unbekannt() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        let aktionen = rahmen(&mut v, B, ClientRahmen::Verlassen { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::UnbekanntesAngebot));
    }

    #[test]
    fn gastgeber_verlaesst_seine_eigene_freigabe_nicht() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let aktionen = rahmen(&mut v, A, ClientRahmen::Verlassen { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::EigenesAngebot));
    }

    #[test]
    fn entfernen_braucht_den_gastgeber() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        let aktionen = rahmen(
            &mut v,
            B,
            ClientRahmen::Entfernen {
                angebot: 1,
                kennung: C.to_string(),
            },
        );
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::NichtGastgeber));
    }

    #[test]
    fn entfernen_beendet_die_teilnahme() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        let aktionen = rahmen(
            &mut v,
            A,
            ClientRahmen::Entfernen {
                angebot: 1,
                kennung: B.to_string(),
            },
        );
        assert_eq!(beendet(&aktionen, B), Some((1, Grund::Entfernt)));
        assert_eq!(trennungen(&aktionen), vec![(1, Some(B.to_string()))]);
        assert_eq!(
            angebote(&aktionen, A).map(|l| kennungen(&l[0])),
            Some(Vec::new())
        );
    }

    #[test]
    fn entfernen_ohne_teilnahme_ist_ungueltig() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let aktionen = rahmen(
            &mut v,
            A,
            ClientRahmen::Entfernen {
                angebot: 1,
                kennung: B.to_string(),
            },
        );
        assert_eq!(fehlercode(&aktionen, A), Some(Fehler::UngueltigeAnfrage));
    }

    #[test]
    fn zurueckziehen_braucht_den_gastgeber() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        let aktionen = rahmen(&mut v, B, ClientRahmen::Zurueckziehen { angebot: 1 });
        assert_eq!(fehlercode(&aktionen, B), Some(Fehler::NichtGastgeber));
    }

    #[test]
    fn zurueckziehen_beendet_alles() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        rahmen(&mut v, C, ClientRahmen::Beitreten { angebot: 1 });
        // Kanäle vergessen: nach dem Zurückziehen gibt es keine Zuordnung mehr.
        assert!(v.datagramm_hin(B, 1, 7).is_some());

        let aktionen = rahmen(&mut v, A, ClientRahmen::Zurueckziehen { angebot: 1 });
        for teilnehmer in [B, C] {
            assert_eq!(
                beendet(&aktionen, teilnehmer),
                Some((1, Grund::Zurueckgezogen)),
                "{teilnehmer} muss die Beendigung bekommen"
            );
            // Danach die leere Liste: die Freigabe ist weg.
            assert_eq!(angebote(&aktionen, teilnehmer), Some(Vec::new()));
        }
        assert_eq!(trennungen(&aktionen), vec![(1, None)]);
        assert_eq!(angebote(&aktionen, A).map(|l| l.len()), Some(0));
        assert!(v.angebote_im_raum(RAUM).is_empty());
        assert!(v.datagramm_hin(B, 1, 7).is_none());
        assert!(v.strom_oeffnen(B, 1).is_err());
    }

    // ------------------------------------------------------- Raumwechsel/Trennen

    #[test]
    fn raumwechsel_beendet_eigene_freigaben() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, "anderer");
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        let aktionen = v.verarbeite(Ereignis::Raumwechsel {
            kennung: A.to_string(),
            raum: Some("neu".to_string()),
        });
        assert_eq!(beendet(&aktionen, B), Some((1, Grund::RaumVerlassen)));
        assert_eq!(angebote(&aktionen, B), Some(Vec::new()));
        assert_eq!(trennungen(&aktionen), vec![(1, None)]);
        // A bekommt die (leere) Liste des neuen Raums.
        assert_eq!(angebote(&aktionen, A), Some(Vec::new()));
        // C ist in einem anderen Raum und wird nicht behelligt.
        assert!(gesendet(&aktionen, C).is_empty());
        assert!(v.angebote_im_raum(RAUM).is_empty());
        assert!(v.angebote_im_raum("neu").is_empty());
    }

    #[test]
    fn raumwechsel_als_teilnehmer_beendet_mit_nachricht() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        let aktionen = v.verarbeite(Ereignis::Raumwechsel {
            kennung: B.to_string(),
            raum: None,
        });
        assert_eq!(beendet(&aktionen, B), Some((1, Grund::RaumVerlassen)));
        assert_eq!(trennungen(&aktionen), vec![(1, Some(B.to_string()))]);
        assert_eq!(
            angebote(&aktionen, A).map(|l| kennungen(&l[0])),
            Some(Vec::new())
        );
    }

    #[test]
    fn raumwechsel_ohne_aenderung_tut_nichts() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        let aktionen = v.verarbeite(Ereignis::Raumwechsel {
            kennung: A.to_string(),
            raum: Some(RAUM.to_string()),
        });
        assert!(aktionen.is_empty());
    }

    #[test]
    fn raumwechsel_unbekannter_kennung_wird_ignoriert() {
        let mut v = frisch();
        let aktionen = v.verarbeite(Ereignis::Raumwechsel {
            kennung: "unbekannt".to_string(),
            raum: Some(RAUM.to_string()),
        });
        assert!(aktionen.is_empty());
    }

    #[test]
    fn trennen_des_gastgebers_beendet_seine_freigaben() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        rahmen(&mut v, C, ClientRahmen::Beitreten { angebot: 1 });

        let aktionen = v.verarbeite(Ereignis::Getrennt {
            kennung: A.to_string(),
        });
        for teilnehmer in [B, C] {
            assert_eq!(
                beendet(&aktionen, teilnehmer),
                Some((1, Grund::GastgeberWeg))
            );
        }
        assert_eq!(trennungen(&aktionen), vec![(1, None)]);
        assert!(v.angebote_im_raum(RAUM).is_empty());
        // Die Verbliebenen bekommen die neue (leere) Liste.
        assert_eq!(angebote(&aktionen, B).map(|l| l.len()), Some(0));
        assert!(angebote(&aktionen, C).is_some());
    }

    #[test]
    fn trennen_als_teilnehmer_raeumt_still_auf() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert!(v.datagramm_hin(B, 1, 3).is_some());

        let aktionen = v.verarbeite(Ereignis::Getrennt {
            kennung: B.to_string(),
        });
        // Keine Nachricht an B — er ist weg —, aber die Liste geht an den Raum.
        assert!(gesendet(&aktionen, B).is_empty());
        assert_eq!(trennungen(&aktionen), vec![(1, Some(B.to_string()))]);
        assert_eq!(
            angebote(&aktionen, A).map(|l| kennungen(&l[0])),
            Some(Vec::new())
        );
        assert!(v.datagramm_hin(B, 1, 3).is_none());
    }

    #[test]
    fn trennen_einer_unbekannten_kennung_tut_nichts() {
        let mut v = frisch();
        assert!(v
            .verarbeite(Ereignis::Getrennt {
                kennung: "weg".to_string()
            })
            .is_empty());
    }

    // -------------------------------------------------------------- TCP-Ströme

    #[test]
    fn strom_oeffnen_vergibt_nummern_ab_eins() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        assert_eq!(v.strom_oeffnen(B, 1), Ok((A.to_string(), 1)));
        assert_eq!(v.strom_oeffnen(B, 1), Ok((A.to_string(), 2)));
        v.strom_geschlossen(B, 1);
        // Die Nummer wird nicht wiederverwendet, der Platz schon.
        assert_eq!(v.strom_oeffnen(B, 1), Ok((A.to_string(), 3)));
    }

    #[test]
    fn strom_oeffnen_prueft_teilnahme_und_grenze() {
        let mut v = Vermittlung::neu(Grenzen {
            stroeme_je_teilnehmer: 2,
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        // Noch nicht beigetreten.
        assert_eq!(v.strom_oeffnen(B, 1), Err(Fehler::UnbekanntesAngebot));
        // Unbekanntes Angebot.
        assert_eq!(v.strom_oeffnen(B, 7), Err(Fehler::UnbekanntesAngebot));

        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert!(v.strom_oeffnen(B, 1).is_ok());
        assert!(v.strom_oeffnen(B, 1).is_ok());
        assert_eq!(v.strom_oeffnen(B, 1), Err(Fehler::ZuVieleStroeme));
        v.strom_geschlossen(B, 1);
        assert!(v.strom_oeffnen(B, 1).is_ok());
    }

    #[test]
    fn strom_geschlossen_ohne_offenen_strom_ist_harmlos() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        // Kein Panik, kein Unterlauf.
        v.strom_geschlossen(B, 1);
        v.strom_geschlossen(B, 99);
    }

    // ------------------------------------------------------------ UDP-Kanäle

    #[test]
    fn datagramme_werden_beidseitig_zugeordnet() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        da(&mut v, C, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        rahmen(&mut v, C, ClientRahmen::Beitreten { angebot: 1 });

        // Erstes Datagramm von B: Kanal 1 beim Gastgeber, B erfährt es einmal.
        let (an, kanal, aktion) = v.datagramm_hin(B, 1, 9).expect("Zuordnung");
        assert_eq!((an.as_str(), kanal), (A, 1));
        assert_eq!(
            aktion,
            Some(senden(
                A,
                ServerRahmen::Kanal {
                    angebot: 1,
                    kanal: 1,
                    von: B.to_string()
                }
            ))
        );
        // Danach ohne erneute Meldung.
        assert_eq!(v.datagramm_hin(B, 1, 9), Some((A.to_string(), 1, None)));
        // C bekommt den nächsten Kanal.
        assert_eq!(v.datagramm_hin(C, 1, 9).map(|(_, k, _)| k), Some(2));

        // Antworten gehen an den richtigen Teilnehmer samt dessen Kanal.
        assert_eq!(v.datagramm_her(A, 1, 1), Some((B.to_string(), 9)));
        assert_eq!(v.datagramm_her(A, 1, 2), Some((C.to_string(), 9)));
        // Falscher Gastgeber oder unbekannter Kanal: nichts.
        assert_eq!(v.datagramm_her(C, 1, 1), None);
        assert_eq!(v.datagramm_her(A, 1, 42), None);
        assert_eq!(v.datagramm_her(A, 9, 5), None);
    }

    #[test]
    fn datagramme_nur_fuer_teilnehmer() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        // B ist (noch) nicht dabei.
        assert!(v.datagramm_hin(B, 1, 1).is_none());
        // Der Gastgeber schickt nicht über seinen eigenen Angebotskanal.
        assert!(v.datagramm_hin(A, 1, 1).is_none());
        // Unbekanntes Angebot.
        assert!(v.datagramm_hin(B, 4, 1).is_none());
    }

    #[test]
    fn kanal_verschwindet_beim_verlassen() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        let (_, kanal, _) = v.datagramm_hin(B, 1, 4).expect("Zuordnung");
        assert!(v.datagramm_her(A, 1, kanal).is_some());

        rahmen(&mut v, B, ClientRahmen::Verlassen { angebot: 1 });
        assert!(v.datagramm_her(A, 1, kanal).is_none());
        assert!(v.datagramm_hin(B, 1, 4).is_none());
    }

    #[test]
    fn kanal_kommt_beim_wiedereintritt_neu() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(v.datagramm_hin(B, 1, 1).map(|(_, k, _)| k), Some(1));
        rahmen(&mut v, B, ClientRahmen::Verlassen { angebot: 1 });
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert_eq!(v.datagramm_hin(B, 1, 1).map(|(_, k, _)| k), Some(2));
    }

    #[test]
    fn mehrere_lokale_kanaele_bekommen_eigene_gastgeberkanaele() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Valheim");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });

        let (_, erster, meldung1) = v.datagramm_hin(B, 1, 3).expect("Zuordnung");
        let (_, zweiter, meldung2) = v.datagramm_hin(B, 1, 4).expect("Zuordnung");
        assert_ne!(erster, zweiter);
        // Je lokalem Kanal ein eigener Rahmen an den Gastgeber.
        assert_eq!(
            meldung1,
            Some(senden(
                A,
                ServerRahmen::Kanal {
                    angebot: 1,
                    kanal: erster,
                    von: B.to_string()
                }
            ))
        );
        assert_eq!(
            meldung2,
            Some(senden(
                A,
                ServerRahmen::Kanal {
                    angebot: 1,
                    kanal: zweiter,
                    von: B.to_string()
                }
            ))
        );
        // Bekannte Paare melden sich nicht erneut.
        assert_eq!(
            v.datagramm_hin(B, 1, 3),
            Some((A.to_string(), erster, None))
        );
        // Antworten gehen an den jeweils richtigen lokalen Kanal.
        assert_eq!(v.datagramm_her(A, 1, erster), Some((B.to_string(), 3)));
        assert_eq!(v.datagramm_her(A, 1, zweiter), Some((B.to_string(), 4)));

        rahmen(&mut v, B, ClientRahmen::Verlassen { angebot: 1 });
        assert_eq!(v.datagramm_her(A, 1, erster), None);
        assert_eq!(v.datagramm_her(A, 1, zweiter), None);
    }

    #[test]
    fn kanaele_je_teilnehmer_sind_begrenzt() {
        let mut v = Vermittlung::neu(Grenzen {
            stroeme_je_teilnehmer: 2,
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Valheim");
        da(&mut v, B, RAUM);
        rahmen(&mut v, B, ClientRahmen::Beitreten { angebot: 1 });
        assert!(v.datagramm_hin(B, 1, 1).is_some());
        assert!(v.datagramm_hin(B, 1, 2).is_some());
        assert!(v.datagramm_hin(B, 1, 3).is_none());
        // Bekannte Kanäle gehen weiter.
        assert!(v.datagramm_hin(B, 1, 2).is_some());
    }

    #[test]
    fn name_mit_steuerzeichen_oder_nur_leerzeichen_wird_abgelehnt() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        for name in ["Mine\ncraft", "   ", "\u{7}"] {
            let a = rahmen(
                &mut v,
                A,
                ClientRahmen::Anbieten {
                    name: name.to_string(),
                    port: 25565,
                    art: Art::Tcp,
                },
            );
            assert_eq!(
                fehlercode(&a, A),
                Some(Fehler::UngueltigeAnfrage),
                "{name:?}"
            );
        }
        rahmen(
            &mut v,
            A,
            ClientRahmen::Anbieten {
                name: "  Minecraft  ".to_string(),
                port: 25565,
                art: Art::Tcp,
            },
        );
        assert_eq!(v.angebote_im_raum(RAUM)[0].name, "Minecraft");
    }

    // ------------------------------------------------------------- Durchsatz

    #[test]
    fn durchsatz_null_ist_unbegrenzt() {
        let mut v = Vermittlung::neu(Grenzen {
            kbit_je_freigabe: 0,
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let jetzt = Instant::now();
        for _ in 0..100 {
            assert!(v.durchsatz(1, 1500, jetzt));
        }
    }

    #[test]
    fn durchsatz_verwirft_zu_viel() {
        let mut v = Vermittlung::neu(Grenzen {
            kbit_je_freigabe: 8, // 1000 Byte je Sekunde
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        let jetzt = Instant::now();
        assert!(v.durchsatz(1, 1000, jetzt), "der Eimer ist voll");
        assert!(!v.durchsatz(1, 1000, jetzt), "jetzt ist er leer");
        // Nach zwei Sekunden ist wieder etwas drin.
        assert!(v.durchsatz(1, 1000, jetzt + Duration::from_secs(2)));
    }

    #[test]
    fn durchsatz_gilt_je_freigabe() {
        let mut v = Vermittlung::neu(Grenzen {
            kbit_je_freigabe: 8,
            ..Grenzen::default()
        });
        da(&mut v, A, RAUM);
        anbieten(&mut v, A, "Minecraft");
        anbieten(&mut v, A, "Sprachserver");
        let jetzt = Instant::now();
        assert!(v.durchsatz(1, 1000, jetzt));
        assert!(!v.durchsatz(1, 1000, jetzt));
        // Die zweite Freigabe hat ihren eigenen Eimer.
        assert!(v.durchsatz(2, 1000, jetzt));
    }

    #[test]
    fn durchsatz_unbekanntes_angebot_laesst_durch() {
        let mut v = frisch();
        assert!(v.durchsatz(99, 1500, Instant::now()));
    }

    // ------------------------------------------------------------- Anzeige

    #[test]
    fn angebote_im_raum_sind_sortiert() {
        let mut v = frisch();
        da(&mut v, A, RAUM);
        da(&mut v, B, RAUM);
        anbieten(&mut v, A, "Minecraft");
        anbieten(&mut v, B, "Sprachserver");
        let liste = v.angebote_im_raum(RAUM);
        assert_eq!(liste.iter().map(|a| a.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(liste[0].gastgeber, A);
        assert_eq!(liste[1].gastgeber, B);
        assert!(v.angebote_im_raum("leer").is_empty());
    }

    #[test]
    fn grenzen_werden_gemeldet() {
        let v = frisch();
        assert_eq!(v.grenzen(), Grenzen::default());
    }
}

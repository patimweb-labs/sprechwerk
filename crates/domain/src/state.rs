//! Der Zustand: Räume, Mitglieder, Nachrichten, Prüfprotokoll.
//!
//! Jede Zustandsänderung läuft über eine Methode dieses Typs. Die Methode
//! prüft das Recht, prüft die Eingabe, ändert den Zustand und schreibt **in
//! jedem Fall** einen Eintrag ins Prüfprotokoll — auch bei Ablehnung
//! (**Festlegung SPEC §8**).

use crate::ids::{MemberId, MessageId, RoomId};
use crate::role::{darf, Capability, Role};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Name des Raums, der beim Anlegen entsteht.
pub const DEFAULT_ROOM_NAME: &str = "Empfang";

/// Obergrenze einer Nachricht in Zeichen (**Festlegung SPEC §6**).
pub const MAX_MESSAGE_LEN: usize = 4_000;

/// Fehler, die eine Zustandsänderung ablehnen kann.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("unbekannter Raum")]
    RaumUnbekannt,
    #[error("unbekanntes Mitglied")]
    MitgliedUnbekannt,
    #[error("unbekannte Nachricht")]
    NachrichtUnbekannt,
    #[error("dafür fehlt das Recht: {0:?}")]
    RechtFehlt(Capability),
    #[error("der Raum ist nicht leer")]
    RaumNichtLeer,
    #[error("ungültiger Name")]
    NameUngueltig,
    #[error("der letzte Administrator kann nicht herabgestuft werden")]
    LetzterAdministrator,
    #[error("ein Raum kann nicht unter sich selbst gehängt werden")]
    Kreis,
    #[error("Nachricht ist zu lang (höchstens {MAX_MESSAGE_LEN} Zeichen)")]
    NachrichtZuLang,
    #[error("der Raum ist gesperrt")]
    RaumGesperrt,
    #[error("gesperrt: {0}")]
    Gesperrt(String),
    #[error("ungültige Kennung: {0}")]
    Kennung(String),
}

/// Ein Mitglied. Die Kennung `schluessel` ist der öffentliche Schlüssel in
/// Textform; sie ist die Identität (**Festlegung SPEC §4**).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: MemberId,
    pub schluessel: String,
    pub name: String,
    pub rolle: Role,
    /// Raum, in dem das Mitglied gerade sitzt.
    #[serde(default)]
    pub raum: Option<RoomId>,
    /// Zeitpunkt des Betretens des aktuellen Raums — Grundlage für die
    /// Verlaufsregel der Gäste.
    #[serde(default)]
    pub raum_seit: Option<i64>,
    pub angelegt_am: i64,
    pub zuletzt_gesehen: i64,
}

/// Ein Raum im Baum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: RoomId,
    #[serde(default)]
    pub eltern: Option<RoomId>,
    pub name: String,
    /// Sortierung unter Geschwistern.
    pub reihenfolge: i32,
    pub angelegt_am: i64,
}

/// Eine Nachricht. Löschen durch den Verfasser setzt `geloescht`, Löschen durch
/// einen Moderator entfernt den Text (**Festlegung SPEC §6**).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub raum: RoomId,
    pub verfasser: MemberId,
    pub verfasser_name: String,
    pub text: String,
    pub zeit: i64,
    #[serde(default)]
    pub geloescht: bool,
}

/// Ein Eintrag im Prüfprotokoll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub laufnummer: u64,
    pub zeit: i64,
    #[serde(default)]
    pub handelnder: Option<MemberId>,
    pub handelnder_name: String,
    /// Kurzbezeichnung der Aktion, etwa `raum_angelegt`.
    pub aktion: String,
    /// Betroffenes Objekt in Textform.
    pub ziel: String,
    #[serde(default)]
    pub vorher: Option<String>,
    #[serde(default)]
    pub nachher: Option<String>,
    /// `erlaubt` oder `abgelehnt: <grund>`.
    pub ergebnis: String,
}

/// Eine Sperre. **Festlegung SPEC §4:** Kennung **und** Adresse, wie bei
/// TeamSpeak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    pub schluessel: Option<String>,
    pub adresse: Option<String>,
    pub grund: String,
    pub zeit: i64,
    pub handelnder_name: String,
}

/// Der gesamte Zustand. Serialisierbar — das ist die Grundlage des Exports
/// (**Festlegung SPEC §2**).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Format der Ausgabe; wird beim Import geprüft.
    pub format_version: u32,
    pub server_name: String,
    /// Rolle, die ein neu angekommenes Mitglied bekommt.
    pub standard_rolle: Role,
    pub raeume: BTreeMap<RoomId, Room>,
    pub mitglieder: BTreeMap<MemberId, Member>,
    pub nachrichten: Vec<Message>,
    pub protokoll: Vec<AuditEntry>,
    pub sperren: Vec<Ban>,
    #[serde(default)]
    naechste_laufnummer: u64,
}

/// Aktuelles Format des Exports.
pub const FORMAT_VERSION: u32 = 1;

impl State {
    /// Neuer Zustand mit einem Empfangsraum.
    pub fn neu(server_name: impl Into<String>, standard_rolle: Role, jetzt_ms: i64) -> Self {
        let mut state = Self {
            format_version: FORMAT_VERSION,
            server_name: server_name.into(),
            standard_rolle,
            raeume: BTreeMap::new(),
            mitglieder: BTreeMap::new(),
            nachrichten: Vec::new(),
            protokoll: Vec::new(),
            sperren: Vec::new(),
            naechste_laufnummer: 1,
        };
        let id = RoomId::neu();
        state.raeume.insert(
            id,
            Room {
                id,
                eltern: None,
                name: DEFAULT_ROOM_NAME.to_string(),
                reihenfolge: 0,
                angelegt_am: jetzt_ms,
            },
        );
        state
    }

    // ---- Prüfprotokoll -------------------------------------------------

    fn protokolliere(
        &mut self,
        zeit: i64,
        handelnder: Option<MemberId>,
        handelnder_name: &str,
        aktion: &str,
        ziel: impl Into<String>,
        vorher: Option<String>,
        nachher: Option<String>,
        ergebnis: String,
    ) {
        let laufnummer = self.naechste_laufnummer;
        self.naechste_laufnummer += 1;
        self.protokoll.push(AuditEntry {
            laufnummer,
            zeit,
            handelnder,
            handelnder_name: handelnder_name.to_string(),
            aktion: aktion.to_string(),
            ziel: ziel.into(),
            vorher,
            nachher,
            ergebnis,
        });
    }

    /// Nächste freie Laufnummer des Protokolls.
    pub fn naechste_laufnummer(&self) -> u64 {
        self.naechste_laufnummer
    }

    /// Setzt die Laufnummer (nur beim Laden aus der Datenbank).
    pub fn setze_naechste_laufnummer(&mut self, wert: u64) {
        self.naechste_laufnummer = wert.max(1);
    }

    /// Protokolleinträge, neueste zuletzt.
    pub fn protokoll_lesen(&self, ab: usize, anzahl: usize) -> &[AuditEntry] {
        let start = ab.min(self.protokoll.len());
        let ende = (start + anzahl).min(self.protokoll.len());
        &self.protokoll[start..ende]
    }

    // ---- Anmeldung -----------------------------------------------------

    /// Meldet ein Mitglied an. Unbekannte Schlüssel legen ein Mitglied an; der
    /// **erste** Ankömmling auf einem leeren Server wird Administrator.
    pub fn anmelden(&mut self, schluessel: &str, name: &str, jetzt_ms: i64) -> Result<MemberId, Error> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            self.protokolliere(jetzt_ms, None, "", "anmeldung", schluessel.to_string(), None, None, "abgelehnt: ungültiger Name".into());
            return Err(Error::NameUngueltig);
        }
        if let Some(grund) = self.sperre_fuer(schluessel, None).map(|b| b.grund.clone()) {
            self.protokolliere(jetzt_ms, None, name, "anmeldung", schluessel.to_string(), None, None, format!("abgelehnt: gesperrt ({grund})"));
            return Err(Error::Gesperrt(grund));
        }

        if let Some(id) = self.mitglied_nach_schluessel(schluessel) {
            let (alter_name, alte_rolle);
            {
                let m = self.mitglieder.get_mut(&id).expect("gerade gefunden");
                alter_name = m.name.clone();
                alte_rolle = m.rolle.text();
                m.name = name.to_string();
                m.zuletzt_gesehen = jetzt_ms;
            }
            if alter_name != name {
                self.protokolliere(jetzt_ms, Some(id), name, "mitglied_umbenannt", id.text(), Some(alter_name), Some(name.to_string()), "erlaubt".into());
            }
            let _ = alte_rolle;
            return Ok(id);
        }

        // Erster Ankömmling wird Administrator (und damit der Betreiber).
        let erste = self.mitglieder.is_empty();
        let rolle = if erste { Role::Administrator } else { self.standard_rolle };
        let id = MemberId::neu();
        self.mitglieder.insert(
            id,
            Member {
                id,
                schluessel: schluessel.to_string(),
                name: name.to_string(),
                rolle,
                raum: None,
                raum_seit: None,
                angelegt_am: jetzt_ms,
                zuletzt_gesehen: jetzt_ms,
            },
        );
        self.protokolliere(
            jetzt_ms,
            Some(id),
            name,
            if erste { "erster_beitritt" } else { "mitglied_angelegt" },
            id.text(),
            None,
            Some(rolle.text().to_string()),
            "erlaubt".into(),
        );
        Ok(id)
    }

    /// Mitglied anhand des Schlüssels.
    pub fn mitglied_nach_schluessel(&self, schluessel: &str) -> Option<MemberId> {
        self.mitglieder
            .values()
            .find(|m| m.schluessel == schluessel)
            .map(|m| m.id)
    }

    /// Ein Mitglied.
    pub fn mitglied(&self, id: MemberId) -> Option<&Member> {
        self.mitglieder.get(&id)
    }

    // ---- Räume ---------------------------------------------------------

    /// Legt einen Raum an. `eltern = None` heißt oberste Ebene.
    pub fn raum_anlegen(
        &mut self,
        wer: MemberId,
        eltern: Option<RoomId>,
        name: &str,
        jetzt_ms: i64,
    ) -> Result<RoomId, Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RaumVerwalten, jetzt_ms, "raum_angelegt", name);
        if let Err(e) = ergebnis {
            return Err(e);
        }
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_angelegt", name, None, None, "abgelehnt: ungültiger Name".into());
            return Err(Error::NameUngueltig);
        }
        if let Some(p) = eltern {
            if !self.raeume.contains_key(&p) {
                self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_angelegt", name, None, None, "abgelehnt: Elternraum unbekannt".into());
                return Err(Error::RaumUnbekannt);
            }
        }
        let id = RoomId::neu();
        let reihenfolge = self
            .raeume
            .values()
            .filter(|r| r.eltern == eltern)
            .map(|r| r.reihenfolge + 1)
            .max()
            .unwrap_or(0);
        self.raeume.insert(
            id,
            Room {
                id,
                eltern,
                name: name.clone(),
                reihenfolge,
                angelegt_am: jetzt_ms,
            },
        );
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_angelegt", id.text(), None, Some(name), "erlaubt".into());
        Ok(id)
    }

    /// Benennt einen Raum um.
    pub fn raum_umbenennen(
        &mut self,
        wer: MemberId,
        raum: RoomId,
        name: &str,
        jetzt_ms: i64,
    ) -> Result<(), Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RaumVerwalten, jetzt_ms, "raum_umbenannt", raum.text());
        ergebnis?;
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_umbenannt", raum.text(), None, None, "abgelehnt: ungültiger Name".into());
            return Err(Error::NameUngueltig);
        }
        let alt;
        {
            let r = self.raeume.get_mut(&raum).ok_or(Error::RaumUnbekannt)?;
            alt = r.name.clone();
            r.name = name.clone();
        }
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_umbenannt", raum.text(), Some(alt), Some(name), "erlaubt".into());
        Ok(())
    }

    /// Hängt einen Raum unter einen anderen (oder an die Wurzel).
    pub fn raum_verschieben(
        &mut self,
        wer: MemberId,
        raum: RoomId,
        neuer_eltern: Option<RoomId>,
        jetzt_ms: i64,
    ) -> Result<(), Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RaumVerwalten, jetzt_ms, "raum_verschoben", raum.text());
        ergebnis?;
        if !self.raeume.contains_key(&raum) {
            return Err(Error::RaumUnbekannt);
        }
        if let Some(ziel) = neuer_eltern {
            if !self.raeume.contains_key(&ziel) {
                self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_verschoben", raum.text(), None, None, "abgelehnt: Ziel unbekannt".into());
                return Err(Error::RaumUnbekannt);
            }
            // Kreisprüfung: kein Vorfahre von `raum` darf Ziel sein.
            let mut lauf = Some(ziel);
            while let Some(k) = lauf {
                if k == raum {
                    self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_verschoben", raum.text(), None, None, "abgelehnt: Kreis".into());
                    return Err(Error::Kreis);
                }
                lauf = self.raeume.get(&k).and_then(|r| r.eltern);
            }
        }
        let alt = self.raeume.get(&raum).and_then(|r| r.eltern).map(|e| e.text());
        let reihenfolge = self
            .raeume
            .values()
            .filter(|r| r.eltern == neuer_eltern)
            .map(|r| r.reihenfolge + 1)
            .max()
            .unwrap_or(0);
        {
            let r = self.raeume.get_mut(&raum).expect("vorher geprüft");
            r.eltern = neuer_eltern;
            r.reihenfolge = reihenfolge;
        }
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_verschoben", raum.text(), alt, neuer_eltern.map(|e| e.text()), "erlaubt".into());
        Ok(())
    }

    /// Löscht einen Raum. Nur möglich, wenn er keine Unterräume mehr hat; die
    /// Nachrichten des Raums werden mitgelöscht (**Festlegung SPEC §6**).
    pub fn raum_loeschen(&mut self, wer: MemberId, raum: RoomId, jetzt_ms: i64) -> Result<(), Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RaumVerwalten, jetzt_ms, "raum_geloescht", raum.text());
        ergebnis?;
        if !self.raeume.contains_key(&raum) {
            return Err(Error::RaumUnbekannt);
        }
        if self.raeume.values().any(|r| r.eltern == Some(raum)) {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_geloescht", raum.text(), None, None, "abgelehnt: Unterräume vorhanden".into());
            return Err(Error::RaumNichtLeer);
        }
        // Anwesende in die Wurzel setzen, damit niemand in einem Loch sitzt.
        let wurzel = self.wurzel_raum();
        for m in self.mitglieder.values_mut() {
            if m.raum == Some(raum) {
                m.raum = wurzel;
                m.raum_seit = Some(jetzt_ms);
            }
        }
        let name = self.raeume.remove(&raum).map(|r| r.name).unwrap_or_default();
        let vorher = self.nachrichten.len();
        self.nachrichten.retain(|n| n.raum != raum);
        let entfernt = vorher - self.nachrichten.len();
        self.protokolliere(
            jetzt_ms,
            Some(wer),
            &handelnder_name,
            "raum_geloescht",
            raum.text(),
            Some(name),
            Some(format!("{entfernt} Nachrichten entfernt")),
            "erlaubt".into(),
        );
        Ok(())
    }

    /// Oberster Raum (erster ohne Eltern), falls vorhanden.
    pub fn wurzel_raum(&self) -> Option<RoomId> {
        self.raeume
            .values()
            .filter(|r| r.eltern.is_none())
            .min_by_key(|r| (r.reihenfolge, r.name.clone()))
            .map(|r| r.id)
    }

    /// Alle Räume, nach Baumlage sortiert (Eltern vor Kindern).
    pub fn raumliste(&self) -> Vec<&Room> {
        fn sammle<'a>(
            raeume: &'a BTreeMap<RoomId, Room>,
            eltern: Option<RoomId>,
            tiefe: usize,
            aus: &mut Vec<(usize, &'a Room)>,
        ) {
            let mut kinder: Vec<&Room> = raeume
                .values()
                .filter(|r| r.eltern == eltern)
                .collect();
            kinder.sort_by(|a, b| (a.reihenfolge, &a.name).cmp(&(b.reihenfolge, &b.name)));
            for k in kinder {
                aus.push((tiefe, k));
                sammle(raeume, Some(k.id), tiefe + 1, aus);
            }
        }
        let mut aus = Vec::new();
        sammle(&self.raeume, None, 0, &mut aus);
        aus.into_iter().map(|(_, r)| r).collect()
    }

    // ---- Anwesenheit ---------------------------------------------------

    /// Betritt einen Raum und liefert den sichtbaren Verlauf.
    pub fn betreten(&mut self, wer: MemberId, raum: RoomId, jetzt_ms: i64) -> Result<Vec<Message>, Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RaumBetreten, jetzt_ms, "raum_betreten", raum.text());
        ergebnis?;
        if !self.raeume.contains_key(&raum) {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_betreten", raum.text(), None, None, "abgelehnt: Raum unbekannt".into());
            return Err(Error::RaumUnbekannt);
        }
        {
            let m = self.mitglieder.get_mut(&wer).expect("vorher geprüft");
            m.raum = Some(raum);
            m.raum_seit = Some(jetzt_ms);
            m.zuletzt_gesehen = jetzt_ms;
        }
        let verlauf = self.verlauf(wer, raum, jetzt_ms)?;
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_betreten", raum.text(), None, None, "erlaubt".into());
        Ok(verlauf)
    }

    /// Verlässt den aktuellen Raum.
    pub fn verlassen(&mut self, wer: MemberId, jetzt_ms: i64) -> Result<(), Error> {
        let (handelnder_name, raum) = {
            let m = self.mitglieder.get_mut(&wer).ok_or(Error::MitgliedUnbekannt)?;
            let raum = m.raum.take().map(|r| r.text());
            m.raum_seit = None;
            m.zuletzt_gesehen = jetzt_ms;
            (m.name.clone(), raum)
        };
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "raum_verlassen", raum.unwrap_or_default(), None, None, "erlaubt".into());
        Ok(())
    }

    /// Wer ist gerade in diesem Raum?
    pub fn anwesende(&self, raum: RoomId) -> Vec<&Member> {
        let mut aus: Vec<&Member> = self.mitglieder.values().filter(|m| m.raum == Some(raum)).collect();
        aus.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        aus
    }

    /// Sichtbarer Verlauf eines Raums. Ohne [`Capability::VerlaufLesen`] nur
    /// das, was seit dem Betreten geschrieben wurde (**Festlegung SPEC §5**).
    pub fn verlauf(&self, wer: MemberId, raum: RoomId, _jetzt_ms: i64) -> Result<Vec<Message>, Error> {
        let m = self.mitglieder.get(&wer).ok_or(Error::MitgliedUnbekannt)?;
        let ab = if darf(m.rolle, Capability::VerlaufLesen) {
            i64::MIN
        } else {
            m.raum_seit.unwrap_or(i64::MAX)
        };
        Ok(self
            .nachrichten
            .iter()
            .filter(|n| n.raum == raum && n.zeit >= ab)
            .cloned()
            .collect())
    }

    // ---- Text ----------------------------------------------------------

    /// Schreibt eine Nachricht in den Raum, in dem der Verfasser sitzt.
    pub fn text_senden(&mut self, wer: MemberId, text: &str, jetzt_ms: i64) -> Result<Message, Error> {
        let (handelnder_name, raum) = {
            let m = self.mitglieder.get(&wer).ok_or(Error::MitgliedUnbekannt)?;
            (m.name.clone(), m.raum)
        };
        let Some(raum) = raum else {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_gesendet", "", None, None, "abgelehnt: in keinem Raum".into());
            return Err(Error::RaumUnbekannt);
        };
        let (_, ergebnis) = self.vorpruefung_person(wer, Capability::TextSchreiben, jetzt_ms, "text_gesendet", raum.text());
        ergebnis?;
        let text = text.trim_end().to_string();
        if text.is_empty() {
            return Err(Error::NameUngueltig);
        }
        if text.chars().count() > MAX_MESSAGE_LEN {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_gesendet", raum.text(), None, None, "abgelehnt: zu lang".into());
            return Err(Error::NachrichtZuLang);
        }
        let nachricht = Message {
            id: MessageId::neu(),
            raum,
            verfasser: wer,
            verfasser_name: handelnder_name.clone(),
            text,
            zeit: jetzt_ms,
            geloescht: false,
        };
        self.nachrichten.push(nachricht.clone());
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_gesendet", nachricht.id.text(), None, None, "erlaubt".into());
        Ok(nachricht)
    }

    /// Löscht eine Nachricht. Der Verfasser setzt einen Platzhalter, wer
    /// [`Capability::PersonEingreifen`] hat, löscht hart.
    pub fn nachricht_loeschen(&mut self, wer: MemberId, id: MessageId, jetzt_ms: i64) -> Result<bool, Error> {
        let (handelnder_name, rolle) = {
            let m = self.mitglieder.get(&wer).ok_or(Error::MitgliedUnbekannt)?;
            (m.name.clone(), m.rolle)
        };
        let index = self
            .nachrichten
            .iter()
            .position(|n| n.id == id)
            .ok_or(Error::NachrichtUnbekannt)?;
        let ist_verfasser = self.nachrichten[index].verfasser == wer;
        let darf_hart = darf(rolle, Capability::PersonEingreifen);
        if !ist_verfasser && !darf_hart {
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_geloescht", id.text(), None, None, "abgelehnt: kein Recht".into());
            return Err(Error::RechtFehlt(Capability::PersonEingreifen));
        }
        // Der Verfasser löscht **immer** weich: der Verlauf bleibt lesbar, und
        // niemand kann durch Löschen nachträglich ein Gespräch verfälschen.
        if ist_verfasser {
            let n = &mut self.nachrichten[index];
            n.geloescht = true;
            n.text = String::new();
            self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_geloescht", id.text(), None, Some("Platzhalter".into()), "erlaubt: weich".into());
            return Ok(false);
        }
        // Fremde Nachricht: nur mit Eingriffsrecht, dann hart.
        let n = self.nachrichten.remove(index);
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "text_geloescht", id.text(), Some(n.text), None, "erlaubt: hart".into());
        Ok(true)
    }

    // ---- Rollen --------------------------------------------------------

    /// Setzt die Rolle eines Mitglieds.
    pub fn rolle_setzen(&mut self, wer: MemberId, ziel: MemberId, rolle: Role, jetzt_ms: i64) -> Result<(), Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::RollenVergeben, jetzt_ms, "rolle_gesetzt", ziel.text());
        ergebnis?;
        let alt = {
            let m = self.mitglieder.get(&ziel).ok_or(Error::MitgliedUnbekannt)?;
            m.rolle
        };
        // Der letzte Administrator bleibt Administrator — sonst ist der Server
        // unverwaltbar und niemand kann es rückgängig machen.
        if alt == Role::Administrator && rolle != Role::Administrator {
            let weitere = self
                .mitglieder
                .values()
                .filter(|m| m.rolle == Role::Administrator && m.id != ziel)
                .count();
            if weitere == 0 {
                self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "rolle_gesetzt", ziel.text(), Some(alt.text().into()), Some(rolle.text().into()), "abgelehnt: letzter Administrator".into());
                return Err(Error::LetzterAdministrator);
            }
        }
        self.mitglieder.get_mut(&ziel).expect("vorher geprüft").rolle = rolle;
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "rolle_gesetzt", ziel.text(), Some(alt.text().into()), Some(rolle.text().into()), "erlaubt".into());
        Ok(())
    }

    // ---- Sperren -------------------------------------------------------

    /// Verhängt eine Sperre über Kennung und/oder Adresse.
    pub fn sperren(
        &mut self,
        wer: MemberId,
        schluessel: Option<String>,
        adresse: Option<String>,
        grund: &str,
        jetzt_ms: i64,
    ) -> Result<(), Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::Sperren, jetzt_ms, "sperre_verhaengt", schluessel.clone().unwrap_or_else(|| adresse.clone().unwrap_or_default()));
        ergebnis?;
        if schluessel.is_none() && adresse.is_none() {
            return Err(Error::NameUngueltig);
        }
        self.sperren.push(Ban {
            schluessel,
            adresse,
            grund: grund.to_string(),
            zeit: jetzt_ms,
            handelnder_name: handelnder_name.clone(),
        });
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "sperre_verhaengt", grund.to_string(), None, None, "erlaubt".into());
        Ok(())
    }

    /// Hebt alle Sperren auf, die zu Kennung oder Adresse passen.
    pub fn entsperren(&mut self, wer: MemberId, schluessel: Option<&str>, adresse: Option<&str>, jetzt_ms: i64) -> Result<usize, Error> {
        let (handelnder_name, ergebnis) = self.vorpruefung_person(wer, Capability::Sperren, jetzt_ms, "sperre_aufgehoben", schluessel.unwrap_or_default().to_string());
        ergebnis?;
        let vorher = self.sperren.len();
        self.sperren.retain(|b| {
            let trifft_schluessel = matches!((&b.schluessel, schluessel), (Some(a), Some(c)) if a == c);
            let trifft_adresse = matches!((&b.adresse, adresse), (Some(a), Some(c)) if a == c);
            !(trifft_schluessel || trifft_adresse)
        });
        let anzahl = vorher - self.sperren.len();
        self.protokolliere(jetzt_ms, Some(wer), &handelnder_name, "sperre_aufgehoben", format!("{anzahl} Einträge"), None, None, "erlaubt".into());
        Ok(anzahl)
    }

    /// Passende Sperre, falls vorhanden.
    pub fn sperre_fuer(&self, schluessel: &str, adresse: Option<&str>) -> Option<&Ban> {
        self.sperren.iter().find(|b| {
            b.schluessel.as_deref() == Some(schluessel)
                || (adresse.is_some() && b.adresse.as_deref() == adresse)
        })
    }

    // ---- Hilfen --------------------------------------------------------

    /// Prüft Person und Recht und protokolliert eine Ablehnung. Liefert Name
    /// des Handelnden und das Ergebnis der Prüfung.
    fn vorpruefung_person(
        &mut self,
        wer: MemberId,
        recht: Capability,
        jetzt_ms: i64,
        aktion: &str,
        ziel: impl Into<String>,
    ) -> (String, Result<(), Error>) {
        let Some(m) = self.mitglieder.get(&wer) else {
            return (String::new(), Err(Error::MitgliedUnbekannt));
        };
        let name = m.name.clone();
        if darf(m.rolle, recht) {
            (name, Ok(()))
        } else {
            let ziel = ziel.into();
            self.protokolliere(
                jetzt_ms,
                Some(wer),
                &name,
                aktion,
                ziel,
                None,
                None,
                format!("abgelehnt: Recht fehlt ({recht:?})"),
            );
            (name, Err(Error::RechtFehlt(recht)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zustand() -> State {
        State::neu("Testwerk", Role::Mitglied, 1_000)
    }

    fn admin(state: &mut State) -> MemberId {
        state.anmelden("schluessel-admin", "Ada", 1_000).unwrap()
    }

    #[test]
    fn erster_ankoemmling_wird_administrator() {
        let mut s = zustand();
        let a = s.anmelden("k1", "Ada", 1_000).unwrap();
        assert_eq!(s.mitglied(a).unwrap().rolle, Role::Administrator);
        let b = s.anmelden("k2", "Bosse", 1_001).unwrap();
        assert_eq!(s.mitglied(b).unwrap().rolle, Role::Mitglied);
    }

    #[test]
    fn wiederanmeldung_behaelt_kennung() {
        let mut s = zustand();
        let a = s.anmelden("k1", "Ada", 1_000).unwrap();
        let wieder = s.anmelden("k1", "Ada", 2_000).unwrap();
        assert_eq!(a, wieder);
        assert_eq!(s.mitglieder.len(), 1);
        assert!(s.protokoll.iter().any(|e| e.aktion == "mitglied_umbenannt") == false);
    }

    #[test]
    fn umbenennung_landet_im_protokoll() {
        let mut s = zustand();
        let a = s.anmelden("k1", "Ada", 1_000).unwrap();
        s.anmelden("k1", "Ada Lovelace", 2_000).unwrap();
        let letzte = s.protokoll.last().unwrap();
        assert_eq!(letzte.aktion, "mitglied_umbenannt");
        assert_eq!(letzte.vorher.as_deref(), Some("Ada"));
        assert_eq!(letzte.nachher.as_deref(), Some("Ada Lovelace"));
        assert_eq!(s.mitglied(a).unwrap().name, "Ada Lovelace");
    }

    #[test]
    fn raum_anlegen_braucht_recht() {
        let mut s = zustand();
        let a = admin(&mut s);
        let b = s.anmelden("k2", "Bosse", 1_001).unwrap();
        assert_eq!(
            s.raum_anlegen(b, None, "Verbote", 1_002),
            Err(Error::RechtFehlt(Capability::RaumVerwalten))
        );
        // Die Ablehnung steht im Protokoll.
        assert!(s
            .protokoll
            .iter()
            .any(|e| e.aktion == "raum_angelegt" && e.ergebnis.starts_with("abgelehnt")));
        let id = s.raum_anlegen(a, None, "Vorstand", 1_003).unwrap();
        assert_eq!(s.raeume.get(&id).unwrap().name, "Vorstand");
    }

    #[test]
    fn unterraum_und_sortierung() {
        let mut s = zustand();
        let a = admin(&mut s);
        let oben = s.raum_anlegen(a, None, "B", 1_010).unwrap();
        let unten = s.raum_anlegen(a, Some(oben), "B1", 1_011).unwrap();
        let liste = s.raumliste();
        let namen: Vec<&str> = liste.iter().map(|r| r.name.as_str()).collect();
        // Empfang ist der Wurzelraum, danach kommt B, darunter B1.
        assert_eq!(namen, vec![DEFAULT_ROOM_NAME, "B", "B1"]);
        assert_eq!(s.raeume.get(&unten).unwrap().eltern, Some(oben));
    }

    #[test]
    fn raum_kann_nicht_unter_sich_selbst() {
        let mut s = zustand();
        let a = admin(&mut s);
        let oben = s.raum_anlegen(a, None, "A", 1_010).unwrap();
        let unten = s.raum_anlegen(a, Some(oben), "A1", 1_011).unwrap();
        assert_eq!(s.raum_verschieben(a, oben, Some(unten), 1_012), Err(Error::Kreis));
    }

    #[test]
    fn raum_mit_unterraum_laesst_sich_nicht_loeschen() {
        let mut s = zustand();
        let a = admin(&mut s);
        let oben = s.raum_anlegen(a, None, "A", 1_010).unwrap();
        s.raum_anlegen(a, Some(oben), "A1", 1_011).unwrap();
        assert_eq!(s.raum_loeschen(a, oben, 1_012), Err(Error::RaumNichtLeer));
        let unten = s.raeume.values().find(|r| r.eltern == Some(oben)).unwrap().id;
        s.raum_loeschen(a, unten, 1_013).unwrap();
        s.raum_loeschen(a, oben, 1_014).unwrap();
        assert!(!s.raeume.contains_key(&oben));
    }

    #[test]
    fn loeschen_setzt_anwesende_in_die_wurzel() {
        let mut s = zustand();
        let a = admin(&mut s);
        let raum = s.raum_anlegen(a, None, "A", 1_010).unwrap();
        s.betreten(a, raum, 1_011).unwrap();
        s.raum_loeschen(a, raum, 1_012).unwrap();
        let wurzel = s.wurzel_raum().unwrap();
        assert_eq!(s.mitglied(a).unwrap().raum, Some(wurzel));
    }

    #[test]
    fn gast_sieht_nur_verlauf_ab_betreten() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        s.betreten(a, empfang, 1_000).unwrap();
        s.text_senden(a, "vorher", 1_001).unwrap();

        let gast = s.anmelden("k-gast", "Gustav", 2_000).unwrap();
        s.rolle_setzen(a, gast, Role::Gast, 2_001).unwrap();
        let verlauf = s.betreten(gast, empfang, 2_002).unwrap();
        assert!(verlauf.is_empty(), "Gast darf alten Verlauf nicht sehen");

        s.text_senden(a, "während", 2_003).unwrap();
        let verlauf = s.verlauf(gast, empfang, 2_004).unwrap();
        assert_eq!(verlauf.len(), 1);
        assert_eq!(verlauf[0].text, "während");
    }

    #[test]
    fn mitglied_sieht_ganzen_verlauf() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        s.betreten(a, empfang, 1_000).unwrap();
        s.text_senden(a, "alt", 1_001).unwrap();
        let b = s.anmelden("k2", "Bosse", 2_000).unwrap();
        let verlauf = s.betreten(b, empfang, 2_001).unwrap();
        assert_eq!(verlauf.len(), 1);
        assert_eq!(verlauf[0].text, "alt");
    }

    #[test]
    fn gast_darf_nicht_sprechen() {
        let mut s = zustand();
        let a = admin(&mut s);
        let g = s.anmelden("k-gast", "Gustav", 2_000).unwrap();
        s.rolle_setzen(a, g, Role::Gast, 2_001).unwrap();
        assert!(!darf(s.mitglied(g).unwrap().rolle, Capability::SpracheSenden));
    }

    #[test]
    fn gast_schreibt_nachricht() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        let g = s.anmelden("k-gast", "Gustav", 2_000).unwrap();
        s.rolle_setzen(a, g, Role::Gast, 2_001).unwrap();
        s.betreten(g, empfang, 2_002).unwrap();
        let n = s.text_senden(g, "hallo", 2_003).unwrap();
        assert_eq!(n.text, "hallo");
    }

    #[test]
    fn verfasser_loescht_weich_moderator_hart() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        s.betreten(a, empfang, 1_000).unwrap();
        let n1 = s.text_senden(a, "eins", 1_001).unwrap();
        let n2 = s.text_senden(a, "zwei", 1_002).unwrap();

        // Verfasser löscht: Platzhalter bleibt.
        let hart = s.nachricht_loeschen(a, n1.id, 1_003).unwrap();
        assert!(!hart);
        let n = s.nachrichten.iter().find(|n| n.id == n1.id).unwrap();
        assert!(n.geloescht);
        assert_eq!(n.text, "");

        // Auch ein Administrator löscht die **eigene** Nachricht nur weich.
        let hart = s.nachricht_loeschen(a, n2.id, 1_004).unwrap();
        assert!(!hart);

        // Ein Moderator löscht eine **fremde** Nachricht hart.
        let m = s.anmelden("k-mod", "Mira", 2_000).unwrap();
        s.rolle_setzen(a, m, Role::Moderator, 2_001).unwrap();
        s.betreten(m, empfang, 2_002).unwrap();
        let n3 = s.text_senden(a, "drei", 2_003).unwrap();
        let hart = s.nachricht_loeschen(m, n3.id, 2_004).unwrap();
        assert!(hart);
        assert!(s.nachrichten.iter().all(|n| n.id != n3.id));
    }

    #[test]
    fn fremde_nachricht_laesst_sich_nicht_loeschen() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        s.betreten(a, empfang, 1_000).unwrap();
        let n = s.text_senden(a, "eins", 1_001).unwrap();
        let b = s.anmelden("k2", "Bosse", 2_000).unwrap();
        s.betreten(b, empfang, 2_001).unwrap();
        assert_eq!(
            s.nachricht_loeschen(b, n.id, 2_002),
            Err(Error::RechtFehlt(Capability::PersonEingreifen))
        );
    }

    #[test]
    fn letzter_administrator_bleibt() {
        let mut s = zustand();
        let a = admin(&mut s);
        assert_eq!(
            s.rolle_setzen(a, a, Role::Mitglied, 1_100),
            Err(Error::LetzterAdministrator)
        );
        let b = s.anmelden("k2", "Bosse", 1_101).unwrap();
        s.rolle_setzen(a, b, Role::Administrator, 1_102).unwrap();
        // Jetzt darf der erste herabgestuft werden.
        s.rolle_setzen(a, a, Role::Mitglied, 1_103).unwrap();
        assert_eq!(s.mitglied(a).unwrap().rolle, Role::Mitglied);
    }

    #[test]
    fn nachricht_zu_lang_wird_abgelehnt() {
        let mut s = zustand();
        let a = admin(&mut s);
        let empfang = s.wurzel_raum().unwrap();
        s.betreten(a, empfang, 1_000).unwrap();
        let lang = "x".repeat(MAX_MESSAGE_LEN + 1);
        assert_eq!(s.text_senden(a, &lang, 1_001), Err(Error::NachrichtZuLang));
    }

    #[test]
    fn sperre_verhindert_anmeldung() {
        let mut s = zustand();
        let a = admin(&mut s);
        let b = s.anmelden("k2", "Bosse", 1_001).unwrap();
        s.sperren(a, Some("k2".into()), None, "Hausfriedensbruch", 1_002)
            .unwrap();
        assert!(matches!(s.anmelden("k2", "Bosse", 1_003), Err(Error::Gesperrt(_))));
        // Entsperren wirkt.
        s.entsperren(a, Some("k2"), None, 1_004).unwrap();
        let wieder = s.anmelden("k2", "Bosse", 1_005).unwrap();
        assert_eq!(wieder, b);
    }

    #[test]
    fn moderator_darf_nicht_sperren() {
        let mut s = zustand();
        let a = admin(&mut s);
        let m = s.anmelden("k2", "Mira", 1_001).unwrap();
        s.rolle_setzen(a, m, Role::Moderator, 1_002).unwrap();
        assert_eq!(
            s.sperren(m, Some("k3".into()), None, "test", 1_003),
            Err(Error::RechtFehlt(Capability::Sperren))
        );
    }

    #[test]
    fn anwesenheit_ist_sortiert() {
        let mut s = zustand();
        let a = admin(&mut s);
        let raum = s.wurzel_raum().unwrap();
        let b = s.anmelden("k2", "anton", 1_001).unwrap();
        let c = s.anmelden("k3", "Berta", 1_002).unwrap();
        for (id, t) in [(a, 1_010), (b, 1_011), (c, 1_012)] {
            s.betreten(id, raum, t).unwrap();
        }
        let namen: Vec<&str> = s.anwesende(raum).iter().map(|m| m.name.as_str()).collect();
        assert_eq!(namen, vec!["Ada", "anton", "Berta"]);
    }

    #[test]
    fn jeder_fehlversuch_steht_im_protokoll() {
        let mut s = zustand();
        let a = admin(&mut s);
        let b = s.anmelden("k2", "Bosse", 1_001).unwrap();
        let _ = s.raum_anlegen(b, None, "Verboten", 1_002);
        let eintrag = s.protokoll.last().unwrap();
        assert_eq!(eintrag.aktion, "raum_angelegt");
        assert_eq!(eintrag.handelnder, Some(b));
        assert!(eintrag.ergebnis.contains("RaumVerwalten"));
        let _ = a;
    }

    #[test]
    fn protokoll_bietet_seiten() {
        let mut s = zustand();
        let a = admin(&mut s);
        for i in 0..10 {
            s.raum_anlegen(a, None, &format!("R{i}"), 2_000 + i).unwrap();
        }
        let seite = s.protokoll_lesen(2, 3);
        assert_eq!(seite.len(), 3);
        assert_eq!(seite[0].laufnummer, s.protokoll[2].laufnummer);
    }

    #[test]
    fn kennung_aus_text_und_zurueck() {
        let id = RoomId::neu();
        assert_eq!(RoomId::from_text(&id.text()).unwrap(), id);
        assert_eq!(RoomId::from_text(&format!("{id}")).unwrap(), id);
        assert!(RoomId::from_text("raum_xyz").is_err());
        assert!(RoomId::from_text("").is_err());
    }

    #[test]
    fn export_und_import_sind_verlustfrei() {
        let mut s = zustand();
        let a = admin(&mut s);
        let raum = s.raum_anlegen(a, None, "Vorstand", 1_010).unwrap();
        s.betreten(a, raum, 1_011).unwrap();
        s.text_senden(a, "Protokoll", 1_012).unwrap();
        s.sperren(a, Some("k-boese".into()), Some("192.0.2.7".into()), "Spam", 1_013)
            .unwrap();

        let text = serde_json::to_string(&s).unwrap();
        let zurueck: State = serde_json::from_str(&text).unwrap();
        assert_eq!(zurueck.raeume.len(), s.raeume.len());
        assert_eq!(zurueck.nachrichten, s.nachrichten);
        assert_eq!(zurueck.protokoll, s.protokoll);
        assert_eq!(zurueck.sperren, s.sperren);
        assert_eq!(zurueck.server_name, s.server_name);
    }
}

//! Der Vertrag zwischen Fenster und Logik.
//!
//! **Regel (AGENTS.md §2):** Die Oberfläche ist `render(modell)`. Deshalb gibt
//! es genau **einen** Zustandstyp [`Zustand`], den der Rust-Teil nach jeder
//! Änderung an das Fenster schickt. Die Oberfläche rechnet nichts nach und hält
//! keinen eigenen Zustand, der nicht im Modell steht.
//!
//! Befehle sind die zweite Hälfte: sie ändern etwas und antworten mit
//! [`Antwort`]. Jeder Befehl hat ein Gegenstück in `ui/src/api.ts` — die
//! Vertragsprüfung (`tests/vertrag.rs`) hält beide Seiten zusammen.

pub mod abbilden;

use serde::{Deserialize, Serialize};
use sprechwerk_domain::{MemberId, MessageId, Role, RoomId};

/// Rolle der Verbindung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Verbindung {
    /// Noch nicht verbunden.
    Getrennt,
    /// Verbindungsaufbau läuft.
    Verbindet { server: String },
    /// Angemeldet.
    Verbunden {
        server: String,
        server_name: String,
        ich: MemberId,
        name: String,
        rolle: Role,
    },
    /// Abgelehnt oder abgebrochen.
    Fehler { text: String },
}

impl Verbindung {
    pub fn ist_verbunden(&self) -> bool {
        matches!(self, Verbindung::Verbunden { .. })
    }
}

/// Ein Raum, wie ihn das Fenster zeigt (flach; der Baum steckt in `eltern`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raum {
    pub id: RoomId,
    pub name: String,
    pub anwesend: usize,
    /// Ist das der Raum, in dem ich sitze?
    pub hier: bool,
    /// Sprache Ende zu Ende verschlüsselt (ADR 0006).
    pub verschluesselt: bool,
}

/// Ein Mitglied, wie es das Fenster zeigt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mitglied {
    pub id: MemberId,
    pub name: String,
    pub rolle: Role,
    pub raum: Option<RoomId>,
    /// Spricht gerade (Sprachrahmen in den letzten 400 ms).
    pub spricht: bool,
    /// Vom Server stummgeschaltet (Mikrofon).
    pub stumm: bool,
    /// Vom Server taubgeschaltet (Wiedergabe).
    pub taub: bool,
    /// Bin ich das?
    pub ich: bool,
}

/// Eine Nachricht im aktuellen Raum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nachricht {
    pub id: MessageId,
    pub verfasser: MemberId,
    pub verfasser_name: String,
    pub text: String,
    /// Zeit in Millisekunden seit der Epoche.
    pub zeit: i64,
    pub geloescht: bool,
    /// Darf ich sie löschen? (eigene Nachricht oder Moderator)
    pub loeschbar: bool,
}

/// Der vollständige Modellstand für die Oberfläche.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zustand {
    pub verbindung: Option<Verbindung>,
    /// Raum, in dem ich sitze.
    pub aktueller_raum: Option<RoomId>,
    pub raeume: Vec<Raum>,
    pub mitglieder: Vec<Mitglied>,
    pub nachrichten: Vec<Nachricht>,
    /// Betriebszahlen des Servers (leer, bis sie einmal abgefragt wurden).
    #[serde(default)]
    pub guete: Option<Guete>,
    /// Zuletzt gelesenes Prüfprotokoll (leer, solange niemand es geöffnet hat).
    ///
    /// Es steht im Modell, nicht in der Ansicht: die Oberfläche ist
    /// `render(modell)`, und ein zweiter Zustand neben dem Modell wäre genau die
    /// Doppelung, die dieses Projekt vermeidet.
    #[serde(default)]
    pub protokoll: Vec<Eintrag>,
    /// Letzte Meldung an den Nutzer (Fehler, Hinweis).
    pub meldung: Option<String>,
}

/// Wie viele Protokollzeilen ein Aufruf ohne Angabe liest.
fn fuenfzig() -> usize {
    50
}

/// Betriebszahlen für die Statuszeile.
///
/// Kein Personenbezug: Zahlen über den Server, nicht über Menschen. Deshalb
/// darf sie jeder sehen — „0 verworfen" ist die ehrlichste Auskunft, die ein
/// Sprachserver geben kann.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guete {
    pub sprachrahmen: u64,
    pub verteilt: u64,
    pub verloren: u64,
    pub abgelehnt: u64,
    pub verbundene: usize,
}

/// Eine Zeile des Prüfprotokolls, wie sie das Fenster zeigt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eintrag {
    pub laufnummer: u64,
    /// Zeitpunkt in Millisekunden seit der Epoche.
    pub zeit: i64,
    pub handelnder_name: String,
    /// Kurzbezeichnung, etwa `raum_angelegt`.
    pub aktion: String,
    /// Betroffenes Objekt in Textform.
    pub ziel: String,
    #[serde(default)]
    pub vorher: Option<String>,
    #[serde(default)]
    pub nachher: Option<String>,
    /// „erlaubt" oder „abgelehnt: …" — die zweite Hälfte ist der eigentliche
    /// Wert: man sieht, was **nicht** passiert ist.
    pub ergebnis: String,
}

impl Zustand {
    /// Ein leerer Zustand ist „getrennt".
    pub fn neu() -> Self {
        Self {
            verbindung: Some(Verbindung::Getrennt),
            ..Default::default()
        }
    }

    /// Eigene Kennung, falls verbunden.
    pub fn ich(&self) -> Option<MemberId> {
        match &self.verbindung {
            Some(Verbindung::Verbunden { ich, .. }) => Some(*ich),
            _ => None,
        }
    }

    /// Eigene Rolle, falls verbunden.
    pub fn meine_rolle(&self) -> Option<Role> {
        match &self.verbindung {
            Some(Verbindung::Verbunden { rolle, .. }) => Some(*rolle),
            _ => None,
        }
    }

    /// Darf ich Räume verwalten? Die Oberfläche blendet Schaltflächen danach
    /// ein — **Sichtbarkeit folgt aus dem Modell** (AGENTS.md §2.3).
    pub fn darf_raeume_verwalten(&self) -> bool {
        self.meine_rolle()
            .map(|r| sprechwerk_domain::role::darf(r, sprechwerk_domain::Capability::RaumVerwalten))
            .unwrap_or(false)
    }

    /// Darf ich Rollen vergeben?
    pub fn darf_rollen_vergeben(&self) -> bool {
        self.meine_rolle()
            .map(|r| {
                sprechwerk_domain::role::darf(r, sprechwerk_domain::Capability::RollenVergeben)
            })
            .unwrap_or(false)
    }

    /// Darf ich das Protokoll lesen?
    pub fn darf_protokoll_lesen(&self) -> bool {
        self.meine_rolle()
            .map(|r| {
                sprechwerk_domain::role::darf(r, sprechwerk_domain::Capability::ProtokollLesen)
            })
            .unwrap_or(false)
    }
}

/// Was das Fenster vom Rust-Teil will.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "befehl", rename_all = "snake_case")]
pub enum Befehl {
    Zustand,
    Verbinden {
        server: String,
        name: String,
        #[serde(default)]
        passwort: Option<String>,
    },
    Trennen,
    Betreten {
        raum: RoomId,
    },
    Text {
        text: String,
    },
    NachrichtLoeschen {
        nachricht: MessageId,
    },
    RaumAnlegen {
        name: String,
    },
    RaumUmbenennen {
        raum: RoomId,
        name: String,
    },
    RaumLoeschen {
        raum: RoomId,
    },
    RolleSetzen {
        mitglied: MemberId,
        rolle: Role,
    },
    /// Mikrofon (`taub = false`) oder Wiedergabe (`taub = true`) stummschalten.
    Stummschalten {
        mitglied: MemberId,
        #[serde(default)]
        taub: bool,
        an: bool,
    },
    /// Ein Mitglied in einen anderen Raum verschieben.
    Verschieben {
        mitglied: MemberId,
        raum: RoomId,
    },
    /// Ein Mitglied aus seinem Raum werfen (zurück in den Wurzelraum).
    AusRaumWerfen {
        mitglied: MemberId,
    },
    /// Ein Mitglied vom Server trennen (Rauswurf).
    MitgliedTrennen {
        mitglied: MemberId,
        #[serde(default)]
        grund: String,
    },
    /// Ende-zu-Ende-Verschlüsselung eines Raums ein-/ausschalten.
    RaumVerschluesseln {
        raum: RoomId,
        an: bool,
    },
    /// Ein Mitglied sperren und trennen.
    Sperren {
        mitglied: MemberId,
        #[serde(default)]
        grund: String,
    },
    /// Eine Sperre aufheben (Kennung und/oder Adresse).
    Entsperren {
        #[serde(default)]
        kennung: Option<String>,
        #[serde(default)]
        adresse: Option<String>,
    },
    /// Betriebszahlen abfragen.
    Zaehler,
    /// Prüfprotokoll lesen (Administrator und Moderator).
    Protokoll {
        #[serde(default)]
        ab: usize,
        #[serde(default = "fuenfzig")]
        anzahl: usize,
    },
}

/// Was der Rust-Teil antwortet.
///
/// `Zustand` ist deutlich größer als die anderen Fälle; das ist Absicht: eine
/// Antwort wird **einmal** je Änderung über die Grenze geschickt, und ein
/// Umweg über den Haufen (`Box`) spart dabei nichts, was man merken würde.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Antwort {
    /// Neuer Modellstand (nach jeder Änderung).
    Zustand { zustand: Zustand },
    /// Befehl ausgeführt, Modell unverändert.
    Ok,
    /// Befehl abgelehnt.
    Fehler { text: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leerer_zustand_ist_getrennt() {
        let z = Zustand::neu();
        assert_eq!(z.verbindung, Some(Verbindung::Getrennt));
        assert!(z.ich().is_none());
        assert!(!z.darf_raeume_verwalten());
    }

    #[test]
    fn sichtbarkeit_folgt_aus_der_rolle() {
        let raum = RoomId::aus_roh(1);
        let mitglied = MemberId::aus_roh(2);
        let mut z = Zustand::neu();
        z.verbindung = Some(Verbindung::Verbunden {
            server: "127.0.0.1:7443".into(),
            server_name: "Test".into(),
            ich: mitglied,
            name: "Ada".into(),
            rolle: Role::Mitglied,
        });
        z.aktueller_raum = Some(raum);
        assert!(z.ich().is_some());
        assert!(!z.darf_raeume_verwalten());

        z.verbindung = Some(Verbindung::Verbunden {
            server: "127.0.0.1:7443".into(),
            server_name: "Test".into(),
            ich: mitglied,
            name: "Ada".into(),
            rolle: Role::Moderator,
        });
        assert!(z.darf_raeume_verwalten());
        assert!(!z.darf_rollen_vergeben());
    }

    #[test]
    fn befehle_haben_stabile_namen() {
        let text = serde_json::to_string(&Befehl::Text {
            text: "hallo".into(),
        })
        .unwrap();
        assert!(text.contains("\"befehl\":\"text\""), "{text}");
        let zurueck: Befehl = serde_json::from_str(&text).unwrap();
        assert!(matches!(zurueck, Befehl::Text { .. }));
    }
}

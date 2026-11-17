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
pub use sprechwerk_protocol::inhalt::{Effekt, Linkvorschau};

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
        /// Eigener **öffentlicher** Schlüssel (Hex) — die Kennung, die auf
        /// einem anderen Gerät dieselbe Person ergibt. Sie ist öffentlich; sie
        /// zu zeigen ist der einzige Weg, den Umzug nachvollziehbar zu machen.
        #[serde(default)]
        kennung: String,
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

/// Was eine Nachricht zeigt (ADR 0012) — ein Summentyp, kein Text mit
/// Sonderwerten: die Oberfläche entscheidet nach `zustand`, nicht nach Inhalt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "zustand", rename_all = "snake_case")]
pub enum NachrichtInhalt {
    /// Entschlüsselter Text, wahlweise mit Effekt und Link-Vorschau.
    Text {
        text: String,
        /// Nur bekannte Effekte — ein unbekannter fällt hier weg.
        #[serde(default)]
        effekt: Option<Effekt>,
        /// Geprüft: `http(s)`, Längen begrenzt, Bild ist Base64.
        #[serde(default)]
        vorschau: Option<Linkvorschau>,
    },
    /// Eine Reaktion auf eine andere Nachricht. Die Oberfläche zeigt sie nicht
    /// als eigenen Beitrag, sondern zusammengefasst am Ziel (`reaktionen`).
    Reaktion {
        ziel: MessageId,
        zeichen: String,
        weg: bool,
    },
    /// Verschlüsselt, der Schlüssel fehlt noch (kommt von einem Mitglied).
    Wartet,
    /// Verändert, umgehängt oder beschädigt.
    Unlesbar,
    /// Eine Art, die diese Version nicht kennt.
    Neuer { art: String },
    /// Gelöscht.
    Geloescht,
}

/// Eine Nachricht im aktuellen Raum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nachricht {
    pub id: MessageId,
    pub verfasser: MemberId,
    pub verfasser_name: String,
    pub inhalt: NachrichtInhalt,
    /// Die Nachricht, wie der Server sie geschickt hat (Geheimtext). Bleibt im
    /// Rust-Teil — die Oberfläche bekommt nur `inhalt`. Daraus wird neu
    /// entschlüsselt, sobald ein Schlüssel ankommt.
    #[serde(skip)]
    pub roh: Option<sprechwerk_domain::Message>,
    /// Zeit in Millisekunden seit der Epoche.
    pub zeit: i64,
    pub geloescht: bool,
    /// Darf ich sie löschen? (eigene Nachricht oder Moderator)
    pub loeschbar: bool,
    /// Zusammengefasste Reaktionen auf diese Nachricht, in der Reihenfolge,
    /// in der das erste Zeichen kam.
    #[serde(default)]
    pub reaktionen: Vec<Reaktionen>,
}

/// Wie oft ein Zeichen an einer Nachricht steht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaktionen {
    pub zeichen: String,
    pub anzahl: usize,
    /// Bin ich dabei? Dann nimmt ein Klick die eigene zurück.
    pub ich: bool,
    /// Wer reagiert hat (für den Hinweis beim Darüberfahren).
    pub namen: Vec<String>,
}

/// Ein Effekt, der gerade abspielen soll.
///
/// `nr` zählt bei jedem Auftritt hoch: dieselbe Nachricht zweimal abgespielt
/// ist zweimal ein neuer Wert — die Oberfläche spielt ab, wenn `nr` sich
/// ändert, und braucht dafür keinen eigenen Zustand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effektauftritt {
    pub nachricht: MessageId,
    pub effekt: Effekt,
    pub nr: u64,
}

/// Link-Vorschau des Entwurfs im Eingabefeld (ADR 0012 §2: der Absender ruft
/// ab, nicht die Empfänger).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Linkentwurf {
    /// Kein Link im Entwurf.
    #[default]
    Keiner,
    /// Wird abgerufen.
    Laedt { url: String },
    /// Liegt vor und geht mit dem Text hinaus.
    Fertig { vorschau: Linkvorschau },
    /// Keine Vorschau für diesen Link (nicht abrufbar oder weggeklickt).
    Ohne { url: String },
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
    /// Fingerabdruck des Server-Zertifikats, mit dem diese Verbindung läuft.
    ///
    /// Steht im Modell, damit man ihn **sehen** und mit dem vergleichen kann,
    /// was der Betreiber ansagt (TOFU, ADR 0002 §3). Eine Anheftung, deren
    /// Wert niemand nachlesen kann, wäre keine Bestätigung.
    #[serde(default)]
    pub zertifikat: Option<String>,
    /// Verhängte Sperren (leer, bis sie einmal abgefragt wurden).
    #[serde(default)]
    pub sperren: Vec<Sperre>,
    /// Für welche verschlüsselten Räume der Raumschlüssel vorliegt (mit Epoche).
    ///
    /// Ohne Schlüssel hört man in einem verschlüsselten Raum **nichts** — der
    /// häufigste Grund für „ich höre niemanden". Deshalb steht der Zustand im
    /// Modell und wird angezeigt, statt nur in einer Notiz zu verschwinden.
    #[serde(default)]
    pub schluessel: Vec<Schluesselstand>,
    /// Liegt der Serverschlüssel zum Schreiben vor? (ADR 0012)
    #[serde(default)]
    pub textschluessel: Textschluessel,
    /// Effekt, der gerade abspielt (zuletzt angekommen oder wiederholt).
    #[serde(default)]
    pub effekt: Option<Effektauftritt>,
    /// Link-Vorschau des Entwurfs.
    #[serde(default)]
    pub entwurf: Linkentwurf,
    /// Umlaufzeit zum Server in Millisekunden (aus Ping/Pong gemessen).
    ///
    /// Sie wird **im Rust-Teil** gemessen, nicht im Fenster: nur dort sitzt die
    /// Uhr am selben Faden wie die Verbindung.
    #[serde(default)]
    pub latenz_ms: Option<u32>,
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

/// Stand des Serverschlüssels für die Anzeige.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Textschluessel {
    /// Nicht verbunden.
    #[default]
    Unbekannt,
    /// Schreiben und Lesen gehen.
    Bereit,
    /// Fehlt noch — ein Mitglied, das ihn hat, muss online sein.
    Wartet,
}

/// Wie viele Protokollzeilen ein Aufruf ohne Angabe liest.
fn fuenfzig() -> usize {
    50
}

/// Der Raumschlüssel eines verschlüsselten Raums.
///
/// Ohne ihn hört man in einem verschlüsselten Raum **nichts** — der häufigste
/// Grund für „ich höre niemanden". Deshalb steht der Zustand im Modell und
/// wird angezeigt, statt nur in einer Notiz zu verschwinden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schluesselstand {
    pub raum: RoomId,
    /// Epoche des Schlüssels (zählt bei jedem Verlassen hoch).
    pub epoche: u32,
}

/// Eine verhängte Sperre, wie sie das Fenster zeigt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sperre {
    /// Gesperrte Kennung (Hex), falls die Sperre an einer Kennung hängt.
    pub kennung: Option<String>,
    /// Gesperrte Adresse, falls die Sperre an einer Adresse hängt.
    pub adresse: Option<String>,
    pub grund: String,
    /// Zeitpunkt in Millisekunden seit der Epoche.
    pub zeit: i64,
    /// Wer sie verhängt hat.
    pub handelnder_name: String,
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
    /// Begründung der Handlung, falls es eine gibt (Sperre, Trennung).
    #[serde(default)]
    pub grund: Option<String>,
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
        #[serde(default)]
        effekt: Option<Effekt>,
    },
    /// Mit einem Zeichen reagieren — oder, wenn ich es schon gesetzt habe,
    /// zurücknehmen. Was davon, entscheidet der Rust-Teil am Modell.
    Reagieren {
        nachricht: MessageId,
        zeichen: String,
    },
    /// Den Effekt einer Nachricht noch einmal abspielen.
    EffektZeigen {
        nachricht: MessageId,
    },
    /// Der erste Link im Entwurf hat sich geändert (`None`: keiner mehr).
    Vorschau {
        #[serde(default)]
        url: Option<String>,
    },
    /// Die Vorschau des Entwurfs nicht mitschicken.
    VorschauVerwerfen,
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
    /// Umlaufzeit messen (schickt ein Lebenszeichen und stoppt die Zeit).
    Ping,
    /// Den Raumschlüssel erneut anfordern (nach einem verlorenen Paket).
    SchluesselAnfordern {
        raum: RoomId,
    },
    /// Verhängte Sperren auflisten (nur Administrator).
    Sperrliste,
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
            kennung: "ab".repeat(32),
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
            kennung: "ab".repeat(32),
        });
        assert!(z.darf_raeume_verwalten());
        assert!(!z.darf_rollen_vergeben());
    }

    #[test]
    fn befehle_haben_stabile_namen() {
        let text = serde_json::to_string(&Befehl::Text {
            text: "hallo".into(),
            effekt: None,
        })
        .unwrap();
        assert!(text.contains("\"befehl\":\"text\""), "{text}");
        let zurueck: Befehl = serde_json::from_str(&text).unwrap();
        assert!(matches!(zurueck, Befehl::Text { .. }));
    }
}

//! Die Nachrichten zwischen Client und Server.
//!
//! Aufbau: jede Richtung hat einen Aufzählungstyp; serde erzeugt daraus
//! JSON mit `typ` und Feldern. Die Namen sind stabil — sie sind die
//! Protokollsprache, nicht nur Serialisierung (**Festlegung SPEC §7**).

use serde::{Deserialize, Serialize};
use sprechwerk_domain::{Message, Role, RoomId};

/// Fehlercodes. Klein gehalten und stabil: ein Code, ein Satz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fehlercode {
    /// Anfrage nicht verstanden.
    UngueltigeAnfrage,
    /// Dafür fehlt das Recht.
    KeinRecht,
    /// Anmeldung abgelehnt (falsches Serverpasswort, gesperrt).
    AnmeldungAbgelehnt,
    /// Objekt gibt es nicht.
    Unbekannt,
    /// Eingabe unzulässig (Name, Länge).
    UngueltigeEingabe,
    /// Protokollversion passt nicht.
    Version,
    /// Später nochmal (Server beschäftigt).
    Spaeter,
}

impl Fehlercode {
    /// Stabiler Text für Protokoll und Anzeige.
    pub fn text(self) -> &'static str {
        match self {
            Fehlercode::UngueltigeAnfrage => "ungültige Anfrage",
            Fehlercode::KeinRecht => "dafür fehlt das Recht",
            Fehlercode::AnmeldungAbgelehnt => "Anmeldung abgelehnt",
            Fehlercode::Unbekannt => "nicht vorhanden",
            Fehlercode::UngueltigeEingabe => "ungültige Eingabe",
            Fehlercode::Version => "Protokollversion passt nicht",
            Fehlercode::Spaeter => "später nochmal",
        }
    }
}

/// Ein Raum, wie ihn der Client sieht (flach; der Baum steckt in `eltern`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raumansicht {
    pub id: RoomId,
    pub name: String,
    pub eltern: Option<RoomId>,
    pub reihenfolge: i32,
    /// Wie viele gerade drin sind.
    pub anwesend: usize,
}

/// Ein Mitglied, wie es der Client sieht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mitgliedsansicht {
    pub id: sprechwerk_domain::MemberId,
    pub name: String,
    pub rolle: Role,
    pub raum: Option<RoomId>,
    /// Spricht gerade (Sprachrahmen in den letzten 400 ms).
    pub spricht: bool,
    /// Vom Server stummgeschaltet (Mikrofon).
    #[serde(default)]
    pub stumm: bool,
    /// Vom Server taubgeschaltet (Wiedergabe).
    #[serde(default)]
    pub taub: bool,
}

/// Kopf eines Sprach-Datagramms.
///
/// ```text
/// [0] Version (1)
/// [1] Codec   (0 = Opus)
/// [2..3] Folge (u16, big-endian) — je Sprecher, läuft um
/// [4..]  Opus-Rahmen
/// ```
///
/// **Vier Byte, nicht acht.** Der Raum steht **nicht** im Kopf: der Server
/// weiß aus dem Sitzungszustand, in welchem Raum der Sprecher sitzt. Das spart
/// bei einem 20-ms-Opus-Rahmen (rund 80 Byte) fünf Prozent Nutzlast und macht
/// zugleich das Umleiten in fremde Räume unmöglich — ein Client kann nicht
/// behaupten, woanders zu sitzen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprachkopf {
    pub version: u8,
    pub codec: u8,
    pub folge: u16,
}

/// Länge des Sprachkopfs.
pub const SPRACHKOPF_LEN: usize = 4;

/// Codec Nummer für Opus.
pub const CODEC_OPUS: u8 = 0;

/// Höchstgröße eines Sprach-Datagramms. QUIC-Datagramme müssen unter der
/// Pfad-MTU bleiben (rund 1200 Byte); 60 ms Opus sind selbst bei 64 kbit/s
/// nur 480 Byte, also ist hier reichlich Luft — und eine harte Grenze gegen
/// Missbrauch.
pub const MAX_SPRACHRAHMEN: usize = 1200;

impl Sprachkopf {
    pub fn neu(folge: u16) -> Self {
        Self {
            version: 1,
            codec: CODEC_OPUS,
            folge,
        }
    }

    pub fn schreiben(&self, nutzlast: &[u8]) -> Vec<u8> {
        let mut aus = Vec::with_capacity(SPRACHKOPF_LEN + nutzlast.len());
        aus.push(self.version);
        aus.push(self.codec);
        aus.extend_from_slice(&self.folge.to_be_bytes());
        aus.extend_from_slice(nutzlast);
        aus
    }

    pub fn lesen(bytes: &[u8]) -> Option<(Self, &[u8])> {
        if bytes.len() < SPRACHKOPF_LEN || bytes.len() > MAX_SPRACHRAHMEN {
            return None;
        }
        let kopf = Self {
            version: bytes[0],
            codec: bytes[1],
            folge: u16::from_be_bytes([bytes[2], bytes[3]]),
        };
        Some((kopf, &bytes[SPRACHKOPF_LEN..]))
    }
}

/// Was der Client schickt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Erster Rahmen. `kennung` ist der öffentliche Schlüssel in Textform,
    /// `beweis` eine Signatur über `nonce` (Besitznachweis).
    Hallo {
        version: u16,
        /// Öffentlicher Schlüssel (Ed25519, 32 Byte, Hex).
        kennung: String,
        name: String,
        /// Signatur über die Zeichen des Nonce aus
        /// [`ServerFrame::Anforderung`] (Ed25519, 64 Byte, Hex).
        beweis: String,
        #[serde(default)]
        serverpasswort: Option<String>,
    },
    /// Raum betreten.
    Betreten { raum: RoomId },
    /// Raum verlassen.
    Verlassen,
    /// Text in den aktuellen Raum schreiben.
    Text { text: String },
    /// Nachricht löschen.
    NachrichtLoeschen {
        nachricht: sprechwerk_domain::MessageId,
    },
    /// Raum anlegen.
    RaumAnlegen {
        name: String,
        #[serde(default)]
        eltern: Option<RoomId>,
    },
    /// Raum umbenennen.
    RaumUmbenennen { raum: RoomId, name: String },
    /// Raum verschieben.
    RaumVerschieben {
        raum: RoomId,
        #[serde(default)]
        eltern: Option<RoomId>,
    },
    /// Raum löschen.
    RaumLoeschen { raum: RoomId },
    /// Mikrofon oder Wiedergabe eines Mitglieds serverseitig stummschalten.
    Stummschalten {
        mitglied: sprechwerk_domain::MemberId,
        /// `true` = Wiedergabe (taub), `false` = Mikrofon.
        #[serde(default)]
        taub: bool,
        an: bool,
    },
    /// Ein Mitglied in einen anderen Raum verschieben.
    Verschieben {
        mitglied: sprechwerk_domain::MemberId,
        raum: RoomId,
    },
    /// Ein Mitglied aus seinem Raum werfen (zurück in den Wurzelraum).
    AusRaumWerfen {
        mitglied: sprechwerk_domain::MemberId,
    },
    /// Ein Mitglied vom Server trennen (rauswerfen).
    Trennen {
        mitglied: sprechwerk_domain::MemberId,
        #[serde(default)]
        grund: String,
    },
    /// Ein Mitglied sperren (Kennung und, wenn bekannt, nichts weiter) und
    /// trennen.
    MitgliedSperren {
        mitglied: sprechwerk_domain::MemberId,
        #[serde(default)]
        grund: String,
    },
    /// Rolle setzen.
    RolleSetzen {
        mitglied: sprechwerk_domain::MemberId,
        rolle: Role,
    },
    /// Sperren (Kennung und/oder Adresse).
    Sperren {
        #[serde(default)]
        kennung: Option<String>,
        #[serde(default)]
        adresse: Option<String>,
        grund: String,
    },
    /// Prüfprotokoll lesen.
    Protokoll { ab: usize, anzahl: usize },
    /// Lebenszeichen.
    Ping,
}

/// Was der Server schickt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum ServerFrame {
    /// Erster Rahmen des Servers: eine Zufallszahl, die der Client signieren
    /// muss. Damit ist die Kennung ein **Besitznachweis** und kein
    /// weitergegebenes Token (**Festlegung SPEC §4**).
    Anforderung { version: u16, nonce: String },
    /// Antwort auf [`ClientFrame::Hallo`]: der ganze Anfangszustand.
    Willkommen {
        version: u16,
        server_name: String,
        /// Eigene Kennung.
        ich: sprechwerk_domain::MemberId,
        rolle: Role,
        raeume: Vec<Raumansicht>,
        mitglieder: Vec<Mitgliedsansicht>,
        /// Verlauf des Raums, in dem das Mitglied zuletzt war (falls).
        #[serde(default)]
        verlauf: Vec<Message>,
    },
    /// Bestätigung eines Raumwechsels samt Verlauf.
    Betreten {
        raum: RoomId,
        verlauf: Vec<Message>,
        anwesend: Vec<Mitgliedsansicht>,
    },
    /// Jemand hat einen Raum betreten oder verlassen.
    Anwesenheit {
        raum: RoomId,
        mitglied: Mitgliedsansicht,
        anwesend: bool,
    },
    /// Neue Nachricht in einem Raum, in dem der Empfänger ist.
    Text { nachricht: Message },
    /// Nachricht wurde gelöscht.
    NachrichtGeloescht {
        nachricht: sprechwerk_domain::MessageId,
        hart: bool,
    },
    /// Raumliste hat sich geändert (anlegen, umbenennen, verschieben, löschen).
    Raeume { raeume: Vec<Raumansicht> },
    /// Ein Mitglied hat sich geändert (Name, Rolle).
    MitgliedGeaendert { mitglied: Mitgliedsansicht },
    /// Prüfprotokoll.
    Protokoll {
        eintraege: Vec<sprechwerk_domain::AuditEntry>,
    },
    /// Antwort auf ein Kommando ohne eigene Nutzlast.
    Ok,
    /// Fehler.
    Fehler { code: Fehlercode, text: String },
    /// Antwort auf [`ClientFrame::Ping`].
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_frame_hat_stabilen_text() {
        let f = ClientFrame::Text {
            text: "hallo".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"typ\":\"text\""), "{json}");
        let zurueck: ClientFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(zurueck, f);
    }

    #[test]
    fn server_frame_ist_umkehrbar() {
        let f = ServerFrame::Fehler {
            code: Fehlercode::KeinRecht,
            text: "dafür fehlt das Recht".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("kein_recht"), "{json}");
        let zurueck: ServerFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(zurueck, f);
    }

    #[test]
    fn sprachkopf_ist_vier_byte_und_umkehrbar() {
        let kopf = Sprachkopf::neu(7);
        let bytes = kopf.schreiben(&[1, 2, 3]);
        assert_eq!(bytes.len(), 4 + 3);
        let (zurueck, nutzlast) = Sprachkopf::lesen(&bytes).unwrap();
        assert_eq!(zurueck, kopf);
        assert_eq!(nutzlast, &[1, 2, 3]);
        // Aufschlag bei einem typischen Opus-Rahmen: 4 von 84 Byte.
        let typisch = kopf.schreiben(&[0u8; 80]);
        assert!(typisch.len() * 100 / 84 <= 101);
    }

    #[test]
    fn zu_kurzer_sprachkopf_ergibt_none() {
        assert!(Sprachkopf::lesen(&[1, 0, 0]).is_none());
    }
}

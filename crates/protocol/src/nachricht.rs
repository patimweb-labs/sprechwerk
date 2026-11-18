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
    pub reihenfolge: i32,
    /// Wie viele gerade drin sind.
    pub anwesend: usize,
    /// Sprache wird in diesem Raum Ende zu Ende verschlüsselt.
    #[serde(default)]
    pub verschluesselt: bool,
    /// Seit wann (ms). Alle Clients bilden daraus dieselbe **Epoche**.
    #[serde(default)]
    pub verschluesselt_seit: i64,
}

/// Ein Mitglied, wie es der Client sieht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mitgliedsansicht {
    pub id: sprechwerk_domain::MemberId,
    pub name: String,
    /// Öffentlicher Anmeldeschlüssel (Hex). Öffentlich — er ist die Identität
    /// und wird für den Schlüsselaustausch der Ende-zu-Ende-Verschlüsselung
    /// gebraucht, damit der Server keinen Schlüssel unterschieben kann.
    #[serde(default)]
    pub kennung: String,
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
    /// Seit wann in diesem Raum (ms). Bestimmt den **Schlüsselwart** der
    /// Ende-zu-Ende-Verschlüsselung: der am längsten Anwesende. Damit wechselt
    /// der Wart nicht, wenn jemand dazukommt — sonst könnten zwei Mitglieder
    /// verschiedene Schlüssel derselben Epoche erzeugen und einander nicht mehr
    /// hören.
    #[serde(default)]
    pub raum_seit: i64,
}

/// Welche Serverschlüssel es gibt (ADR 0012 §1). Der Server kennt nur die
/// Kennungen, nie die Schlüssel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schluesselstand {
    /// Mit diesem wird geschrieben. `None`: es gibt noch keinen (neuer Server)
    /// oder er wurde nach einer Sperre verworfen — der nächste Client erzeugt
    /// einen.
    #[serde(default)]
    pub aktuell: Option<String>,
    /// Alle, mit denen je geschrieben wurde (ältester zuerst) — zum Lesen.
    #[serde(default)]
    pub alle: Vec<String>,
}

/// Kopf eines Sprach-Datagramms.
///
/// ```text
/// [0]     Version (2)
/// [1]     Codec   (0 = Opus)
/// [2..3]  Folge   (u16, big-endian) — je Sprecher, läuft um
/// [4..5]  von     (u16, big-endian) — Sprecher, **vom Server gesetzt**
/// [6..]   Opus-Rahmen
/// ```
///
/// **Sechs Byte.** Der Client schickt `von = 0`; der Server trägt die Nummer
/// des Sprechers ein. Warum überhaupt: der Empfänger braucht für **jeden
/// Sprecher einen eigenen Dekodierer und Jitter-Puffer**. Ohne diese Nummer
/// würde ein einziger Dekodierer Rahmen verschiedener Stimmen vermischen — das
/// klingt wie Rauschen.
///
/// Der **Raum** steht weiterhin nicht im Kopf: der Server weiß aus dem
/// Sitzungszustand, wo der Sprecher sitzt, und ein Client kann nicht
/// behaupten, woanders zu sitzen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprachkopf {
    pub version: u8,
    pub codec: u8,
    pub folge: u16,
    /// Sprecher-Nummer, vom Server gesetzt (0 = noch nicht gesetzt).
    pub von: u16,
}

/// Länge des Sprachkopfs.
pub const SPRACHKOPF_LEN: usize = 6;

/// Wie viele Protokollzeilen ein Aufruf ohne Angabe liest.
fn fuenfzig() -> usize {
    50
}

/// Aktuelle Version des Sprachkopfs.
pub const SPRACHKOPF_VERSION: u8 = 2;

/// Codec Nummer für Opus.
pub const CODEC_OPUS: u8 = 0;

/// Nutzlast ist Ende-zu-Ende verschlüsselt (ADR 0006).
///
/// Der **Kopf bleibt lesbar**: der Server muss weiterhin verteilen und das
/// Rederecht prüfen können. Nur der Opus-Rahmen dahinter ist Geheimtext.
pub const CODEC_VERSCHLUESSELT: u8 = 3;

/// Höchstgröße eines Sprach-Datagramms. QUIC-Datagramme müssen unter der
/// Pfad-MTU bleiben (rund 1200 Byte); 60 ms Opus sind selbst bei 64 kbit/s
/// nur 480 Byte, also ist hier reichlich Luft — und eine harte Grenze gegen
/// Missbrauch.
pub const MAX_SPRACHRAHMEN: usize = 1200;

impl Sprachkopf {
    pub fn neu(folge: u16) -> Self {
        Self {
            version: SPRACHKOPF_VERSION,
            codec: CODEC_OPUS,
            folge,
            von: 0,
        }
    }

    /// Setzt die Sprecher-Nummer (macht der Server beim Weiterleiten).
    pub fn mit_von(mut self, von: u16) -> Self {
        self.von = von;
        self
    }

    pub fn schreiben(&self, nutzlast: &[u8]) -> Vec<u8> {
        let mut aus = Vec::with_capacity(SPRACHKOPF_LEN + nutzlast.len());
        aus.push(self.version);
        aus.push(self.codec);
        aus.extend_from_slice(&self.folge.to_be_bytes());
        aus.extend_from_slice(&self.von.to_be_bytes());
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
            von: u16::from_be_bytes([bytes[4], bytes[5]]),
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
    /// Nachricht in den aktuellen Raum schreiben.
    ///
    /// `text` ist **Geheimtext** (`geheimtext::zerlegen`), verschlüsselt mit
    /// dem aktuellen Serverschlüssel. Klartext nimmt der Server nicht an
    /// (ADR 0012).
    ///
    /// `anhaenge` nennt die Dateien, die der Absender vorher auf einem
    /// Datenstrom hochgeladen hat ([`crate::anhang`]). Der Server hängt sie an
    /// die Nachricht — löscht man die Nachricht, löscht er die Dateien.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        anhaenge: Vec<String>,
    },
    /// Einen Serverschlüssel festlegen, wenn es keinen aktuellen gibt.
    ///
    /// Der Server nimmt den **ersten** an; kommt ein zweiter, antwortet er mit
    /// dem Stand, und der Client verwirft seinen.
    ServerschluesselAnlegen { id: String },
    /// Welche Serverschlüssel dieser Client hält.
    ServerschluesselVorhanden { ids: Vec<String> },
    /// Ein Serverschlüssel für **ein** anderes Mitglied, verpackt und
    /// unterschrieben wie ein [`ClientFrame::Schluesselpaket`]. Der Server
    /// leitet es unverändert weiter.
    ServerschluesselPaket {
        fuer: sprechwerk_domain::MemberId,
        id: String,
        daten: String,
        unterschrift: String,
    },
    /// Nachricht löschen.
    NachrichtLoeschen {
        nachricht: sprechwerk_domain::MessageId,
    },
    /// Raum anlegen (flache Liste, keine Unterräume).
    RaumAnlegen { name: String },
    /// Raum umbenennen.
    RaumUmbenennen { raum: RoomId, name: String },
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
    /// Ende-zu-Ende-Verschlüsselung eines Raums ein-/ausschalten.
    RaumVerschluesseln {
        raum: sprechwerk_domain::RoomId,
        an: bool,
    },
    /// Ein Schlüsselpaket für **einen** anderen Anwesenden.
    ///
    /// Der Server leitet es unverändert weiter — er kann es nicht lesen (der
    /// Raumschlüssel liegt darin verschlüsselt für genau diesen Empfänger).
    /// `unterschrift` ist eine Ed25519-Signatur des Absenders über
    /// `epoche || raum || daten`, damit der Server kein Paket unterschieben
    /// kann.
    Schluesselpaket {
        raum: sprechwerk_domain::RoomId,
        /// Zählt bei jedem Verlassen hoch.
        epoche: u32,
        /// Kennung des Empfängers.
        fuer: sprechwerk_domain::MemberId,
        /// Verschlüsselter Raumschlüssel (Hex).
        daten: String,
        /// Signatur des Absenders (Hex).
        unterschrift: String,
    },
    /// Hebt eine Sperre auf (Kennung und/oder Adresse).
    ///
    /// Ohne diesen Befehl käme man an eine verhängte Sperre nur über die
    /// Datenbank — eine Sperre, die man nicht zurücknehmen kann, ist im Betrieb
    /// eine Falle.
    Entsperren {
        #[serde(default)]
        kennung: Option<String>,
        #[serde(default)]
        adresse: Option<String>,
    },
    /// Bittet den Schlüsselwart um den Raumschlüssel.
    ///
    /// Nötig, wenn ein Schlüsselpaket **verloren** ging: der Wart hat den
    /// Schlüssel der Epoche schon und würde von selbst nichts mehr schicken.
    /// Der Server leitet die Bitte an die anderen im Raum weiter — er kann sie
    /// nicht selbst erfüllen, er kennt den Schlüssel nicht.
    SchluesselAnfrage { raum: sprechwerk_domain::RoomId },
    /// Verhängte Sperren auflisten (nur Administrator).
    Sperrliste,
    /// Betriebszahlen abfragen (Sprachrahmen, Verworfene, Verbundene).
    ///
    /// Jeder darf das: die Zahlen sagen nichts über Personen, aber viel über
    /// den Server. „0 verworfen" ist die ehrlichste Auskunft, die ein
    /// Sprachserver geben kann.
    Zaehler,
    /// Prüfprotokoll lesen.
    ///
    /// `von_hinten` liefert die **neuesten** Einträge (neueste zuerst). Ohne
    /// das Flag zählt `ab` vom Anfang — das ist für einen Mitschnitt richtig,
    /// für die Ansicht im Fenster aber falsch: dort will man sehen, was gerade
    /// geschehen ist, nicht was im ersten Jahr passierte.
    Protokoll {
        #[serde(default)]
        ab: usize,
        #[serde(default = "fuenfzig")]
        anzahl: usize,
        #[serde(default)]
        von_hinten: bool,
    },
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
        /// Raum, in dem das Mitglied landet (der Standardraum).
        #[serde(default)]
        raum: Option<RoomId>,
        /// Dessen Verlauf.
        #[serde(default)]
        verlauf: Vec<Message>,
        /// Serverschlüssel (Kennungen).
        #[serde(default)]
        serverschluessel: Schluesselstand,
    },
    /// Der Stand der Serverschlüssel hat sich geändert (angelegt, verworfen).
    Serverschluessel { stand: Schluesselstand },
    /// Bitte an einen Halter: `fuer` fehlt der Schlüssel `id`.
    ///
    /// Der Server **vermittelt** nur — er weiß, wer verbunden ist und welche
    /// Kennungen er hält. Der Halter verpackt den Schlüssel für `kennung`
    /// (den öffentlichen Schlüssel des Empfängers) und unterschreibt.
    ServerschluesselAnfrage {
        fuer: sprechwerk_domain::MemberId,
        kennung: String,
        id: String,
    },
    /// Ein Serverschlüssel, weitergeleitet vom Absender.
    ServerschluesselPaket {
        von: sprechwerk_domain::MemberId,
        /// Öffentlicher Schlüssel des Absenders — für die Unterschrift und den
        /// Umschlag. Der Empfänger kennt den Absender sonst vielleicht nicht
        /// (er sitzt in einem anderen Raum).
        von_kennung: String,
        id: String,
        daten: String,
        unterschrift: String,
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
    /// Ein Schlüsselpaket, weitergeleitet vom Absender.
    Schluesselpaket {
        raum: sprechwerk_domain::RoomId,
        epoche: u32,
        /// Wer es geschickt hat (und damit, wessen Unterschrift zu prüfen ist).
        von: sprechwerk_domain::MemberId,
        daten: String,
        unterschrift: String,
    },
    /// Jemand bittet um den Raumschlüssel (weitergeleitet vom Server).
    ///
    /// Der Wart antwortet mit [`ClientFrame::Schluesselpaket`]-Paketen an die
    /// Anwesenden; der Server sieht dabei nur Geheimtext.
    SchluesselAnfrage {
        raum: sprechwerk_domain::RoomId,
        /// Wer gefragt hat.
        von: sprechwerk_domain::MemberId,
    },
    /// Verhängte Sperren.
    Sperrliste {
        sperren: Vec<sprechwerk_domain::Ban>,
    },
    /// Betriebszahlen des Servers.
    Zaehler {
        /// Sprachrahmen, die ankamen.
        sprachrahmen: u64,
        /// Wie oft ein Rahmen an einen Empfänger ging.
        verteilt: u64,
        /// Rahmen, die nicht hinausgingen (Sendepuffer voll).
        verloren: u64,
        /// Rahmen, die der Server abgelehnt hat (kein Recht, kein Raum, kaputt).
        abgelehnt: u64,
        /// Gerade verbundene Mitglieder.
        verbundene: usize,
    },
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
            anhaenge: Vec::new(),
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
    fn sprachkopf_ist_sechs_byte_und_umkehrbar() {
        let kopf = Sprachkopf::neu(7);
        let bytes = kopf.schreiben(&[1, 2, 3]);
        assert_eq!(bytes.len(), SPRACHKOPF_LEN + 3);
        assert_eq!(SPRACHKOPF_LEN, 6, "Version, Codec, Folge, Sprecher");
        let (zurueck, nutzlast) = Sprachkopf::lesen(&bytes).unwrap();
        assert_eq!(zurueck, kopf);
        assert_eq!(nutzlast, &[1, 2, 3]);
        // Sprecher-Nummer setzt der Server — sie muss mit ankommen.
        let gestempelt = kopf.mit_von(7).schreiben(&[1, 2]);
        assert_eq!(Sprachkopf::lesen(&gestempelt).unwrap().0.von, 7);
        // Aufschlag bei einem typischen Opus-Rahmen: 6 von 86 Byte.
        let typisch = kopf.schreiben(&[0u8; 80]);
        assert!(
            typisch.len() * 100 / 86 <= 101,
            "Kopf muss unter 10 % Aufschlag bleiben"
        );
    }

    #[test]
    fn zu_kurzer_sprachkopf_ergibt_none() {
        assert!(Sprachkopf::lesen(&[1, 0, 0]).is_none());
    }
    /// Jeder Rahmen, den der Server schicken kann, muss beim Client
    /// ankommen — und umgekehrt.
    ///
    /// Diese Liste ist absichtlich vollständig und gezählt: kommt eine
    /// Nachrichtenart dazu und wird hier nicht ergänzt, schlägt der Test fehl.
    /// Damit gibt es keinen „toten" Rahmen, den der Server sendet und den kein
    /// Client kennt (die Fehlerklasse „weißer Bildschirm bei unbekannter
    /// Nachricht").
    #[test]
    fn alle_serverrahmen_sind_umkehrbar() {
        let ich = sprechwerk_domain::MemberId::neu();
        let raum = sprechwerk_domain::RoomId::neu();
        let nachricht = sprechwerk_domain::MessageId::neu();
        let mitglied = Mitgliedsansicht {
            id: ich,
            name: "Ada".into(),
            kennung: "ab".repeat(32),
            rolle: Role::Administrator,
            raum: Some(raum),
            spricht: false,
            stumm: false,
            taub: false,
            raum_seit: 0,
        };
        let rahmen = vec![
            ServerFrame::Anforderung {
                version: 1,
                nonce: "00".into(),
            },
            ServerFrame::Willkommen {
                version: 1,
                server_name: "Testwerk".into(),
                ich,
                rolle: Role::Mitglied,
                raeume: Vec::new(),
                mitglieder: Vec::new(),
                raum: None,
                verlauf: Vec::new(),
                serverschluessel: Schluesselstand {
                    aktuell: Some("ab".repeat(16)),
                    alle: vec!["cd".repeat(16), "ab".repeat(16)],
                },
            },
            ServerFrame::Serverschluessel {
                stand: Schluesselstand::default(),
            },
            ServerFrame::ServerschluesselAnfrage {
                fuer: ich,
                kennung: "ab".repeat(32),
                id: "ab".repeat(16),
            },
            ServerFrame::ServerschluesselPaket {
                von: ich,
                von_kennung: "ab".repeat(32),
                id: "ab".repeat(16),
                daten: "00".into(),
                unterschrift: "11".into(),
            },
            ServerFrame::Betreten {
                raum,
                verlauf: Vec::new(),
                anwesend: Vec::new(),
            },
            ServerFrame::Anwesenheit {
                raum,
                mitglied: mitglied.clone(),
                anwesend: true,
            },
            ServerFrame::Text {
                nachricht: Message {
                    id: nachricht,
                    raum,
                    verfasser: ich,
                    verfasser_name: "Ada".into(),
                    text: "hallo".into(),
                    zeit: 1,
                    geloescht: false,
                    anhaenge: Vec::new(),
                },
            },
            ServerFrame::NachrichtGeloescht {
                nachricht,
                hart: true,
            },
            ServerFrame::Raeume { raeume: Vec::new() },
            ServerFrame::MitgliedGeaendert {
                mitglied: mitglied.clone(),
            },
            ServerFrame::Schluesselpaket {
                raum,
                epoche: 3,
                von: ich,
                daten: "00".into(),
                unterschrift: "11".into(),
            },
            ServerFrame::SchluesselAnfrage { raum, von: ich },
            ServerFrame::Sperrliste {
                sperren: vec![sprechwerk_domain::Ban {
                    schluessel: Some("ab".repeat(32)),
                    adresse: None,
                    grund: "Spam".into(),
                    zeit: 1_765_000_000_000,
                    handelnder_name: "Ada".into(),
                }],
            },
            ServerFrame::Zaehler {
                sprachrahmen: 800,
                verteilt: 5596,
                verloren: 0,
                abgelehnt: 3,
                verbundene: 4,
            },
            ServerFrame::Protokoll {
                eintraege: Vec::new(),
            },
            ServerFrame::Ok,
            ServerFrame::Fehler {
                code: Fehlercode::KeinRecht,
                text: "dafür fehlt das Recht".into(),
            },
            ServerFrame::Pong,
            // Raumansicht und Anwesenheit sind die einzigen Rahmen mit
            // `verschluesselt`/`verschluesselt_seit` — einmal mit Werten.
            ServerFrame::Raeume {
                raeume: vec![Raumansicht {
                    id: raum,
                    name: "Vorstand".into(),
                    reihenfolge: 3,
                    anwesend: 2,
                    verschluesselt: true,
                    verschluesselt_seit: 1_765_000_000_000,
                }],
            },
        ];
        assert_eq!(rahmen.len(), 20, "ServerFrame-Arten: Liste ergänzen");

        for f in &rahmen {
            let json = serde_json::to_string(f).expect("serialisieren");
            let zurueck: ServerFrame = serde_json::from_str(&json)
                .unwrap_or_else(|_| panic!("Rahmen lässt sich nicht zurücklesen: {json}"));
            assert_eq!(&zurueck, f, "Rahmen verliert beim Umweg Informationen");
        }
    }

    /// Fremdes und kaputtes JSON ist ein Fehler, kein Absturz.
    ///
    /// Die Eingabe kommt vom Netz: hier darf nichts panisch werden, und ein
    /// unbekannter `typ` darf nicht als „irgendein Rahmen" durchgehen.
    ///
    /// **Unbekannte Felder** werden dagegen bewusst hingenommen (Serde-Vorgabe):
    /// ein neuerer Client darf ein Feld mitschicken, das dieser Server noch
    /// nicht kennt, ohne dass die Verbindung scheitert. Fehlende **pflicht**-
    /// felder sind dagegen ein Fehler — sonst würde ein Tippfehler still ein
    /// anderes Ziel treffen.
    #[test]
    fn fremdes_json_wird_abgelehnt() {
        let kaputt = [
            "",
            "null",
            "{}",
            r#"{"typ":"gibtesnicht"}"#,
            r#"{"typ":"text"}"#,
            r#"{"typ":"text","text":42}"#,
            r#"{"typ":"betreten","raum":"keine-kennung"}"#,
            r#"[1,2,3]"#,
        ];
        for text in kaputt {
            assert!(
                serde_json::from_str::<ClientFrame>(text).is_err(),
                "kaputtes JSON wurde angenommen: {text}"
            );
        }

        // Zusatzfelder sind erlaubt (Vorwärtsverträglichkeit).
        let mit_extra: ClientFrame =
            serde_json::from_str(r#"{"typ":"ping","unerwartet":1}"#).expect("Zusatzfeld");
        assert_eq!(mit_extra, ClientFrame::Ping);
    }

    /// Ein Rahmen über der Grenze wird abgewiesen, bevor er Speicher kostet.
    /// Die Nachforderung des Schlüssels ist ein normaler Rahmen.
    #[test]
    fn schluesselanfrage_ist_umkehrbar() {
        let f = ClientFrame::SchluesselAnfrage {
            raum: sprechwerk_domain::RoomId::neu(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("schluessel_anfrage"), "{json}");
        assert_eq!(serde_json::from_str::<ClientFrame>(&json).unwrap(), f);
    }

    #[test]
    fn zu_grosse_nutzlast_wird_abgelehnt() {
        let zu_gross = ClientFrame::Text {
            text: "x".repeat(crate::frame::MAX_RAHMEN + 1),
            anhaenge: Vec::new(),
        };
        assert!(crate::frame::schreibe_rahmen(&zu_gross).is_err());
    }

    /// Eine unbekannte Kennung ist ein Fehler, kein Zufallswert: sonst könnte
    /// ein Client mit einer erfundenen Kennung ein fremdes Mitglied treffen.
    #[test]
    fn kennungen_sind_streng() {
        assert!(serde_json::from_str::<sprechwerk_domain::MemberId>(r#""mitglied_zzzz""#).is_err());
        assert!(serde_json::from_str::<sprechwerk_domain::MemberId>(r#""""#).is_err());
    }
}

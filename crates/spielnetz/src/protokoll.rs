//! Was zwischen Client und Server über die Spielnetz-Verbindung geht.
//!
//! # Zwei Ebenen
//!
//! * **Steuerung** — JSON-Rahmen mit Längenpräfix, geschrieben mit
//!   [`sprechwerk_protocol::schreibe_rahmen`] und gelesen mit
//!   [`sprechwerk_protocol::lese_rahmen`]. Dieselbe Rahmung wie die Sitzung
//!   (`SPEC §7`): ein Protokoll, das man mit `tcpdump` und einem Texteditor
//!   lesen kann. Nur eben auf einer **eigenen** QUIC-Verbindung
//!   (ALPN [`ALPN`]) — Spielverkehr teilt sich weder Steuerstrom noch
//!   Staukontrolle mit der Sprache (ADR 0016 §2).
//! * **Daten** — je TCP-Strom ein [`StromKopf`] als erster Rahmen, je
//!   UDP-Datagramm ein [`Datagrammkopf`] aus 8 Byte. Der Inhalt beider Ebenen
//!   ist Geheimtext (siehe [`crate::krypto`]); der Server sieht nur die Köpfe —
//!   er muss wissen, wohin er weiterleitet.
//!
//! # Der Raum steht nicht im Kopf
//!
//! Wie beim Sprachkopf (AGENTS.md §9) steht auch hier kein Raum: welcher Raum
//! zu einer Kennung gehört, weiß der Server aus dem Sitzungszustand. Ein Client
//! kann sich also nicht in eine fremde Freigabe hineinschreiben.

use serde::{Deserialize, Serialize};
use sprechwerk_protocol::frame::RahmenFehler;
use sprechwerk_protocol::{lese_rahmen, schreibe_rahmen};

/// ALPN der eigenen Verbindung. Die Sitzung selbst bleibt `sprechwerk/1`.
pub const ALPN: &[u8] = b"sprechwerk-spielnetz/1";

/// Version der Spielnetz-Sprache. Wird im [`ClientRahmen::Hallo`] und in jedem
/// Datagrammkopf geprüft.
pub const VERSION: u16 = 1;

/// Nummer einer Freigabe, vom Server vergeben (fortlaufend ab 1).
pub type AngebotId = u32;

/// Kanalnummer eines UDP-Paares, **je Angebot** vergeben.
///
/// Der Gastgeber vergibt sie je (Teilnehmer, dessen lokaler Kanal): ein Spiel
/// mit mehreren Sockets bekommt für jeden eine eigene Nummer. Alle Kanäle
/// eines Paares teilen sich Siegel und Zähler; die Nummer steht nur im
/// Datagrammkopf und geht nicht in die Schlüsselableitung ein.
pub type Kanal = u16;

/// Größte UDP-Nutzlast, die getunnelt wird (ADR 0016 §7).
///
/// Größere Datagramme werden verworfen und gezählt — nicht zerteilt. Ein
/// Spiel, das sie schickt, verliert sie; das ist der Preis der Vorgabe
/// „QUIC-Datagramm minus Kopf, Sitzung, Zähler und Prüfsumme“: 1100 Byte
/// Nutzlast, dazu 8 (Kopf) + 8 (Sitzung) + 8 (Zähler) + 16 (Prüfsumme) =
/// 1140 Byte, also unter den 1150, die der QUIC-Datagrammrahmen hergibt.
pub const MAX_UDP_NUTZLAST: usize = 1100;

/// Art des freigegebenen Ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Art {
    Tcp,
    Udp,
}

/// Ein Teilnehmer einer Freigabe.
///
/// Kennung **und** Name: die Kennung braucht der Gastgeber zum Entfernen und
/// beide Seiten zum Rechnen (ADR 0016 §5), den Namen die Oberfläche zum
/// Anzeigen. Ihn erst im Fenster nachzuschlagen ginge nicht — die Spielnetz-
/// Verbindung kennt die Mitgliederliste der Sitzung nicht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Teilnehmer {
    /// Kennung (Hex) — öffentlich, wie beim Gastgeber.
    pub kennung: String,
    /// Anzeigename, wie er in der Vermittlung stand.
    pub name: String,
}

/// Eine Freigabe im Raum — was alle sehen, die im selben Raum sind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Angebot {
    pub id: AngebotId,
    pub name: String,
    pub port: u16,
    pub art: Art,
    /// Kennung des Gastgebers (Hex). Öffentlich — daraus rechnet jeder
    /// Teilnehmer den Austauschschlüssel aus (ADR 0016 §5).
    pub gastgeber: String,
    pub gastgeber_name: String,
    /// Teilnehmer, nach Kennung sortiert.
    #[serde(default)]
    pub teilnehmer: Vec<Teilnehmer>,
}

/// Grenzen, die der Server dem Client beim Anmelden nennt (ADR 0016 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grenzen {
    /// Freigaben je Mitglied.
    pub freigaben_je_mitglied: u8,
    /// Teilnehmer je Freigabe.
    pub teilnehmer_je_freigabe: u8,
    /// Gleichzeitige TCP-Ströme je Teilnehmer.
    pub stroeme_je_teilnehmer: u8,
    /// Durchsatz je Freigabe in kbit/s; `0` = unbegrenzt.
    pub kbit_je_freigabe: u32,
}

impl Default for Grenzen {
    fn default() -> Self {
        // Die Vorgaben aus ADR 0016 §7 — eine Stelle, nicht zwei.
        Self {
            freigaben_je_mitglied: 2,
            teilnehmer_je_freigabe: 16,
            stroeme_je_teilnehmer: 32,
            kbit_je_freigabe: 20_000,
        }
    }
}

/// Rahmen vom Client zum Server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum ClientRahmen {
    /// Erster Rahmen: Version, Kennung und der Beweis über die Nonce aus
    /// [`ServerRahmen::Anforderung`].
    Hallo {
        version: u16,
        kennung: String,
        beweis: String,
    },
    /// Port freigeben.
    Anbieten { name: String, port: u16, art: Art },
    /// Freigabe zurücknehmen (nur der Gastgeber).
    Zurueckziehen { angebot: AngebotId },
    /// Einer Freigabe beitreten.
    Beitreten { angebot: AngebotId },
    /// Eine Freigabe verlassen, der man beigetreten ist.
    Verlassen { angebot: AngebotId },
    /// Einen Teilnehmer hinauswerfen (nur der Gastgeber).
    Entfernen { angebot: AngebotId, kennung: String },
}

/// Rahmen vom Server zum Client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum ServerRahmen {
    /// Vor dem [`ClientRahmen::Hallo`]: die Nonce für den Beweis.
    Anforderung { version: u16, nonce: String },
    /// Anmeldung angenommen.
    Willkommen { grenzen: Grenzen },
    /// Alle Angebote im aktuellen Raum des Empfängers (vollständige Liste,
    /// ersetzt die vorige).
    Angebote { liste: Vec<Angebot> },
    /// Die eigene Freigabe steht; `angebot` ist ihre Nummer.
    Angeboten { angebot: AngebotId },
    /// Beitritt angenommen.
    Beigetreten { angebot: AngebotId },
    /// An den Gastgeber: Kanal `kanal` gehört ab jetzt zu Teilnehmer `von`.
    ///
    /// Je (Teilnehmer, dessen lokaler Kanal) einer — ein Teilnehmer kann
    /// mehrere Sockets offen haben und bekommt für jeden einen eigenen Rahmen.
    Kanal {
        angebot: AngebotId,
        kanal: Kanal,
        von: String,
    },
    /// Diese Freigabe ist vorbei. Der Grund sagt, warum.
    Beendet { angebot: AngebotId, grund: Grund },
    /// Die letzte Anfrage war nicht in Ordnung.
    Fehler { code: Fehler, text: String },
}

/// Warum eine Freigabe endet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grund {
    /// Der Gastgeber hat sie zurückgezogen.
    Zurueckgezogen,
    /// Der Gastgeber hat diesen Teilnehmer entfernt.
    Entfernt,
    /// Der Teilnehmer ist selbst gegangen.
    Verlassen,
    /// Der Gastgeber oder der Teilnehmer hat den Raum gewechselt.
    RaumVerlassen,
    /// Die Verbindung des Gastgebers ist weg.
    GastgeberWeg,
    /// Das Modul wurde abgeschaltet.
    Abgeschaltet,
}

impl Grund {
    /// Stabiler Text für Anzeige und Protokoll.
    pub fn text(self) -> &'static str {
        match self {
            Grund::Zurueckgezogen => "Freigabe zurückgezogen",
            Grund::Entfernt => "vom Gastgeber entfernt",
            Grund::Verlassen => "Freigabe verlassen",
            Grund::RaumVerlassen => "Raum verlassen",
            Grund::GastgeberWeg => "Gastgeber ist weg",
            Grund::Abgeschaltet => "Spielnetz abgeschaltet",
        }
    }
}

/// Fehlercodes. Klein gehalten und stabil: ein Code, ein Satz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fehler {
    /// Protokollversion passt nicht.
    Version,
    /// Beweis falsch, Kennung gesperrt.
    AnmeldungAbgelehnt,
    /// Erst [`ClientRahmen::Hallo`] schicken.
    NichtAngemeldet,
    /// Dafür fehlt das Recht (`Capability::Spielnetz`, ADR 0016 §8).
    KeinRecht,
    /// Ohne Raum keine Freigabe.
    KeinRaum,
    /// Grenze aus [`Grenzen::freigaben_je_mitglied`] erreicht.
    ZuVieleFreigaben,
    /// Grenze aus [`Grenzen::teilnehmer_je_freigabe`] erreicht.
    ZuVieleTeilnehmer,
    /// Grenze aus [`Grenzen::stroeme_je_teilnehmer`] erreicht.
    ZuVieleStroeme,
    /// Die Nummer gehört zu keiner Freigabe (mehr).
    UnbekanntesAngebot,
    /// Die Freigabe gehört zu einem anderen Raum.
    AndererRaum,
    /// Der Gastgeber kann seiner eigenen Freigabe nicht beitreten.
    EigenesAngebot,
    /// Nur der Gastgeber darf das.
    NichtGastgeber,
    /// Anfrage ergibt keinen Sinn (Port 0, Name zu lang, …).
    UngueltigeAnfrage,
}

impl Fehler {
    /// Stabiler Text für Anzeige und Protokoll.
    pub fn text(self) -> &'static str {
        match self {
            Fehler::Version => "Protokollversion passt nicht",
            Fehler::AnmeldungAbgelehnt => "Anmeldung abgelehnt",
            Fehler::NichtAngemeldet => "nicht angemeldet",
            Fehler::KeinRecht => "dafür fehlt das Recht",
            Fehler::KeinRaum => "ohne Raum keine Freigabe",
            Fehler::ZuVieleFreigaben => "zu viele Freigaben",
            Fehler::ZuVieleTeilnehmer => "zu viele Teilnehmer",
            Fehler::ZuVieleStroeme => "zu viele Ströme",
            Fehler::UnbekanntesAngebot => "unbekannte Freigabe",
            Fehler::AndererRaum => "Freigabe gehört zu einem anderen Raum",
            Fehler::EigenesAngebot => "das ist die eigene Freigabe",
            Fehler::NichtGastgeber => "nur der Gastgeber darf das",
            Fehler::UngueltigeAnfrage => "ungültige Anfrage",
        }
    }
}

/// Erster Rahmen auf einem getunnelten **TCP**-Strom.
///
/// Teilnehmer → Server: `von = None`, `strom = 0` (der Teilnehmer hat je
/// Freigabe nur einen Strom je Verbindung, die Nummer vergibt der Server).
/// Server → Gastgeber: `von = Some(kennung)`, `strom` vom Server vergeben —
/// daran erkennt der Gastgeber, zu welchem Teilnehmer die Bytes gehören, und
/// leitet sie an sein lokales Spiel weiter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StromKopf {
    pub angebot: AngebotId,
    pub von: Option<String>,
    pub strom: u32,
}

/// Kopf eines getunnelten UDP-Datagramms, 8 Byte, big endian.
///
/// ```text
/// [0..4)  Angebotsnummer (u32)
/// [4..6)  Kanal (u16)
/// [6]     Version (1)
/// [7]     reserviert (0)
/// ```
///
/// Danach folgen Zähler und Geheimtext (siehe [`crate::krypto`]). Der Kopf ist
/// **nicht** verschlüsselt und **nicht** signiert: der Server muss ihn lesen
/// können, und gegen Fälschung schützt die Prüfsumme des Geheimtextes — ein
/// umgeschriebener Kopf macht den Empfänger nur ratlos, nicht gläubig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Datagrammkopf {
    pub angebot: AngebotId,
    pub kanal: Kanal,
}

impl Datagrammkopf {
    /// Länge des Kopfs in Byte.
    pub const LAENGE: usize = 8;

    /// Version im Datagrammkopf (ein Byte, nicht zwei).
    pub const VERSION: u8 = 1;

    /// Setzt den Kopf vor die Nutzlast.
    pub fn schreiben(&self, nutzlast: &[u8]) -> Vec<u8> {
        let mut aus = Vec::with_capacity(Self::LAENGE + nutzlast.len());
        aus.extend_from_slice(&self.angebot.to_be_bytes());
        aus.extend_from_slice(&self.kanal.to_be_bytes());
        aus.push(Self::VERSION);
        aus.push(0);
        aus.extend_from_slice(nutzlast);
        aus
    }

    /// Liest den Kopf. `None` bei zu kurzer Eingabe oder fremder Version —
    /// beides heißt für den Aufrufer: verwerfen.
    pub fn lesen(d: &[u8]) -> Option<(Self, &[u8])> {
        if d.len() < Self::LAENGE || d[6] != Self::VERSION {
            return None;
        }
        // `reserviert` wird nicht geprüft: es ist für eine spätere Erweiterung
        // da, und ein gesetztes Bit darf kein Datagramm kosten.
        let angebot = AngebotId::from_be_bytes([d[0], d[1], d[2], d[3]]);
        let kanal = Kanal::from_be_bytes([d[4], d[5]]);
        Some((Self { angebot, kanal }, &d[Self::LAENGE..]))
    }
}

/// Schreibt einen Steuerrahmen (Längenpräfix + JSON).
pub fn schreibe<T: Serialize>(rahmen: &T) -> Result<Vec<u8>, RahmenFehler> {
    schreibe_rahmen(rahmen)
}

/// Liest einen Steuerrahmen aus einem Puffer.
///
/// `Ok(None)` heißt: noch unvollständig. Der zweite Wert ist die Zahl der
/// verbrauchten Bytes.
pub fn lese<T: serde::de::DeserializeOwned>(
    puffer: &[u8],
) -> Result<Option<(T, usize)>, RahmenFehler> {
    lese_rahmen(puffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn muster_angebot() -> Angebot {
        Angebot {
            id: 7,
            name: "Minecraft".to_string(),
            port: 25565,
            art: Art::Tcp,
            gastgeber: "ab".repeat(32),
            gastgeber_name: "Ada".to_string(),
            teilnehmer: vec![Teilnehmer {
                kennung: "cd".repeat(32),
                name: "Bosse".to_string(),
            }],
        }
    }

    fn alle_client_rahmen() -> Vec<ClientRahmen> {
        vec![
            ClientRahmen::Hallo {
                version: VERSION,
                kennung: "ab".repeat(32),
                beweis: "cd".repeat(64),
            },
            ClientRahmen::Anbieten {
                name: "Minecraft".to_string(),
                port: 25565,
                art: Art::Tcp,
            },
            ClientRahmen::Anbieten {
                name: "Sprachserver".to_string(),
                port: 9987,
                art: Art::Udp,
            },
            ClientRahmen::Zurueckziehen { angebot: 1 },
            ClientRahmen::Beitreten { angebot: 2 },
            ClientRahmen::Verlassen { angebot: 3 },
            ClientRahmen::Entfernen {
                angebot: 4,
                kennung: "ef".repeat(32),
            },
        ]
    }

    fn alle_server_rahmen() -> Vec<ServerRahmen> {
        let mut rahmen = vec![
            ServerRahmen::Anforderung {
                version: VERSION,
                nonce: "01".repeat(32),
            },
            ServerRahmen::Willkommen {
                grenzen: Grenzen::default(),
            },
            ServerRahmen::Angebote {
                liste: vec![muster_angebot()],
            },
            ServerRahmen::Angeboten { angebot: 5 },
            ServerRahmen::Beigetreten { angebot: 6 },
            ServerRahmen::Kanal {
                angebot: 7,
                kanal: 1,
                von: "cd".repeat(32),
            },
        ];
        for grund in [
            Grund::Zurueckgezogen,
            Grund::Entfernt,
            Grund::Verlassen,
            Grund::RaumVerlassen,
            Grund::GastgeberWeg,
            Grund::Abgeschaltet,
        ] {
            rahmen.push(ServerRahmen::Beendet { angebot: 8, grund });
        }
        for code in [
            Fehler::Version,
            Fehler::AnmeldungAbgelehnt,
            Fehler::NichtAngemeldet,
            Fehler::KeinRecht,
            Fehler::KeinRaum,
            Fehler::ZuVieleFreigaben,
            Fehler::ZuVieleTeilnehmer,
            Fehler::ZuVieleStroeme,
            Fehler::UnbekanntesAngebot,
            Fehler::AndererRaum,
            Fehler::EigenesAngebot,
            Fehler::NichtGastgeber,
            Fehler::UngueltigeAnfrage,
        ] {
            rahmen.push(ServerRahmen::Fehler {
                code,
                text: code.text().to_string(),
            });
        }
        rahmen
    }

    #[test]
    fn jeder_client_rahmen_ist_umkehrbar() {
        for rahmen in alle_client_rahmen() {
            let roh = schreibe(&rahmen).expect("schreiben");
            let (gelesen, verbraucht) = lese::<ClientRahmen>(&roh)
                .expect("lesen")
                .expect("vollständig");
            assert_eq!(gelesen, rahmen);
            assert_eq!(verbraucht, roh.len());
        }
    }

    #[test]
    fn jeder_server_rahmen_ist_umkehrbar() {
        for rahmen in alle_server_rahmen() {
            let roh = schreibe(&rahmen).expect("schreiben");
            let (gelesen, verbraucht) = lese::<ServerRahmen>(&roh)
                .expect("lesen")
                .expect("vollständig");
            assert_eq!(gelesen, rahmen);
            assert_eq!(verbraucht, roh.len());
        }
    }

    #[test]
    fn json_hat_das_typ_feld() {
        let roh = schreibe(&ClientRahmen::Anbieten {
            name: "Minecraft".to_string(),
            port: 25565,
            art: Art::Udp,
        })
        .expect("schreiben");
        let text = String::from_utf8(roh[4..].to_vec()).expect("UTF-8");
        assert!(text.contains("\"typ\":\"anbieten\""), "{text}");
        assert!(text.contains("\"art\":\"udp\""), "{text}");
        // Die Nummer steht als Zahl im JSON, nicht als Zeichenkette.
        assert!(text.contains("\"port\":25565"), "{text}");

        let roh = schreibe(&ServerRahmen::Beendet {
            angebot: 1,
            grund: Grund::GastgeberWeg,
        })
        .expect("schreiben");
        let text = String::from_utf8(roh[4..].to_vec()).expect("UTF-8");
        assert!(text.contains("\"typ\":\"beendet\""), "{text}");
        assert!(text.contains("\"grund\":\"gastgeber_weg\""), "{text}");
    }

    #[test]
    fn grenzen_haben_die_vorgaben_aus_dem_adr() {
        let g = Grenzen::default();
        assert_eq!(g.freigaben_je_mitglied, 2);
        assert_eq!(g.teilnehmer_je_freigabe, 16);
        assert_eq!(g.stroeme_je_teilnehmer, 32);
        assert_eq!(g.kbit_je_freigabe, 20_000);
    }

    #[test]
    fn grenzen_ohne_felder_bekommt_die_vorgaben() {
        // Ein älterer Server, der ein Feld noch nicht kennt, soll den Client
        // nicht mit einer falschen Null versehen.
        let g: Grenzen = serde_json::from_str("{}").expect("JSON");
        assert_eq!(g, Grenzen::default());
        let teil: Grenzen = serde_json::from_str("{\"teilnehmer_je_freigabe\":4}").expect("JSON");
        assert_eq!(teil.teilnehmer_je_freigabe, 4);
        assert_eq!(teil.freigaben_je_mitglied, 2);
    }

    #[test]
    fn datagrammkopf_ist_umkehrbar() {
        let kopf = Datagrammkopf {
            angebot: 0x0102_0304,
            kanal: 0x0a0b,
        };
        let roh = kopf.schreiben(b"hallo");
        assert_eq!(roh.len(), Datagrammkopf::LAENGE + 5);
        assert_eq!(
            &roh[..8],
            &[1, 2, 3, 4, 0x0a, 0x0b, 1, 0],
            "Big endian, Version 1, reserviert 0"
        );
        let (gelesen, nutzlast) = Datagrammkopf::lesen(&roh).expect("Kopf");
        assert_eq!(gelesen, kopf);
        assert_eq!(nutzlast, b"hallo");
    }

    #[test]
    fn datagrammkopf_ohne_nutzlast() {
        let kopf = Datagrammkopf {
            angebot: 9,
            kanal: 3,
        };
        let roh = kopf.schreiben(&[]);
        let (gelesen, nutzlast) = Datagrammkopf::lesen(&roh).expect("Kopf");
        assert_eq!(gelesen, kopf);
        assert!(nutzlast.is_empty());
    }

    #[test]
    fn zu_kurzer_datagrammkopf_ist_none() {
        for laenge in 0..Datagrammkopf::LAENGE {
            assert!(
                Datagrammkopf::lesen(&[0u8; Datagrammkopf::LAENGE][..laenge]).is_none(),
                "{laenge} Byte müssen zu kurz sein"
            );
        }
    }

    #[test]
    fn falsche_version_ist_none() {
        let kopf = Datagrammkopf {
            angebot: 1,
            kanal: 1,
        };
        let mut roh = kopf.schreiben(b"x");
        roh[6] = 2;
        assert!(Datagrammkopf::lesen(&roh).is_none());
        roh[6] = 0;
        assert!(Datagrammkopf::lesen(&roh).is_none());
    }

    #[test]
    fn stromkopf_ist_umkehrbar() {
        let kopf = StromKopf {
            angebot: 3,
            von: Some("ab".repeat(32)),
            strom: 17,
        };
        let roh = schreibe(&kopf).expect("schreiben");
        let (gelesen, _) = lese::<StromKopf>(&roh)
            .expect("lesen")
            .expect("vollständig");
        assert_eq!(gelesen, kopf);
    }

    #[test]
    fn max_udp_nutzlast_ist_die_vorgabe_aus_dem_adr() {
        assert_eq!(MAX_UDP_NUTZLAST, 1100);
        // Kopf, Sitzung, Zähler und Prüfsumme müssen in ein QUIC-Datagramm
        // passen (Reserve für den Datagrammrahmen).
        const GESAMT: usize = MAX_UDP_NUTZLAST + Datagrammkopf::LAENGE + 8 + 8 + 16;
        const _: () = assert!(GESAMT <= 1150);
    }

    #[test]
    fn alpn_ist_die_vereinbarte_kennung() {
        assert_eq!(ALPN, b"sprechwerk-spielnetz/1");
        assert_eq!(VERSION, 1);
    }
}

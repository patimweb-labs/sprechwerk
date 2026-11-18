//! Was **in** einer verschlüsselten Chatnachricht steht (ADR 0012 §2).
//!
//! Das ist die Sprache zwischen den Clients — der Server sieht davon nichts,
//! nur die Hülle (`geheimtext`). Eine neue Art braucht deshalb keine
//! Protokolländerung; ein älterer Client erkennt sie als
//! [`Gelesen::Unbekannt`] und sagt das, statt sie falsch zu zeigen.

use serde::{Deserialize, Serialize};

/// Aktuelle Fassung des Inhalts.
pub const INHALT_VERSION: u32 = 1;

/// Arten, die dieser Client kennt. Scheitert eine davon beim Lesen, ist sie
/// kaputt — nicht „neuer".
const BEKANNT: &[&str] = &["text", "reaktion"];

/// Höchstlänge eines Reaktionszeichens in Bytes (ein Emoji mit Hautton und
/// Verbindern passt; ein Satz nicht).
pub const MAX_ZEICHEN_BYTES: usize = 32;

/// Eine Nachricht, wie sie vor dem Verschlüsseln aussieht.
///
/// **Neue Felder sind optional** (`serde(default)`): ein älterer Client liest
/// einen Text mit Vorschau noch als Text und zeigt ihn ohne — das
/// ist besser als „neuere Version nötig". Eine neue **Art** dagegen ist für ihn
/// unbekannt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Inhalt {
    /// Geschriebener Text, wahlweise mit Link-Vorschau.
    ///
    /// Bis 1.3 gab es hier ein Feld `effekt` (Konfetti, Herzen, Beben). Es ist
    /// entfallen (ADR 0013); alte Nachrichten tragen es noch und bleiben
    /// lesbar, weil unbekannte Felder übergangen werden.
    Text {
        text: String,
        /// Vorschau des ersten Links — vom **Absender** abgerufen, damit kein
        /// Empfänger seine Adresse einer fremden Seite verrät (ADR 0012 §2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vorschau: Option<Linkvorschau>,
    },
    /// Eine Reaktion auf eine Nachricht. Eigener Umschlag, damit der Server
    /// nicht einmal weiß, dass es eine Reaktion ist; der Client fasst sie
    /// zusammen. `weg` nimmt die eigene Reaktion zurück.
    Reaktion {
        /// Kennung der Nachricht (Textform von `MessageId`).
        ziel: String,
        zeichen: String,
        #[serde(default, skip_serializing_if = "ist_falsch")]
        weg: bool,
    },
}

fn ist_falsch(b: &bool) -> bool {
    !*b
}

/// Vorschau einer Seite, wie sie der Absender gesehen hat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linkvorschau {
    /// Die Adresse, wie sie im Text steht.
    pub url: String,
    pub titel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beschreibung: Option<String>,
    /// Vorschaubild als JPEG, Base64 — klein gerechnet vom Absender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bild: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Umschlag {
    v: u32,
    #[serde(flatten)]
    inhalt: serde_json::Value,
}

/// Ergebnis des Lesens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gelesen {
    Inhalt(Inhalt),
    /// Eine Art, die dieser Client nicht kennt (neuere Version).
    Unbekannt {
        art: String,
    },
    /// Kein gültiger Inhalt (beschädigt oder fremd).
    Kaputt,
}

impl Inhalt {
    /// Ein schlichter Text ohne Vorschau.
    pub fn text(text: impl Into<String>) -> Self {
        Inhalt::Text {
            text: text.into(),
            vorschau: None,
        }
    }

    /// Als Bytes zum Verschlüsseln.
    pub fn schreiben(&self) -> Vec<u8> {
        let inhalt = serde_json::to_value(self).expect("Inhalt ist serialisierbar");
        serde_json::to_vec(&Umschlag {
            v: INHALT_VERSION,
            inhalt,
        })
        .expect("Inhalt ist serialisierbar")
    }

    /// Aus entschlüsselten Bytes.
    pub fn lesen(bytes: &[u8]) -> Gelesen {
        let Ok(umschlag) = serde_json::from_slice::<Umschlag>(bytes) else {
            return Gelesen::Kaputt;
        };
        let art = umschlag
            .inhalt
            .get("art")
            .and_then(|a| a.as_str())
            .map(str::to_string);
        match serde_json::from_value::<Inhalt>(umschlag.inhalt) {
            Ok(inhalt) => Gelesen::Inhalt(inhalt),
            Err(_) => match art {
                Some(art) if !BEKANNT.contains(&art.as_str()) => Gelesen::Unbekannt { art },
                _ => Gelesen::Kaputt,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_ist_umkehrbar() {
        let i = Inhalt::text("Hallo 👋");
        let bytes = i.schreiben();
        let roh = std::str::from_utf8(&bytes).unwrap();
        assert!(roh.contains(r#""v":1"#));
        // Ohne Vorschau steht das Feld gar nicht da — so liest auch ein
        // Client der ersten Stufe den Text.
        assert!(!roh.contains("vorschau"), "{roh}");
        assert_eq!(Inhalt::lesen(&bytes), Gelesen::Inhalt(i));
    }

    #[test]
    fn vorschau_ist_umkehrbar() {
        let i = Inhalt::Text {
            text: "Schau: https://example.org".into(),
            vorschau: Some(Linkvorschau {
                url: "https://example.org".into(),
                titel: "Beispiel".into(),
                beschreibung: Some("Eine Seite".into()),
                bild: Some("AAAA".into()),
            }),
        };
        assert_eq!(Inhalt::lesen(&i.schreiben()), Gelesen::Inhalt(i));
    }

    #[test]
    fn alte_nachricht_mit_effekt_bleibt_lesbar() {
        // So schrieb 1.3 einen Text mit Effekt; das Feld gibt es nicht mehr.
        let alt = br#"{"v":1,"art":"text","text":"Hallo","effekt":"konfetti"}"#;
        assert_eq!(Inhalt::lesen(alt), Gelesen::Inhalt(Inhalt::text("Hallo")));
    }

    #[test]
    fn reaktion_ist_umkehrbar_und_weg_ist_vorgabe_falsch() {
        let i = Inhalt::Reaktion {
            ziel: "nachricht_1".into(),
            zeichen: "👍".into(),
            weg: false,
        };
        let bytes = i.schreiben();
        assert!(!std::str::from_utf8(&bytes).unwrap().contains("weg"));
        assert_eq!(Inhalt::lesen(&bytes), Gelesen::Inhalt(i));
        let weg = Inhalt::Reaktion {
            ziel: "nachricht_1".into(),
            zeichen: "👍".into(),
            weg: true,
        };
        assert_eq!(Inhalt::lesen(&weg.schreiben()), Gelesen::Inhalt(weg));
    }

    #[test]
    fn neue_art_ist_unbekannt_nicht_kaputt() {
        let neu = br#"{"v":1,"art":"anhang","dateien":[]}"#;
        assert_eq!(
            Inhalt::lesen(neu),
            Gelesen::Unbekannt {
                art: "anhang".into()
            }
        );
    }

    #[test]
    fn kaputtes_ist_kaputt() {
        for b in [
            &b""[..],
            b"null",
            b"{}",
            br#"{"v":1}"#,
            // bekannte Art mit fehlendem Feld: kaputt, nicht „neuer"
            br#"{"v":1,"art":"text"}"#,
            br#"{"v":1,"art":"text","text":5}"#,
            br#"{"v":1,"art":"reaktion","zeichen":"x"}"#,
        ] {
            assert_eq!(
                Inhalt::lesen(b),
                Gelesen::Kaputt,
                "{:?}",
                std::str::from_utf8(b)
            );
        }
    }
}

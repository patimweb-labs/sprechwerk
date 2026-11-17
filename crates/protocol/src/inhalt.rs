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
const BEKANNT: &[&str] = &["text"];

/// Eine Nachricht, wie sie vor dem Verschlüsseln aussieht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub enum Inhalt {
    /// Geschriebener Text.
    Text { text: String },
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
        let i = Inhalt::Text {
            text: "Hallo 👋".into(),
        };
        let bytes = i.schreiben();
        assert!(std::str::from_utf8(&bytes).unwrap().contains(r#""v":1"#));
        assert_eq!(Inhalt::lesen(&bytes), Gelesen::Inhalt(i));
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

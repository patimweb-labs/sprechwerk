//! Die Form, in der Chatnachrichten auf dem Server liegen (ADR 0012).
//!
//! ```text
//! e1.<id: 32 hex>.<hex(nonce 12 ‖ ChaCha20-Poly1305(inhalt))>
//! ```
//!
//! Hier steht **keine Krypto** — nur die Hülle. Der Server prüft damit, dass
//! er Geheimtext bekommt (und nie Klartext ablegt) und mit welchem
//! Serverschlüssel er verschlüsselt ist. Ver- und entschlüsselt wird im Client
//! (`sprechwerk_adapters::serverschluessel`).

/// Kennung der Hülle. Steigt, wenn sich Verfahren oder Aufbau ändern.
pub const PRAEFIX: &str = "e1.";

/// Länge einer Schlüsselkennung (16 Byte als Hex).
pub const ID_LEN: usize = 32;

/// Nonce (12) und Echtheitsprüfung (16): so lang ist der Geheimtext mindestens.
pub const MIN_DATEN_BYTES: usize = 12 + 16;

/// Höchstlänge des entschlüsselten Inhalts (JSON). Text ist auf
/// `MAX_MESSAGE_LEN` Zeichen begrenzt; dazu kommen Link-Vorschau mit kleinem
/// Bild oder die Liste der Anhänge.
pub const MAX_INHALT_BYTES: usize = 256 * 1024;

/// Höchstlänge eines Geheimtexts, so wie er gespeichert wird.
pub const MAX_GEHEIMTEXT_LEN: usize =
    PRAEFIX.len() + ID_LEN + 1 + 2 * (MAX_INHALT_BYTES + MIN_DATEN_BYTES);

// Was die Hülle erlaubt, muss die Domain auch speichern.
const _: () = assert!(MAX_GEHEIMTEXT_LEN <= sprechwerk_domain::MAX_GESPEICHERT_LEN);

/// Ein zerlegter Geheimtext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huelle<'a> {
    /// Kennung des Serverschlüssels.
    pub id: &'a str,
    /// Nonce und Geheimtext als Hex.
    pub daten: &'a str,
}

/// Ist `id` eine gültige Schlüsselkennung (32 kleine Hexzeichen)?
pub fn ist_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Zerlegt einen Geheimtext. `None`, wenn es keiner ist — dann ist es
/// Klartext oder kaputt, und beides nimmt der Server nicht an.
pub fn zerlegen(text: &str) -> Option<Huelle<'_>> {
    if text.len() > MAX_GEHEIMTEXT_LEN {
        return None;
    }
    let rest = text.strip_prefix(PRAEFIX)?;
    let (id, daten) = rest.split_once('.')?;
    if !ist_id(id) {
        return None;
    }
    let hex_ok = daten.len() % 2 == 0
        && daten.len() >= 2 * MIN_DATEN_BYTES
        && daten
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    hex_ok.then_some(Huelle { id, daten })
}

/// Setzt eine Hülle zusammen.
pub fn zusammensetzen(id: &str, daten_hex: &str) -> String {
    format!("{PRAEFIX}{id}.{daten_hex}")
}

/// Zusatzdaten der Verschlüsselung: Raum, Verfasser, Schlüsselkennung.
///
/// Damit kann der Server eine Nachricht weder in einen anderen Raum hängen noch
/// einem anderen Verfasser zuschreiben — sie wäre dann nicht mehr lesbar.
pub fn zusatzdaten(raum: &str, verfasser: &str, id: &str) -> Vec<u8> {
    format!("sprechwerk nachricht v1|{raum}|{verfasser}|{id}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gueltig() -> String {
        zusammensetzen(&"ab".repeat(16), &"00".repeat(MIN_DATEN_BYTES))
    }

    #[test]
    fn erkennt_geheimtext() {
        let text = gueltig();
        let h = zerlegen(&text).expect("gültig");
        assert_eq!(h.id, "ab".repeat(16));
        assert_eq!(h.daten.len(), 2 * MIN_DATEN_BYTES);
    }

    #[test]
    fn klartext_und_kaputtes_sind_kein_geheimtext() {
        for text in [
            "Hallo",
            "",
            "e1.",
            "e1.abc.00",
            // Großbuchstaben: eine Kennung, eine Schreibweise.
            &format!("e1.{}.{}", "AB".repeat(16), "00".repeat(28)),
            // zu kurz für Nonce und Prüfsumme
            &format!("e1.{}.{}", "ab".repeat(16), "00".repeat(27)),
            // ungerade Länge
            &format!("e1.{}.{}0", "ab".repeat(16), "00".repeat(28)),
            // kein Hex
            &format!("e1.{}.{}", "ab".repeat(16), "zz".repeat(28)),
            // anderes Verfahren
            &format!("e2.{}.{}", "ab".repeat(16), "00".repeat(28)),
        ] {
            assert!(zerlegen(text).is_none(), "angenommen: {text}");
        }
    }

    #[test]
    fn zu_lang_ist_kein_geheimtext() {
        let lang = zusammensetzen(&"ab".repeat(16), &"00".repeat(MAX_INHALT_BYTES + 29));
        assert!(zerlegen(&lang).is_none());
        // Die Grenze passt in einen Rahmen.
        const { assert!(MAX_GEHEIMTEXT_LEN < crate::MAX_RAHMEN) };
    }

    #[test]
    fn zusatzdaten_binden_raum_und_verfasser() {
        assert_ne!(zusatzdaten("r1", "m1", "x"), zusatzdaten("r2", "m1", "x"));
        assert_ne!(zusatzdaten("r1", "m1", "x"), zusatzdaten("r1", "m2", "x"));
        assert_ne!(zusatzdaten("r1", "m1", "x"), zusatzdaten("r1", "m1", "y"));
    }
}

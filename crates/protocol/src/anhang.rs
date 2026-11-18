//! Anhänge auf der Leitung (ADR 0012 §3).
//!
//! Eine Datei geht **nicht** über den Steuerstrom und nie über Datagramme,
//! sondern über einen **eigenen** bidirektionalen QUIC-Strom derselben
//! Verbindung — angemeldet und verschlüsselt ist sie schon.
//!
//! ```text
//! Hochladen                                Herunterladen
//! Client ── Datenanfrage::Hochladen ─►      Client ── Datenanfrage::Herunterladen ─►
//!        ── Geheimtext (groesse Byte) ─►           ◄─ Datenantwort::Daten { groesse }
//!        ── Ende des Stroms ─►                     ◄─ Geheimtext (groesse Byte)
//!        ◄─ Datenantwort::Fertig                    ◄─ Ende des Stroms
//! ```
//!
//! Der Server sieht nur **Geheimtext, Größe und Zeit** — keinen Namen, keinen
//! Typ, keinen Schlüssel. Die stehen im Nachrichtenumschlag
//! ([`crate::inhalt::Datei`]), den nur Mitglieder öffnen.
//!
//! # Blöcke
//!
//! Der Client verschlüsselt in Blöcken zu [`BLOCK`] Byte, jeder mit eigenem
//! Nonce (Blocknummer und Schlussmarke) und eigener Prüfsumme (16 Byte). Damit
//! muss weder eine große Datei in den Speicher noch ein Video ganz entschlüsselt
//! werden, bevor es spielt — ein Bereich lässt sich blockweise lesen. Die
//! Schlussmarke im letzten Block verhindert, dass jemand die Datei unbemerkt
//! kürzt. Hier steht nur die **Rechnung**; die Krypto steht im Client
//! (`sprechwerk_adapters::anhang`).

use serde::{Deserialize, Serialize};

use crate::Fehlercode;

/// Größe eines Klartextblocks.
pub const BLOCK: u64 = 64 * 1024;

/// Prüfsumme je Block (Poly1305).
pub const MARKE: u64 = 16;

/// Länge einer Anhangskennung (16 Byte als Hex).
pub const ID_LEN: usize = 32;

/// Höchstzahl der Dateien an einer Nachricht.
pub const MAX_DATEIEN: usize = 16;

/// Ist `id` eine gültige Anhangskennung (32 kleine Hexzeichen)?
///
/// Die Kennung wird Dateiname auf dem Server — deshalb so eng: kein Punkt,
/// kein Schrägstrich, keine Großbuchstaben.
pub fn ist_id(id: &str) -> bool {
    id.len() == ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Wie viele Blöcke eine Datei mit `klar` Byte hat. Auch eine leere Datei hat
/// einen (leeren) Block — er trägt die Schlussmarke.
pub fn bloecke(klar: u64) -> u64 {
    klar.div_ceil(BLOCK).max(1)
}

/// Größe des Geheimtexts zu `klar` Byte Klartext.
pub fn geheim_groesse(klar: u64) -> u64 {
    klar + bloecke(klar) * MARKE
}

/// Größe des Klartexts zu `geheim` Byte Geheimtext — `None`, wenn es keine
/// Größe ist, die beim Verschlüsseln entstehen kann.
pub fn klar_groesse(geheim: u64) -> Option<u64> {
    let voll = BLOCK + MARKE;
    let ganze = geheim / voll;
    let rest = geheim % voll;
    let klar = match (ganze, rest) {
        (0, r) if r >= MARKE => r - MARKE,
        (0, _) => return None,
        (n, 0) => n * BLOCK,
        (_, r) if r > MARKE => ganze * BLOCK + r - MARKE,
        _ => return None,
    };
    (geheim_groesse(klar) == geheim).then_some(klar)
}

/// Der erste Rahmen auf einem Datenstrom (vom Client).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum Datenanfrage {
    /// Danach folgen genau `groesse` Byte Geheimtext, dann das Ende des Stroms.
    ///
    /// Die Kennung wählt der **Client** (Zufall): sie steht in den
    /// Zusatzdaten jedes Blocks, so kann der Server Dateien nicht vertauschen.
    Hochladen { id: String, groesse: u64 },
    /// Bitte um eine Datei.
    Herunterladen { id: String },
}

/// Die Antwort des Servers auf einem Datenstrom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum Datenantwort {
    /// Hochgeladen und abgelegt. Bis eine Nachricht auf die Datei zeigt, gilt
    /// sie als offen und wird nach [`OFFEN_FRIST_S`] weggeräumt.
    Fertig,
    /// Danach folgen genau `groesse` Byte Geheimtext.
    Daten {
        groesse: u64,
    },
    Fehler {
        code: Fehlercode,
        text: String,
    },
}

/// So lange darf eine hochgeladene Datei ohne Nachricht liegen (Sekunden).
pub const OFFEN_FRIST_S: u64 = 3_600;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groessen_passen_zueinander() {
        for klar in [
            0,
            1,
            BLOCK - 1,
            BLOCK,
            BLOCK + 1,
            3 * BLOCK,
            3 * BLOCK + 7,
            10_000_000,
        ] {
            let geheim = geheim_groesse(klar);
            assert_eq!(klar_groesse(geheim), Some(klar), "klar = {klar}");
        }
        assert_eq!(geheim_groesse(0), MARKE);
        assert_eq!(geheim_groesse(BLOCK), BLOCK + MARKE);
        assert_eq!(geheim_groesse(BLOCK + 1), BLOCK + 1 + 2 * MARKE);
    }

    #[test]
    fn unmoegliche_groessen_sind_keine() {
        // Kürzer als eine Marke, oder ein Rest, der keine Marke tragen kann.
        for geheim in [0, 1, MARKE - 1, BLOCK + MARKE + 1, BLOCK + MARKE + MARKE] {
            assert_eq!(klar_groesse(geheim), None, "geheim = {geheim}");
        }
    }

    #[test]
    fn kennung_ist_eng() {
        assert!(ist_id(&"0a".repeat(16)));
        for nein in [
            "",
            "../../etc/passwd",
            &"0A".repeat(16),
            &"0a".repeat(15),
            &format!("{}.x", "0a".repeat(15)),
        ] {
            assert!(!ist_id(nein), "{nein}");
        }
    }

    #[test]
    fn rahmen_haben_stabile_namen() {
        let a = serde_json::to_string(&Datenanfrage::Hochladen {
            id: "x".into(),
            groesse: 3,
        })
        .unwrap();
        assert_eq!(a, r#"{"typ":"hochladen","id":"x","groesse":3}"#);
        let b = serde_json::to_string(&Datenantwort::Daten { groesse: 9 }).unwrap();
        assert_eq!(b, r#"{"typ":"daten","groesse":9}"#);
    }
}

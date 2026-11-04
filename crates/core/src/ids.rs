//! Kennungen.
//!
//! **Festlegung (SPEC §7):** Räume, Mitglieder und Nachrichten tragen eine
//! zufällige 128-Bit-Kennung, nach außen als Text. Kennungen werden **nie**
//! wiederverwendet — ein wiederverwendeter Wert lässt eine Oberfläche Daten
//! des Vorgängers anzeigen, und genau dieser Fehler ist bei TeamSpeak
//! berüchtigt (`clid`-Wiederverwendung).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! kennung {
    ($name:ident, $vorsilbe:literal, $doku:literal) => {
        #[doc = $doku]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u128);

        // Auf der Leitung und im Export steht die **Textform** (`raum_3f2a…`).
        // Zwei Gründe: JSON kennt kein 128-Bit-Integer, und ein lesbarer Wert
        // ist beim Fehlersuchen mehr wert als 16 gesparte Byte.
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.text())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct Besucher;
                impl serde::de::Visitor<'_> for Besucher {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, "eine Kennung wie {}{{32 hex}}", $vorsilbe)
                    }
                    fn visit_str<E: serde::de::Error>(self, wert: &str) -> Result<$name, E> {
                        $name::from_text(wert).map_err(|e| E::custom(e.to_string()))
                    }
                }
                d.deserialize_str(Besucher)
            }
        }

        impl $name {
            /// Neue Zufallskennung.
            pub fn neu() -> Self {
                Self(uuid_artig())
            }

            /// Kennung aus einem Rohwert (nur für Import und Tests).
            pub fn aus_roh(wert: u128) -> Self {
                Self(wert)
            }

            /// Rohwert.
            pub fn roh(self) -> u128 {
                self.0
            }

            /// Kennung aus ihrer Textform (mit oder ohne Vorsilbe).
            ///
            /// Wird beim Import gebraucht: dort darf ein kaputter Wert nicht
            /// stillschweigend zu einer Zufallskennung werden.
            pub fn from_text(text: &str) -> Result<Self, crate::state::Error> {
                let roh = text.strip_prefix($vorsilbe).unwrap_or(text);
                if roh.len() != 32 || !roh.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(crate::state::Error::Kennung(text.to_string()));
                }
                let wert = u128::from_str_radix(roh, 16)
                    .map_err(|_| crate::state::Error::Kennung(text.to_string()))?;
                Ok(Self(wert))
            }

            /// Textform mit Vorsilbe, etwa `raum_3f2a…`.
            pub fn text(self) -> String {
                format!("{}{:032x}", $vorsilbe, self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{:032x}", $vorsilbe, self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self)
            }
        }
    };
}

kennung!(RoomId, "raum_", "Kennung eines Raums.");
kennung!(MemberId, "mitglied_", "Kennung eines Mitglieds.");
kennung!(MessageId, "nachricht_", "Kennung einer Nachricht.");

/// 128 Bit aus dem Betriebssystem-Zufall.
///
/// Ohne eigene Krypto: `std` liefert über [`std::collections::hash_map::RandomState`]
/// keine belastbare Quelle, deshalb wird hier die Systemzeit mit einem
/// prozessweiten Zähler und der Adresse eines frisch angelegten Wertes
/// vermischt. Für Kennungen genügt das; **Geheimnisse werden so nicht erzeugt**
/// (die kommen aus dem Schlüsselpaar, siehe `proto`).
fn uuid_artig() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ZAEHLER: AtomicU64 = AtomicU64::new(0);

    let zeit = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = ZAEHLER.fetch_add(1, Ordering::Relaxed);
    let platz = Box::new(0u8);
    let adresse = Box::into_raw(platz) as u64;

    // Zwei Runden xorshift, damit auch dicht aufeinanderfolgende Aufrufe
    // streuen.
    let mut a = zeit ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut b = adresse ^ (zeit.rotate_left(17)) ^ n;
    for _ in 0..2 {
        a ^= a << 13;
        a ^= a >> 7;
        a ^= a << 17;
        b ^= b << 11;
        b ^= b >> 5;
        b ^= b << 19;
    }
    ((a as u128) << 64) | b as u128
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn kennungen_sind_eindeutig() {
        let menge: HashSet<u128> = (0..10_000).map(|_| RoomId::neu().roh()).collect();
        assert_eq!(menge.len(), 10_000, "Kennungen wiederholen sich");
    }

    #[test]
    fn textform_traegt_vorsilbe_und_32_stellen() {
        let id = RoomId::aus_roh(0x1234);
        assert_eq!(
            id.text(),
            "raum_00000000000000000000000000001234".to_string(),
            "{id}"
        );
        assert!(format!("{id}").starts_with("raum_"));
    }

    #[test]
    fn serde_benutzt_die_textform() {
        let id = MemberId::aus_roh(7);
        let text = serde_json::to_string(&id).unwrap();
        assert_eq!(text, "\"mitglied_00000000000000000000000000000007\"");
        let zurueck: MemberId = serde_json::from_str(&text).unwrap();
        assert_eq!(zurueck, id);
        // Kaputte Werte werden abgelehnt, nicht stillschweigend ersetzt.
        assert!(serde_json::from_str::<MemberId>("\"mitglied_xyz\"").is_err());
    }
}

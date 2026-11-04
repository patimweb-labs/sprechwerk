//! Rollen und Rechte.
//!
//! **Festlegung (SPEC §5):** vier feste Rollen, global gültig, Rechte im
//! Programm festgelegt. Keine Kanalgruppen, keine Vererbung, keine
//! Leistungsrechte, keine konfigurierbaren Rechte.
//!
//! Das ist bewusst die Gegenposition zu TeamSpeak (489 Rechte, fünf Ebenen,
//! Negation, Vererbung): dort kann niemand mehr erklären, warum jemand etwas
//! darf. Hier ist die Antwort eine Tabellenzeile.

use serde::{Deserialize, Serialize};

/// Die vier Rollen. Reihenfolge = Rangordnung, aber sie wird **nicht** für
/// Rechteprüfungen benutzt (jede Rolle hat ihre Rechte aus der Tabelle); sie
/// dient nur der Sortierung in Listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Volle Verwaltung: Rollen, Sperren, Einstellungen, Räume, Personen.
    Administrator,
    /// Räume und Personen, kein Rechtemanagement, keine Sperren.
    Moderator,
    /// Sprechen, schreiben, lesen, Räume betreten.
    Mitglied,
    /// Schreiben und lesen, **nicht** sprechen, Verlauf nur ab Anwesenheit.
    Gast,
}

impl Role {
    /// Textform für Protokoll und Datenbank.
    pub fn text(self) -> &'static str {
        match self {
            Role::Administrator => "administrator",
            Role::Moderator => "moderator",
            Role::Mitglied => "mitglied",
            Role::Gast => "gast",
        }
    }

    /// Aus der Textform; unbekannte Werte ergeben [`None`] (der Aufrufer
    /// entscheidet, ob das ein Fehler ist — beim Import ist es einer).
    pub fn aus_text(text: &str) -> Option<Self> {
        match text {
            "administrator" => Some(Role::Administrator),
            "moderator" => Some(Role::Moderator),
            "mitglied" => Some(Role::Mitglied),
            "gast" => Some(Role::Gast),
            _ => None,
        }
    }
}

/// Die vollständige Liste der Rechte. Mehr gibt es nicht — was hier fehlt,
/// kann niemand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Einen Raum betreten (Sprache und Text).
    RaumBetreten,
    /// Den Verlauf eines Raums vollständig lesen (nicht nur das, was während
    /// der eigenen Anwesenheit geschrieben wurde).
    VerlaufLesen,
    /// Text schreiben.
    TextSchreiben,
    /// Sprache senden (Pakettyp Sprachrahmen).
    SpracheSenden,
    /// Räume anlegen, umbenennen, verschieben, löschen.
    RaumVerwalten,
    /// Personen verschieben, stummschalten, aus dem Raum werfen.
    PersonEingreifen,
    /// Personen sperren (Kennung und Adresse).
    Sperren,
    /// Rollen vergeben.
    RollenVergeben,
    /// Prüfprotokoll lesen.
    ProtokollLesen,
    /// Servereinstellungen ändern.
    EinstellungenAendern,
}

/// Die Rechtetabelle. **Diese Funktion ist die einzige Wahrheit** über Rechte.
pub fn darf(rolle: Role, recht: Capability) -> bool {
    use Capability::*;
    use Role::*;
    match rolle {
        Administrator => true,
        Moderator => matches!(
            recht,
            RaumBetreten
                | VerlaufLesen
                | TextSchreiben
                | SpracheSenden
                | RaumVerwalten
                | PersonEingreifen
                | ProtokollLesen
        ),
        Mitglied => matches!(
            recht,
            RaumBetreten | VerlaufLesen | TextSchreiben | SpracheSenden
        ),
        Gast => matches!(recht, RaumBetreten | TextSchreiben),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabelle_entspricht_der_festlegung() {
        // SPEC §5, Zeile für Zeile.
        assert!(darf(Role::Administrator, Capability::Sperren));
        assert!(darf(Role::Administrator, Capability::RollenVergeben));
        assert!(darf(Role::Administrator, Capability::EinstellungenAendern));
        assert!(darf(Role::Administrator, Capability::ProtokollLesen));

        assert!(darf(Role::Moderator, Capability::RaumVerwalten));
        assert!(darf(Role::Moderator, Capability::PersonEingreifen));
        assert!(darf(Role::Moderator, Capability::ProtokollLesen));
        assert!(!darf(Role::Moderator, Capability::Sperren));
        assert!(!darf(Role::Moderator, Capability::RollenVergeben));

        assert!(darf(Role::Mitglied, Capability::SpracheSenden));
        assert!(darf(Role::Mitglied, Capability::VerlaufLesen));
        assert!(!darf(Role::Mitglied, Capability::RaumVerwalten));
        assert!(!darf(Role::Mitglied, Capability::PersonEingreifen));

        assert!(darf(Role::Gast, Capability::RaumBetreten));
        assert!(darf(Role::Gast, Capability::TextSchreiben));
        assert!(!darf(Role::Gast, Capability::SpracheSenden));
        assert!(!darf(Role::Gast, Capability::VerlaufLesen));
    }

    #[test]
    fn textform_ist_umkehrbar() {
        for rolle in [
            Role::Administrator,
            Role::Moderator,
            Role::Mitglied,
            Role::Gast,
        ] {
            assert_eq!(Role::aus_text(rolle.text()), Some(rolle));
        }
        assert_eq!(Role::aus_text("chef"), None);
    }
}

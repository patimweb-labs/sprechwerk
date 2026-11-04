// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Einstellungen des Fensters.
//!
//! Bewusst wenige und nur solche, die das Fenster selbst braucht (Serveradresse,
//! Name, Farbschema, Sprache). Was den Server betrifft, gehört in dessen
//! Konfigurationsdatei — nicht in eine zweite Wahrheit.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Pfad der Einstellungsdatei (vom Fenster verwaltet).
pub struct EinstellungenPfad(pub PathBuf);

/// Ein gemerkter Server in der Favoritenliste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Favorit {
    /// Anzeigename (frei wählbar), z. B. „Trainingsabend".
    pub name: String,
    /// Adresse, z. B. `sprechwerk.verein.example:7443`.
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Einstellungen {
    /// Zuletzt benutzte Serveradresse.
    pub server: String,
    /// Anzeigename.
    pub name: String,
    /// Farbschema: `dark`, `light` oder `mono` (AGENTS.md §2.7).
    pub thema: String,
    /// Sprache der Oberfläche (`de`).
    pub sprache: String,
    /// Serverpasswort merken (nur wenn der Nutzer es will).
    pub passwort: String,
    /// Aufnahmegerät (Name des Geräts; leer = Vorgabe des Systems).
    pub mikrofon: String,
    /// Wiedergabegerät (leer = Vorgabe des Systems).
    pub lautsprecher: String,
    /// Push-to-Talk statt offenem Mikrofon.
    pub push_to_talk: bool,
    /// Taste für Push-to-Talk (Anzeigename, z. B. „Leertaste").
    pub ptt_taste: String,
    /// Sprechschwelle 0–100 (nur bei offenem Mikrofon).
    pub schwelle: u8,
    /// Vorlauf in Millisekunden: so viel Ton wird vor dem Tastendruck
    /// mitgeschickt, damit das erste Wort nicht abgeschnitten wird.
    pub vorlauf_ms: u16,
    /// Gemerkte Server (Favoriten) — die Startseite, wenn keine Verbindung steht.
    pub favoriten: Vec<Favorit>,
    /// Kompaktes Design: dichtere Zeilen im Verlauf und in den Raumlisten.
    pub kompakt: bool,
    /// Rauschunterdrückung im Aufnahmepfad (Störgeräusche, Lüfter, Tastatur).
    pub rauschunterdrueckung: bool,
    /// Noisegate: unterhalb der Sprechschwelle wird **nichts** gesendet.
    pub noisegate: bool,
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self {
            server: "127.0.0.1:7443".to_string(),
            name: String::new(),
            thema: "dark".to_string(),
            sprache: "de".to_string(),
            passwort: String::new(),
            mikrofon: String::new(),
            lautsprecher: String::new(),
            push_to_talk: false,
            ptt_taste: "Leertaste".to_string(),
            schwelle: 30,
            vorlauf_ms: 60,
            favoriten: Vec::new(),
            kompakt: false,
            rauschunterdrueckung: true,
            noisegate: true,
        }
    }
}

impl Einstellungen {
    /// Lädt aus der Datei; fehlt sie, gelten die Vorgaben.
    pub fn laden(pfad: &Path) -> Self {
        std::fs::read_to_string(pfad)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Schreibt die Datei (Verzeichnis wird angelegt).
    pub fn speichern(&self, pfad: &Path) -> Result<(), String> {
        if let Some(eltern) = pfad.parent() {
            std::fs::create_dir_all(eltern).map_err(|e| format!("Verzeichnis: {e}"))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("JSON: {e}"))?;
        std::fs::write(pfad, text).map_err(|e| format!("Schreiben: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgaben_sind_brauchbar() {
        let e = Einstellungen::default();
        assert_eq!(e.thema, "dark");
        assert_eq!(e.sprache, "de");
        assert_eq!(e.server, "127.0.0.1:7443");
        assert_eq!(e.vorlauf_ms, 60, "Vorlauf gegen abgeschnittenes erstes Wort");
        assert!(!e.push_to_talk);
        assert!(e.favoriten.is_empty());
        assert!(!e.kompakt, "dichtes Design ist eine Wahl, nicht die Vorgabe");
    }

    #[test]
    fn speichern_und_laden_ist_umkehrbar() {
        let verzeichnis = std::env::temp_dir().join(format!("sprechwerk-desktop-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let mut e = Einstellungen::default();
        e.name = "Ada".into();
        e.thema = "light".into();
        e.speichern(&pfad).unwrap();
        let geladen = Einstellungen::laden(&pfad);
        assert_eq!(geladen, e);
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[test]
    fn favoriten_ueberleben_das_speichern() {
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop3-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let mut e = Einstellungen::default();
        e.favoriten = vec![
            Favorit {
                name: "Trainingsabend".into(),
                server: "192.0.2.10:7443".into(),
            },
            Favorit {
                name: "Vorstand".into(),
                server: "192.0.2.11:7443".into(),
            },
        ];
        e.speichern(&pfad).unwrap();
        let geladen = Einstellungen::laden(&pfad);
        assert_eq!(geladen.favoriten.len(), 2);
        assert_eq!(geladen.favoriten[0].name, "Trainingsabend");
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[test]
    fn alte_datei_ohne_favoriten_bleibt_lesbar() {
        // Nachrüsten darf bestehende Einstellungen nicht unlesbar machen.
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop4-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        std::fs::create_dir_all(&verzeichnis).unwrap();
        std::fs::write(&pfad, r#"{"server":"127.0.0.1:7443","name":"Ada","thema":"dark","sprache":"de","passwort":"","mikrofon":"","lautsprecher":"","push_to_talk":false,"ptt_taste":"Leertaste","schwelle":30,"vorlauf_ms":60}"#).unwrap();
        let geladen = Einstellungen::laden(&pfad);
        assert_eq!(geladen.name, "Ada");
        assert!(geladen.favoriten.is_empty());
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[test]
    fn kaputte_datei_faellt_auf_vorgaben_zurueck() {
        let verzeichnis = std::env::temp_dir().join(format!("sprechwerk-desktop2-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        std::fs::create_dir_all(&verzeichnis).unwrap();
        std::fs::write(&pfad, "{ kaputt").unwrap();
        assert_eq!(Einstellungen::laden(&pfad), Einstellungen::default());
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }
}

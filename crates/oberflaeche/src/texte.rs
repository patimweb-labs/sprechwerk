// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Sprache der Oberfläche: Deutsch, Englisch, Spanisch, Französisch.
//!
//! Eine Wahrheit, vier Sprachen: `texte.json`. Fehlt ein Schlüssel in der
//! gewählten Sprache, gilt Deutsch; fehlt er ganz, steht der Schlüssel da
//! (sichtbar, nicht leer). Platzhalter `{}` werden der Reihe nach gefüllt.

use std::collections::HashMap;
use std::sync::OnceLock;

type Tabelle = HashMap<String, HashMap<String, String>>;

fn tabelle() -> &'static Tabelle {
    static T: OnceLock<Tabelle> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str(include_str!("../texte.json")).expect("texte.json ist gültig")
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sprache {
    #[default]
    De,
    En,
    Es,
    Fr,
}

impl Sprache {
    pub const ALLE: [Sprache; 4] = [Sprache::De, Sprache::En, Sprache::Es, Sprache::Fr];

    pub fn aus_code(code: &str) -> Self {
        match code {
            "en" => Sprache::En,
            "es" => Sprache::Es,
            "fr" => Sprache::Fr,
            _ => Sprache::De,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Sprache::De => "de",
            Sprache::En => "en",
            Sprache::Es => "es",
            Sprache::Fr => "fr",
        }
    }

    pub fn anzeige(self) -> &'static str {
        match self {
            Sprache::De => "Deutsch",
            Sprache::En => "English",
            Sprache::Es => "Español",
            Sprache::Fr => "Français",
        }
    }
}

/// Text zu einem Schlüssel.
pub fn t(sprache: Sprache, schluessel: &str) -> String {
    match tabelle().get(schluessel) {
        None => schluessel.to_string(),
        Some(eintrag) => eintrag
            .get(sprache.code())
            .or_else(|| eintrag.get("de"))
            .cloned()
            .unwrap_or_else(|| schluessel.to_string()),
    }
}

/// Text mit Platzhaltern.
pub fn tf(sprache: Sprache, schluessel: &str, werte: &[&dyn std::fmt::Display]) -> String {
    let mut text = t(sprache, schluessel);
    for wert in werte {
        text = text.replacen("{}", &wert.to_string(), 1);
    }
    text
}

/// Abkürzung der Rolle fürs Abzeichen (A, M, G); Mitglied hat keine.
pub fn rolle_kurz(sprache: Sprache, rolle: sprechwerk_domain::Role) -> String {
    let name = match rolle {
        sprechwerk_domain::Role::Administrator => "administrator",
        sprechwerk_domain::Role::Moderator => "moderator",
        sprechwerk_domain::Role::Mitglied => return String::new(),
        sprechwerk_domain::Role::Gast => "gast",
    };
    t(sprache, &format!("rolle.kurz.{name}"))
}

/// Rollenname in der gewählten Sprache.
pub fn rolle(sprache: Sprache, rolle: sprechwerk_domain::Role) -> String {
    let name = match rolle {
        sprechwerk_domain::Role::Administrator => "administrator",
        sprechwerk_domain::Role::Moderator => "moderator",
        sprechwerk_domain::Role::Mitglied => "mitglied",
        sprechwerk_domain::Role::Gast => "gast",
    };
    t(sprache, &format!("rolle.{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deutsch_und_rueckfall() {
        assert_eq!(t(Sprache::De, "app.settings"), "Einstellungen");
        assert_eq!(t(Sprache::En, "app.settings"), "Settings");
        assert_eq!(t(Sprache::Fr, "gibt.es.nicht"), "gibt.es.nicht");
    }

    #[test]
    fn platzhalter_der_reihe_nach() {
        assert_eq!(
            tf(Sprache::De, "app.counts", &[&3, &12]),
            "3 anwesend · 12 Nachrichten"
        );
    }

    #[test]
    fn jeder_schluessel_hat_deutsch() {
        for (k, v) in tabelle() {
            assert!(v.contains_key("de"), "{k} ohne Deutsch");
        }
    }

    #[test]
    fn codes_hin_und_zurueck() {
        for s in Sprache::ALLE {
            assert_eq!(Sprache::aus_code(s.code()), s);
        }
    }
}

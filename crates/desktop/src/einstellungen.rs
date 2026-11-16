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
    /// Wann zuletzt verbunden (ms seit der Epoche); 0 = noch nie.
    ///
    /// Steht je Eintrag, nicht nur für den letzten insgesamt: wer drei Server
    /// hat, will bei jedem sehen, wann er dort war.
    pub zuletzt: i64,
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
    /// Chat eingeklappt (schmale Schiene rechts). **Kein Schalter in den
    /// Einstellungen** — der Zustand wird nur gemerkt.
    ///
    /// `alias = "kompakt"`: ältere Einstellungsdateien hatten dort ein
    /// Feld für ein dichteres Layout; es bedeutet dasselbe („enger") und wird
    /// übernommen, statt die Datei zu verwerfen.
    #[serde(alias = "kompakt")]
    pub chat_eingeklappt: bool,
    /// Rauschunterdrückung im Aufnahmepfad (Störgeräusche, Lüfter, Tastatur).
    pub rauschunterdrueckung: bool,
    /// Noisegate: unterhalb der Sprechschwelle wird **nichts** gesendet.
    pub noisegate: bool,
    /// Echounterdrückung: die eigene Wiedergabe aus der Aufnahme rechnen.
    /// Nötig, wenn über Lautsprecher gesprochen wird (sonst pfeift es).
    pub echounterdrueckung: bool,
    /// Angeheftete Server-Zertifikate (TOFU, ADR 0002 §3).
    ///
    /// **Warum an der Adresse und nicht am Favoriten?** Man kann sich mit einem
    /// Server verbinden, ohne ihn zu merken; das Zertifikat muss trotzdem
    /// geprüft werden. Der Favorit ist eine Bequemlichkeit, die Anheftung eine
    /// Sicherheitszusage.
    pub vertrauen: Vec<Vertrauen>,
}

/// Ein angehefteter Fingerabdruck: „dieser Adresse glaube ich, solange sie
/// dieses Zertifikat zeigt".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Vertrauen {
    /// Adresse, wie sie eingetippt wurde (`sprechwerk.verein.example:7443`).
    pub server: String,
    /// SHA-256 über das Zertifikat, hexadezimal (64 Zeichen).
    pub fingerabdruck: String,
}

impl Einstellungen {
    /// Merkt sich, wann dieser Server zuletzt benutzt wurde.
    ///
    /// Nur bei vorhandenen Einträgen: wer einen Server ohne Favorit verbindet,
    /// soll sich nicht plötzlich einen Eintrag einfangen.
    pub fn merke_verbindung(&mut self, server: &str) {
        let jetzt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.server = server.to_string();
        if let Some(f) = self.favoriten.iter_mut().find(|f| f.server == server) {
            f.zuletzt = jetzt;
        }
    }
}

impl Einstellungen {
    /// Der angeheftete Fingerabdruck dieses Servers, falls einer vorliegt.
    ///
    /// `None` heißt: **erster Kontakt**. Der Aufrufer nimmt das Zertifikat an
    /// und heftet es an — mehr sagt „trust on first use" nicht.
    pub fn vertrauen_fuer(&self, server: &str) -> Option<&str> {
        let gesucht = adresse(server);
        self.vertrauen
            .iter()
            .find(|v| adresse(&v.server) == gesucht)
            .map(|v| v.fingerabdruck.as_str())
    }

    /// Heftet einen Fingerabdruck an (ersetzt einen vorhandenen Eintrag).
    ///
    /// Ein leerer Wert wird ignoriert: ein Eintrag ohne Fingerabdruck wäre eine
    /// Zusage ohne Inhalt und würde beim nächsten Kontakt alles durchlassen.
    pub fn merke_vertrauen(&mut self, server: &str, fingerabdruck: &str) {
        let wert = sprechwerk_protocol::normalisiere(fingerabdruck);
        if wert.len() != sprechwerk_protocol::FINGERABDRUCK_LEN {
            return;
        }
        let gesucht = adresse(server);
        match self
            .vertrauen
            .iter_mut()
            .find(|v| adresse(&v.server) == gesucht)
        {
            Some(vorhanden) => vorhanden.fingerabdruck = wert,
            None => self.vertrauen.push(Vertrauen {
                server: server.trim().to_string(),
                fingerabdruck: wert,
            }),
        }
    }

    /// Vergisst den angehefteten Wert — der nächste Kontakt heftet neu an.
    ///
    /// Der **einzige** Weg, eine Abweichung zu übergehen, und er ist bewusst
    /// eine Handlung: ein Server, der neu aufgesetzt wurde, braucht ihn; ein
    /// Angreifer darf ihn nicht auslösen können.
    pub fn vergiss_vertrauen(&mut self, server: &str) -> bool {
        let gesucht = adresse(server);
        let vorher = self.vertrauen.len();
        self.vertrauen.retain(|v| adresse(&v.server) != gesucht);
        self.vertrauen.len() != vorher
    }
}

/// Adressen vergleichen: Groß-/Kleinschreibung und Leerzeichen am Rand zählen
/// nicht — `Verein.Example:7443` und `verein.example:7443` sind derselbe Server.
fn adresse(server: &str) -> String {
    server.trim().to_ascii_lowercase()
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
            chat_eingeklappt: false,
            rauschunterdrueckung: true,
            noisegate: true,
            echounterdrueckung: true,
            vertrauen: Vec::new(),
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
    ///
    /// Nur für den Eigentümer lesbar: hier steht das gemerkte Serverpasswort.
    /// Mit `std::fs::write` lag es unter Linux für jeden Benutzer des Rechners
    /// lesbar (0644).
    pub fn speichern(&self, pfad: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("JSON: {e}"))?;
        sprechwerk_adapters::geheim_schreiben(pfad, text.as_bytes())
            .map_err(|e| format!("Schreiben: {e}"))
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
        assert_eq!(
            e.vorlauf_ms, 60,
            "Vorlauf gegen abgeschnittenes erstes Wort"
        );
        assert!(!e.push_to_talk);
        assert!(e.favoriten.is_empty());
        assert!(
            !e.chat_eingeklappt,
            "der Chat ist offen, bis jemand ihn zuklappt"
        );
    }

    #[test]
    fn speichern_und_laden_ist_umkehrbar() {
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let e = Einstellungen {
            name: "Ada".into(),
            thema: "light".into(),
            ..Default::default()
        };
        e.speichern(&pfad).unwrap();
        let geladen = Einstellungen::laden(&pfad);
        assert_eq!(geladen, e);
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[cfg(unix)]
    #[test]
    fn einstellungen_mit_passwort_sind_nicht_fuer_alle_lesbar() {
        use std::os::unix::fs::PermissionsExt;
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop-rechte-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let e = Einstellungen {
            passwort: "vereinsgeheim".into(),
            ..Default::default()
        };
        e.speichern(&pfad).unwrap();
        let rechte = std::fs::metadata(&pfad).unwrap().permissions().mode() & 0o777;
        assert_eq!(rechte, 0o600, "einstellungen.json hat {rechte:o}");
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }

    #[test]
    fn favoriten_ueberleben_das_speichern() {
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop3-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let e = Einstellungen {
            favoriten: vec![
                Favorit {
                    name: "Trainingsabend".into(),
                    server: "192.0.2.10:7443".into(),
                    zuletzt: 1_700_000_000_000,
                },
                Favorit {
                    name: "Vorstand".into(),
                    server: "192.0.2.11:7443".into(),
                    zuletzt: 0,
                },
            ],
            ..Default::default()
        };
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
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-desktop2-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        std::fs::create_dir_all(&verzeichnis).unwrap();
        std::fs::write(&pfad, "{ kaputt").unwrap();
        assert_eq!(Einstellungen::laden(&pfad), Einstellungen::default());
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }
    /// „Zuletzt verbunden" gilt je Eintrag — und nur für vorhandene Einträge.
    #[test]
    fn zuletzt_verbunden_je_eintrag() {
        let mut e = Einstellungen {
            favoriten: vec![
                Favorit {
                    name: "Zuhause".into(),
                    server: "192.0.2.10:7443".into(),
                    zuletzt: 0,
                },
                Favorit {
                    name: "Verein".into(),
                    server: "192.0.2.11:7443".into(),
                    zuletzt: 5_000,
                },
            ],
            ..Default::default()
        };
        e.merke_verbindung("192.0.2.10:7443");
        assert!(
            e.favoriten[0].zuletzt > 0,
            "der benutzte Server bekommt eine Zeit"
        );
        assert_eq!(e.favoriten[1].zuletzt, 5_000, "der andere bleibt unberührt");
        assert_eq!(e.server, "192.0.2.10:7443");

        // Ein Server ohne Eintrag legt keinen an.
        let vorher = e.favoriten.len();
        e.merke_verbindung("198.51.100.99:7443");
        assert_eq!(e.favoriten.len(), vorher, "kein Eintrag ohne Favorit");
        assert_eq!(e.server, "198.51.100.99:7443", "aber gemerkt wird er");
    }

    #[test]
    fn zertifikat_wird_angeheftet_und_wieder_gefunden() {
        let mut e = Einstellungen::default();
        let fp = sprechwerk_protocol::zertifikat_fingerabdruck(b"verein");
        assert!(
            e.vertrauen_fuer("Verein.Example:7443").is_none(),
            "erster Kontakt: noch nichts angeheftet"
        );
        e.merke_vertrauen("Verein.Example:7443", &fp);
        assert_eq!(
            e.vertrauen_fuer(" verein.example:7443 "),
            Some(fp.as_str()),
            "Adresse wird verglichen, nicht buchstabiert"
        );
        assert_eq!(e.vertrauen.len(), 1);

        // Denselben Server erneut anheften ersetzt, statt zu sammeln.
        let neu = sprechwerk_protocol::zertifikat_fingerabdruck(b"neu");
        e.merke_vertrauen("verein.example:7443", &neu);
        assert_eq!(e.vertrauen.len(), 1, "ein Eintrag je Adresse");
        assert_eq!(e.vertrauen_fuer("verein.example:7443"), Some(neu.as_str()));
    }

    #[test]
    fn vergessen_ist_eine_handlung_und_wirkt() {
        let mut e = Einstellungen::default();
        let fp = sprechwerk_protocol::zertifikat_fingerabdruck(b"verein");
        e.merke_vertrauen("verein.example:7443", &fp);
        assert!(e.vergiss_vertrauen("VEREIN.example:7443"));
        assert!(e.vertrauen_fuer("verein.example:7443").is_none());
        assert!(
            !e.vergiss_vertrauen("verein.example:7443"),
            "nichts zu vergessen ist keine Änderung"
        );
    }

    #[test]
    fn unbrauchbarer_fingerabdruck_wird_nicht_angeheftet() {
        let mut e = Einstellungen::default();
        e.merke_vertrauen("verein.example:7443", "");
        e.merke_vertrauen("verein.example:7443", "3ff0");
        assert!(
            e.vertrauen.is_empty(),
            "ein Eintrag ohne vollen Wert würde alles durchlassen"
        );
    }

    #[test]
    fn angeheftete_zertifikate_ueberleben_das_speichern() {
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-vertrauen-{}", std::process::id()));
        let pfad = verzeichnis.join("einstellungen.json");
        let fp = sprechwerk_protocol::zertifikat_fingerabdruck(b"verein");
        let mut e = Einstellungen::default();
        e.merke_vertrauen("verein.example:7443", &fp);
        e.speichern(&pfad).unwrap();
        let geladen = Einstellungen::laden(&pfad);
        assert_eq!(
            geladen.vertrauen_fuer("verein.example:7443"),
            Some(fp.as_str()),
            "sonst wäre die Anheftung nach jedem Neustart weg"
        );
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }
}

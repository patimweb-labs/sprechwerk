//! Anhänge auf diesem Gerät (ADR 0012 §3): Zwischenspeicher und das
//! Adressschema `anhang:`, über das das Fenster Bilder und Videos lädt.
//!
//! **Im Zwischenspeicher liegt Geheimtext**, so wie er vom Server kam. Erst
//! beim Lesen wird entschlüsselt — und nur der Bereich, den das Fenster
//! verlangt (ein Video fragt in Stücken). So liegt kein Klartext auf der
//! Platte, den niemand speichern wollte, und keine Datei muss ganz in den
//! Speicher.
//!
//! Die Schlüssel stehen hier, nicht im Modell: die Oberfläche kennt nur die
//! Kennung einer Datei. Eingetragen werden sie beim Entschlüsseln des
//! Verlaufs (`anwendung::chat_lesen`) — wer eine Nachricht lesen kann, kann
//! auch ihre Dateien laden.

use sprechwerk_adapters::anhang::{Dateischluessel, Geheimdatei};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Höchstens so viel liefert eine Antwort auf eine Bereichsanfrage. Ein
/// `<video>` fragt „ab hier bis zum Ende" — das wäre bei einem großen Film
/// ein Gigabyte im Speicher. Mit Teilantwort fragt es einfach weiter.
pub const MAX_BEREICH: u64 = 2 * 1024 * 1024;

/// Ohne Bereichsanfrage (ein `<img>`) wird so viel ganz ausgeliefert.
pub const MAX_GANZ: u64 = 64 * 1024 * 1024;

/// So lange bleibt eine Datei im Zwischenspeicher (Tage).
pub const AUFBEWAHREN_TAGE: u64 = 30;

#[derive(Clone)]
struct Eintrag {
    schluessel: Dateischluessel,
    /// Geprüfter Medientyp (`abbilden::art_und_typ`).
    typ: &'static str,
}

/// Die Dateien dieses Geräts.
pub struct Dateiablage {
    verzeichnis: PathBuf,
    eintraege: Mutex<HashMap<String, Eintrag>>,
}

/// Was das Adressschema antwortet — ohne Tauri, damit es prüfbar ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http {
    pub status: u16,
    pub typ: &'static str,
    pub inhalt: Vec<u8>,
    /// `Content-Range`, wenn es eine Teilantwort ist.
    pub bereich: Option<String>,
    pub gesamt: u64,
}

impl Http {
    fn fehler(status: u16) -> Self {
        Self {
            status,
            typ: "text/plain",
            inhalt: Vec::new(),
            bereich: None,
            gesamt: 0,
        }
    }
}

impl Dateiablage {
    /// Legt das Verzeichnis an, räumt halbe und alte Dateien weg.
    pub fn neu(verzeichnis: impl AsRef<Path>) -> Self {
        let verzeichnis = verzeichnis.as_ref().to_path_buf();
        let _ = std::fs::create_dir_all(&verzeichnis);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&verzeichnis, std::fs::Permissions::from_mode(0o700));
        }
        let frist = std::time::Duration::from_secs(AUFBEWAHREN_TAGE * 86_400);
        if let Ok(liste) = std::fs::read_dir(&verzeichnis) {
            for e in liste.flatten() {
                let alt = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|d| d > frist);
                let halb = e.path().extension().is_some_and(|x| x == "teil");
                if alt || halb {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Self {
            verzeichnis,
            eintraege: Mutex::new(HashMap::new()),
        }
    }

    /// Merkt sich Schlüssel und Typ einer Datei.
    pub fn merken(&self, id: &str, schluessel: Dateischluessel, typ: &'static str) {
        if !sprechwerk_protocol::anhang::ist_id(id) {
            return;
        }
        if let Ok(mut e) = self.eintraege.lock() {
            e.insert(id.to_string(), Eintrag { schluessel, typ });
        }
    }

    /// Der Schlüssel einer bekannten Datei.
    pub fn schluessel(&self, id: &str) -> Option<Dateischluessel> {
        self.eintraege
            .lock()
            .ok()?
            .get(id)
            .map(|e| e.schluessel.clone())
    }

    /// Wo der Geheimtext liegt (liegt oder liegen wird).
    pub fn pfad(&self, id: &str) -> PathBuf {
        self.verzeichnis.join(id)
    }

    /// Liegt die Datei schon vor?
    pub fn liegt_vor(&self, id: &str) -> bool {
        sprechwerk_protocol::anhang::ist_id(id) && self.pfad(id).is_file()
    }

    /// Verwirft eine Datei, die sich nicht entschlüsseln ließ.
    pub fn vergessen(&self, id: &str) {
        if sprechwerk_protocol::anhang::ist_id(id) {
            let _ = std::fs::remove_file(self.pfad(id));
        }
    }

    /// Beantwortet eine Anfrage an `anhang://…/<id>` (mit `Range`, wenn
    /// das Fenster eine schickt).
    pub fn antwort(&self, pfad: &str, bereich: Option<&str>) -> Http {
        let id = pfad.trim_start_matches('/');
        if !sprechwerk_protocol::anhang::ist_id(id) || !self.liegt_vor(id) {
            return Http::fehler(404);
        }
        let Some(eintrag) = self.eintraege.lock().ok().and_then(|e| e.get(id).cloned()) else {
            return Http::fehler(404);
        };
        let Ok(mut datei) = Geheimdatei::oeffnen(&self.pfad(id), eintrag.schluessel, id) else {
            return Http::fehler(500);
        };
        let gesamt = datei.groesse();
        let (von, bis, teil) = match bereich {
            Some(b) => match bereich_lesen(b, gesamt) {
                Some((von, bis)) => (von, bis.min(von + MAX_BEREICH), true),
                None => {
                    return Http {
                        bereich: Some(format!("bytes */{gesamt}")),
                        gesamt,
                        ..Http::fehler(416)
                    }
                }
            },
            None if gesamt > MAX_GANZ => (0, MAX_BEREICH.min(gesamt), true),
            None => (0, gesamt, false),
        };
        match datei.lesen(von, bis) {
            Ok(inhalt) => Http {
                status: if teil { 206 } else { 200 },
                typ: eintrag.typ,
                bereich: teil.then(|| format!("bytes {von}-{}/{gesamt}", bis.saturating_sub(1))),
                inhalt,
                gesamt,
            },
            // Verändert: nichts zeigen, und beim nächsten Mal neu laden.
            Err(_) => {
                self.vergessen(id);
                Http::fehler(500)
            }
        }
    }
}

/// Liest `bytes=a-b`, `bytes=a-` oder `bytes=-n`. Liefert `[von, bis)`.
pub fn bereich_lesen(kopf: &str, gesamt: u64) -> Option<(u64, u64)> {
    let spanne = kopf.trim().strip_prefix("bytes=")?;
    // Mehrere Bereiche fragt ein Video nicht; der erste genügt.
    let spanne = spanne.split(',').next()?.trim();
    let (a, b) = spanne.split_once('-')?;
    let (von, bis) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (gesamt.saturating_sub(n), gesamt)
        }
        (a, "") => (a.parse().ok()?, gesamt),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.saturating_add(1)),
    };
    let bis = bis.min(gesamt);
    (von < bis).then_some((von, bis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_adapters::anhang::{neue_id, verschluesseln};

    fn ablage(name: &str) -> (Dateiablage, PathBuf) {
        let d =
            std::env::temp_dir().join(format!("sprechwerk-dateien-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        (Dateiablage::neu(&d), d)
    }

    #[test]
    fn bereiche() {
        assert_eq!(bereich_lesen("bytes=0-", 100), Some((0, 100)));
        assert_eq!(bereich_lesen("bytes=10-19", 100), Some((10, 20)));
        assert_eq!(bereich_lesen("bytes=90-500", 100), Some((90, 100)));
        assert_eq!(bereich_lesen("bytes=-10", 100), Some((90, 100)));
        assert_eq!(bereich_lesen("bytes=0-0, 5-6", 100), Some((0, 1)));
        for nein in [
            "bytes=100-",
            "bytes=x-1",
            "items=0-1",
            "bytes=5-4",
            "bytes=-0",
        ] {
            assert_eq!(bereich_lesen(nein, 100), None, "{nein}");
        }
    }

    #[test]
    fn liefert_entschluesselt_und_in_stuecken() {
        let (a, d) = ablage("liefern");
        let id = neue_id();
        let s = Dateischluessel::neu();
        let klar: Vec<u8> = (0..(3 * MAX_BEREICH as usize)).map(|i| i as u8).collect();
        std::fs::write(a.pfad(&id), verschluesseln(&s, &id, &klar)).unwrap();

        // Ohne Schlüssel: nichts.
        assert_eq!(a.antwort(&format!("/{id}"), None).status, 404);
        a.merken(&id, s.clone(), "video/mp4");

        // Ganz (klein genug).
        let h = a.antwort(&format!("/{id}"), None);
        assert_eq!(
            (h.status, h.typ, h.bereich.as_deref()),
            (200, "video/mp4", None)
        );
        assert_eq!(h.inhalt, klar);

        // „Ab hier bis zum Ende": gekappt, als Teilantwort.
        let h = a.antwort(&format!("/{id}"), Some("bytes=100-"));
        assert_eq!(h.status, 206);
        assert_eq!(h.inhalt.len() as u64, MAX_BEREICH);
        assert_eq!(h.inhalt[..], klar[100..100 + MAX_BEREICH as usize]);
        assert_eq!(
            h.bereich.as_deref(),
            Some(format!("bytes 100-{}/{}", 100 + MAX_BEREICH - 1, klar.len()).as_str())
        );

        // Außerhalb: 416.
        assert_eq!(
            a.antwort(&format!("/{id}"), Some("bytes=999999999-"))
                .status,
            416
        );
        // Kein Weg hinaus.
        assert_eq!(a.antwort("/../schluessel", None).status, 404);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn veraenderte_datei_wird_vergessen() {
        let (a, d) = ablage("kaputt");
        let id = neue_id();
        let s = Dateischluessel::neu();
        let mut geheim = verschluesseln(&s, &id, b"Hallo Verein");
        geheim[3] ^= 1;
        std::fs::write(a.pfad(&id), geheim).unwrap();
        a.merken(&id, s, "image/png");
        assert_eq!(a.antwort(&id, None).status, 500);
        assert!(!a.liegt_vor(&id), "eine veränderte Datei wird neu geladen");
        let _ = std::fs::remove_dir_all(d);
    }
}

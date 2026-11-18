//! Die Ablage der Anhänge: ein Verzeichnis, eine Datei je Anhang (ADR 0012 §3).
//!
//! Hier liegt **nur Geheimtext** — verschlüsselt vom Absender mit einem
//! Schlüssel, den der Server nie sieht. Der Dateiname ist die Kennung (32
//! Hexzeichen, `protocol::anhang::ist_id`), also nichts, was ein Mitglied
//! wählen könnte, um aus dem Verzeichnis zu entkommen.
//!
//! Während des Hochladens heißt die Datei `<id>.teil`; erst wenn alle Bytes da
//! sind, wird sie umbenannt. Was beim Start noch `.teil` heißt, ist ein
//! abgebrochener Versuch und fliegt weg.
//!
//! Aufgeräumt wird **nach Gebrauch, nicht nach Ereignis**: alles, worauf keine
//! Nachricht zeigt und was nicht gerade hochgeladen oder frisch abgelegt ist,
//! wird gelöscht. So erwischt es jeden Weg, auf dem eine Nachricht
//! verschwindet — Löschen, Raum löschen, Aufbewahrungsfrist, Import.

use sprechwerk_protocol::anhang::ist_id;
use std::path::{Path, PathBuf};

const TEIL: &str = "teil";

/// Das Verzeichnis der Anhänge.
#[derive(Debug, Clone)]
pub struct Ablage {
    verzeichnis: PathBuf,
}

impl Ablage {
    /// Legt das Verzeichnis an (nur für den Eigentümer) und räumt halbe
    /// Uploads weg.
    pub fn oeffnen(verzeichnis: impl AsRef<Path>) -> std::io::Result<Self> {
        let verzeichnis = verzeichnis.as_ref().to_path_buf();
        std::fs::create_dir_all(&verzeichnis)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&verzeichnis, std::fs::Permissions::from_mode(0o700))?;
        }
        let ablage = Self { verzeichnis };
        for (name, _) in ablage.eintraege() {
            if name.ends_with(&format!(".{TEIL}")) {
                let _ = std::fs::remove_file(ablage.verzeichnis.join(&name));
            }
        }
        Ok(ablage)
    }

    /// Pfad der fertigen Datei. Nur mit geprüfter Kennung aufrufen.
    pub fn pfad(&self, id: &str) -> PathBuf {
        debug_assert!(ist_id(id));
        self.verzeichnis.join(id)
    }

    /// Pfad während des Hochladens.
    pub fn teil(&self, id: &str) -> PathBuf {
        debug_assert!(ist_id(id));
        self.verzeichnis.join(format!("{id}.{TEIL}"))
    }

    /// Gibt es die fertige Datei?
    pub fn gibt_es(&self, id: &str) -> bool {
        ist_id(id) && self.pfad(id).is_file()
    }

    /// Wie viel die fertigen Dateien belegen, in Byte. Laufende Uploads
    /// zählt der Kern mit ihrer angekündigten Größe dazu.
    pub fn belegt(&self) -> u64 {
        self.eintraege()
            .iter()
            .filter(|(name, _)| ist_id(name))
            .map(|(_, g)| g)
            .sum()
    }

    /// Macht aus dem Teil die fertige Datei.
    pub fn abschliessen(&self, id: &str) -> std::io::Result<()> {
        std::fs::rename(self.teil(id), self.pfad(id))
    }

    /// Verwirft einen abgebrochenen Upload.
    pub fn verwerfen(&self, id: &str) {
        let _ = std::fs::remove_file(self.teil(id));
    }

    /// Löscht jede fertige Datei, die `behalten` nicht nennt. Liefert, wie
    /// viele es waren. Fremdes im Verzeichnis (kein Kennungsname) bleibt
    /// liegen — das hat nicht der Server hingelegt.
    pub fn aufraeumen(&self, behalten: impl Fn(&str) -> bool) -> usize {
        let mut weg = 0;
        for (name, _) in self.eintraege() {
            if ist_id(&name) && !behalten(&name) && std::fs::remove_file(self.pfad(&name)).is_ok() {
                weg += 1;
            }
        }
        weg
    }

    fn eintraege(&self) -> Vec<(String, u64)> {
        let Ok(liste) = std::fs::read_dir(&self.verzeichnis) else {
            return Vec::new();
        };
        liste
            .filter_map(Result::ok)
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file()
                    .then(|| (e.file_name().to_string_lossy().to_string(), m.len()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verzeichnis(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sprechwerk-ablage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn halbe_uploads_fliegen_beim_oeffnen_weg() {
        let d = verzeichnis("teil");
        let a = Ablage::oeffnen(&d).unwrap();
        let id = "0a".repeat(16);
        std::fs::write(a.teil(&id), b"halb").unwrap();
        assert_eq!(a.belegt(), 0, "halbe zählen nicht als belegt");
        let a = Ablage::oeffnen(&d).unwrap();
        assert!(!a.teil(&id).exists());
        assert!(!a.gibt_es(&id));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aufraeumen_behaelt_was_gebraucht_wird() {
        let d = verzeichnis("auf");
        let a = Ablage::oeffnen(&d).unwrap();
        let (x, y) = ("0a".repeat(16), "0b".repeat(16));
        for id in [&x, &y] {
            std::fs::write(a.teil(id), b"12345").unwrap();
            a.abschliessen(id).unwrap();
        }
        std::fs::write(d.join("liesmich.txt"), b"fremd").unwrap();
        assert_eq!(a.belegt(), 10);
        assert_eq!(a.aufraeumen(|id| id == x), 1);
        assert!(a.gibt_es(&x));
        assert!(!a.gibt_es(&y));
        assert!(d.join("liesmich.txt").exists(), "Fremdes bleibt liegen");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keine_wege_aus_dem_verzeichnis() {
        let d = verzeichnis("weg");
        let a = Ablage::oeffnen(&d).unwrap();
        assert!(!a.gibt_es("../sprechwerk.sqlitedb"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

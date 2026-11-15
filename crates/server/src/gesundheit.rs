//! Lebenszeichen für den Container — und die Prüfung dazu.
//!
//! **Warum nicht einfach „verbinde dich und schau, ob es geht"?** Weil der
//! erste Client Administrator wird (SPEC §7). Ein `HEALTHCHECK`, der sich alle
//! 30 Sekunden verbindet, hätte den Administratorplatz mit einem Wegwerfschlüssel
//! belegt — der Betreiber käme nie hinein. Ein Gesundheitscheck darf den Dienst
//! nicht verändern.
//!
//! Deshalb schreibt der Server im Betrieb einen Zeitstempel in
//! `<daten>/lebt`, und `sprechwerk --gesundheit` prüft nur, ob er frisch ist.
//! Das beweist genau das, was ein Healthcheck beweisen soll: die Aufgabe, die
//! den Dienst am Leben hält, läuft noch. Kein Port, kein Schlüssel, kein Netz.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Wie oft das Lebenszeichen geschrieben wird.
pub const TAKT: Duration = Duration::from_secs(10);

/// Ab wann ein Lebenszeichen als alt gilt (drei Takte: ein Aussetzer ist kein
/// Ausfall).
pub const GRENZE_MS: i64 = 30_000;

/// Dateiname im Datenverzeichnis.
pub const DATEI: &str = "lebt";

/// Wo das Lebenszeichen liegt.
pub fn pfad(daten: &Path) -> PathBuf {
    daten.join(DATEI)
}

/// Schreibt den Zeitstempel — über eine Zwischendatei und `rename`, damit der
/// Leser nie eine halb geschriebene Datei sieht.
pub fn schreibe(daten: &Path, jetzt_ms: i64) -> std::io::Result<()> {
    std::fs::create_dir_all(daten)?;
    let ziel = pfad(daten);
    let zwischen = daten.join(format!("{DATEI}.neu"));
    {
        let mut datei = std::fs::File::create(&zwischen)?;
        writeln!(datei, "{jetzt_ms}")?;
        datei.sync_all()?;
    }
    std::fs::rename(&zwischen, &ziel)
}

/// Liest den Zeitstempel; `None`, wenn er fehlt oder unlesbar ist.
pub fn lese(daten: &Path) -> Option<i64> {
    let text = std::fs::read_to_string(pfad(daten)).ok()?;
    text.trim().parse().ok()
}

/// Ist der Dienst gesund? Reine Rechnung, damit prüfbar ohne Prozess.
///
/// `None` heißt: noch nie geschrieben — dann ist der Dienst nicht gesund (er
/// hätte beim Start sofort schreiben müssen).
pub fn ist_frisch(stand: Option<i64>, jetzt_ms: i64) -> bool {
    let Some(stand) = stand else { return false };
    // Ein Zeitstempel aus der Zukunft ist kein Beweis für Gesundheit, sondern
    // für eine verstellte Uhr oder eine bearbeitete Datei: nicht gesund.
    if stand > jetzt_ms {
        return false;
    }
    jetzt_ms - stand <= GRENZE_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frisch_ist_gesund() {
        assert!(ist_frisch(Some(1_000), 1_000));
        assert!(ist_frisch(Some(1_000), 1_000 + GRENZE_MS));
    }

    #[test]
    fn alt_ist_krank() {
        assert!(!ist_frisch(Some(1_000), 1_000 + GRENZE_MS + 1));
    }

    #[test]
    fn fehlend_ist_krank() {
        // Nie geschrieben: der Dienst läuft nicht (oder erst seit Millisekunden).
        assert!(!ist_frisch(None, 1_000));
    }

    #[test]
    fn zeitstempel_aus_der_zukunft_gilt_nicht() {
        assert!(!ist_frisch(Some(2_000), 1_000));
    }

    #[test]
    fn schreiben_und_lesen_ist_umkehrbar() {
        let verzeichnis =
            std::env::temp_dir().join(format!("sprechwerk-lebt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        assert!(lese(&verzeichnis).is_none(), "frisches Verzeichnis: nichts");
        schreibe(&verzeichnis, 12_345).unwrap();
        assert_eq!(lese(&verzeichnis), Some(12_345));
        // Zweites Schreiben ersetzt, ohne Reste.
        schreibe(&verzeichnis, 67_890).unwrap();
        assert_eq!(lese(&verzeichnis), Some(67_890));
        assert!(
            !pfad(&verzeichnis).with_extension("neu").exists(),
            "die Zwischendatei bleibt nicht liegen"
        );
        let _ = std::fs::remove_dir_all(&verzeichnis);
    }
}

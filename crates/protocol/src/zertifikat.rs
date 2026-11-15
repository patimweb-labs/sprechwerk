//! Fingerabdruck des Server-Zertifikats — **eine** Rechnung für beide Seiten.
//!
//! Der Server druckt den Fingerabdruck beim Start an, der Client rechnet ihn
//! beim Verbinden aus und vergleicht ihn mit dem angehefteten Wert (TOFU,
//! ADR 0002 §3). Beide müssen dasselbe rechnen: SHA-256 über die
//! DER-Kodierung des Zertifikats, hexadezimal klein.
//!
//! **Warum hier und nicht zweimal?** Läge die Rechnung an zwei Orten, wäre ein
//! Unterschied zwischen „was der Server anzeigt" und „was der Client prüft" ein
//! stiller Fehler — und genau der Fehler, den eine Anheftung aufdecken soll.
//! Ein Test vergleicht deshalb beide Aufrufer mit demselben Zertifikat.

use sha2::{Digest, Sha256};

/// Länge eines Fingerabdrucks in Zeichen (32 Byte hexadezimal).
pub const FINGERABDRUCK_LEN: usize = 64;

/// SHA-256 über ein DER-Zertifikat, hexadezimal klein.
pub fn zertifikat_fingerabdruck(zertifikat: &[u8]) -> String {
    hex::encode(Sha256::digest(zertifikat))
}

/// Räumt eine Nutzereingabe auf: Kleinbuchstaben, nur Hex-Ziffern.
///
/// Wer den Fingerabdruck vom Serverbildschirm abtippt, schreibt Doppelpunkte
/// oder Leerzeichen mit — das darf nicht zu „falsches Zertifikat" führen.
pub fn normalisiere(fingerabdruck: &str) -> String {
    fingerabdruck
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Sind zwei Fingerabdrücke derselbe? (Doppelpunkte und Groß-/Kleinschreibung
/// zählen nicht, fehlende Hex-Ziffern dagegen sehr wohl.)
pub fn gleich(a: &str, b: &str) -> bool {
    let a = normalisiere(a);
    let b = normalisiere(b);
    a.len() == FINGERABDRUCK_LEN && a == b
}

/// Darf dieses Zertifikat gelten?
///
/// * `erwartet == None` — erster Kontakt: annehmen (der Aufrufer heftet den
///   Wert danach an, siehe `Zertifikatspruefung` in `sprechwerk-adapters`).
/// * sonst: muss übereinstimmen, und zwar vollständig.
///
/// Reine Rechnung, ohne TLS — deshalb prüfbar, ohne einen Server zu starten.
pub fn pruefe_fingerabdruck(erwartet: Option<&str>, gesehen: &str) -> Result<(), String> {
    let Some(erwartet) = erwartet else {
        return Ok(());
    };
    if gleich(erwartet, gesehen) {
        return Ok(());
    }
    Err(format!(
        "Der Server hat ein anderes Zertifikat als beim letzten Mal.\n\
         erwartet: {}\n\
         gesehen:  {}\n\
         Die Verbindung wurde abgebrochen — es kann ein Angriff sein. Wurde der \
         Server neu aufgesetzt (Zertifikat neu erzeugt), muss der gemerkte Wert \
         erst vergessen werden.",
        gruppiere(erwartet),
        gruppiere(gesehen),
    ))
}

/// Fingerabdruck in Gruppen zu vier Zeichen: `3ff0:5a78:eb5d:…`.
///
/// Dieselbe Schreibweise wie in der Oberfläche (`ui/src/format.ts`), damit man
/// Serverbildschirm und Fenster Zeichen für Zeichen vergleichen kann.
pub fn gruppiere(fingerabdruck: &str) -> String {
    let roh = normalisiere(fingerabdruck);
    if roh.is_empty() {
        return "—".to_string();
    }
    let gruppen: Vec<String> = roh
        .as_bytes()
        .chunks(4)
        .map(|stueck| String::from_utf8_lossy(stueck).to_string())
        .collect();
    gruppen.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_stimmt_mit_dem_bekannten_wert() {
        // Bekannter Vektor: SHA-256("abc").
        assert_eq!(
            zertifikat_fingerabdruck(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn leerzeichen_und_doppelpunkte_stoeren_nicht() {
        let roh = zertifikat_fingerabdruck(b"zert");
        let getippt = gruppiere(&roh).to_uppercase();
        assert!(gleich(&roh, &getippt), "{roh} gegen {getippt}");
        assert!(pruefe_fingerabdruck(Some(&getippt), &roh).is_ok());
    }

    #[test]
    fn ein_anderes_zertifikat_wird_abgelehnt() {
        let a = zertifikat_fingerabdruck(b"server-a");
        let b = zertifikat_fingerabdruck(b"server-b");
        let fehler = pruefe_fingerabdruck(Some(&a), &b).unwrap_err();
        assert!(fehler.contains("anderes Zertifikat"), "{fehler}");
        // Beide Werte stehen in der Meldung: ohne sie kann niemand entscheiden.
        assert!(fehler.contains(&gruppiere(&a)));
        assert!(fehler.contains(&gruppiere(&b)));
    }

    #[test]
    fn erster_kontakt_ohne_erwartung_ist_erlaubt() {
        assert!(pruefe_fingerabdruck(None, &zertifikat_fingerabdruck(b"x")).is_ok());
    }

    #[test]
    fn abgeschnittener_fingerabdruck_gilt_nicht() {
        // Achtung Falle: ein Vergleich über `starts_with` würde hier bestehen.
        let a = zertifikat_fingerabdruck(b"server-a");
        assert!(pruefe_fingerabdruck(Some(&a), &a[..8]).is_err());
        assert!(pruefe_fingerabdruck(Some(&a[..8]), &a).is_err());
    }

    #[test]
    fn gruppierung_ist_die_anzeigeform() {
        assert_eq!(gruppiere("3ff05a78eb5d"), "3ff0:5a78:eb5d");
        assert_eq!(gruppiere(""), "—");
    }
}

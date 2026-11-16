//! Zertifikat für QUIC.
//!
//! QUIC ist immer TLS 1.3 (**Festlegung SPEC §7**: keine eigene Krypto). Beim
//! ersten Start erzeugt der Server ein selbst signiertes Zertifikat, legt es im
//! Datenverzeichnis ab und zeigt den Fingerabdruck an. Der Client muss diesen
//! Fingerabdruck bestätigen — genau wie bei SSH beim ersten Verbinden.
//!
//! Die Rechnung für den Fingerabdruck steht **nicht** hier, sondern in
//! `sprechwerk_protocol::zertifikat`: Client und Server müssen Bit für Bit
//! dasselbe rechnen, sonst prüft der Client etwas anderes, als der Server
//! anzeigt.

use anyhow::Context;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::path::Path;

/// Zertifikat und Schlüssel in Byteform, fertig für `quinn`.
pub struct Zertifikat {
    pub kette: Vec<CertificateDer<'static>>,
    pub schluessel: PrivateKeyDer<'static>,
    /// SHA-256 über das Zertifikat, hexadezimal — das ist die Kennung, die der
    /// Client bestätigt.
    pub fingerabdruck: String,
}

/// Lädt das Zertifikat aus dem Datenverzeichnis oder erzeugt es.
pub fn laden_oder_erzeugen(daten: &Path) -> anyhow::Result<Zertifikat> {
    std::fs::create_dir_all(daten)
        .with_context(|| format!("Datenverzeichnis {}", daten.display()))?;
    let zert_pfad = daten.join("zertifikat.der");
    let key_pfad = daten.join("schluessel.der");

    let (zert_der, key_der) = match (std::fs::read(&zert_pfad), std::fs::read(&key_pfad)) {
        (Ok(z), Ok(k)) if !z.is_empty() && !k.is_empty() => (z, k),
        _ => {
            let erzeugt = rcgen::generate_simple_self_signed(vec!["sprechwerk".to_string()])
                .context("Zertifikat erzeugen")?;
            let z = erzeugt.cert.der().to_vec();
            let k = erzeugt.key_pair.serialize_der();
            std::fs::write(&zert_pfad, &z)?;
            // Der Schlüssel ist ein Geheimnis: nur der Eigentümer darf lesen —
            // und zwar ab dem ersten Byte, nicht erst nach einem `chmod`.
            geheim_schreiben(&key_pfad, &k)
                .with_context(|| format!("TLS-Schlüssel {}", key_pfad.display()))?;
            (z, k)
        }
    };

    let fingerabdruck = fingerabdruck(&zert_der);
    Ok(Zertifikat {
        kette: vec![CertificateDer::from(zert_der.to_vec())],
        schluessel: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        fingerabdruck,
    })
}

/// Schreibt eine Datei mit Rechten 0600 **von Anfang an** (unter Unix).
///
/// Dieselbe Rechnung wie `sprechwerk_adapters::geheim_schreiben`; der Server
/// hängt bewusst nicht an der Client-Kiste. Erst schreiben und dann `chmod`
/// ließe ein Fenster, in dem der TLS-Schlüssel für alle lesbar ist.
fn geheim_schreiben(pfad: &Path, inhalt: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut optionen = std::fs::OpenOptions::new();
    optionen.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        optionen.mode(0o600);
    }
    let mut datei = optionen.open(pfad)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        datei.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    datei.write_all(inhalt)?;
    datei.sync_all()
}

/// SHA-256 über das Zertifikat als Hex.
///
/// Delegiert an `sprechwerk_protocol`: die Anzeige des Servers und die Prüfung
/// des Clients sind **dieselbe** Funktion, nicht zwei gleiche.
pub fn fingerabdruck(zert_der: &[u8]) -> String {
    sprechwerk_protocol::zertifikat_fingerabdruck(zert_der)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn tls_schluessel_ist_nur_fuer_den_dienst_lesbar() {
        use std::os::unix::fs::PermissionsExt;
        let daten =
            std::env::temp_dir().join(format!("sprechwerk-tls-rechte-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&daten);
        laden_oder_erzeugen(&daten).unwrap();
        let rechte = std::fs::metadata(daten.join("schluessel.der"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(rechte, 0o600, "TLS-Schlüssel hat {rechte:o}");
        let _ = std::fs::remove_dir_all(&daten);
    }

    #[test]
    fn zertifikat_wird_einmal_erzeugt_und_dann_geladen() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-tls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = laden_oder_erzeugen(&dir).unwrap();
        let b = laden_oder_erzeugen(&dir).unwrap();
        assert_eq!(
            a.fingerabdruck, b.fingerabdruck,
            "Fingerabdruck muss stabil bleiben"
        );
        assert_eq!(a.kette[0].as_ref(), b.kette[0].as_ref());
        assert_eq!(a.fingerabdruck.len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

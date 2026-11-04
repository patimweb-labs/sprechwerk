//! Zertifikat für QUIC.
//!
//! QUIC ist immer TLS 1.3 (**Festlegung SPEC §7**: keine eigene Krypto). Beim
//! ersten Start erzeugt der Server ein selbst signiertes Zertifikat, legt es im
//! Datenverzeichnis ab und zeigt den Fingerabdruck an. Der Client muss diesen
//! Fingerabdruck bestätigen — genau wie bei SSH beim ersten Verbinden.

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
    std::fs::create_dir_all(daten).with_context(|| format!("Datenverzeichnis {}", daten.display()))?;
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
            std::fs::write(&key_pfad, &k)?;
            // Der Schlüssel ist ein Geheimnis: nur der Eigentümer darf lesen.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&key_pfad, std::fs::Permissions::from_mode(0o600));
            }
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

/// SHA-256 über das Zertifikat als Hex.
pub fn fingerabdruck(zert_der: &[u8]) -> String {
    // ring ist über rustls ohnehin da; SHA-256 direkt daraus.
    let digest = ring_sha256(zert_der);
    hex::encode(digest)
}

fn ring_sha256(daten: &[u8]) -> [u8; 32] {
    // SHA-256 ohne zusätzliche Abhängigkeit: `ring` ist über rustls vorhanden.
    let digest = ring::digest::digest(&ring::digest::SHA256, daten);
    let mut aus = [0u8; 32];
    aus.copy_from_slice(digest.as_ref());
    aus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zertifikat_wird_einmal_erzeugt_und_dann_geladen() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-tls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = laden_oder_erzeugen(&dir).unwrap();
        let b = laden_oder_erzeugen(&dir).unwrap();
        assert_eq!(a.fingerabdruck, b.fingerabdruck, "Fingerabdruck muss stabil bleiben");
        assert_eq!(a.kette[0].as_ref(), b.kette[0].as_ref());
        assert_eq!(a.fingerabdruck.len(), 64);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

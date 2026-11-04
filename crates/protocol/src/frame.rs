//! Rahmen: 4 Byte Länge (big-endian) + JSON.
//!
//! **Festlegung (SPEC §7):** JSON-Rahmen, längenpräfixiert, über einen
//! bidirektionalen QUIC-Strom. Bewusst kein Binärformat: ein Protokoll, das man
//! mit `tcpdump` und einem Texteditor lesen kann, ist im Betrieb mehr wert als
//! ein paar gesparte Bytes. Sprache geht als QUIC-Datagramm daneben, nicht
//! durch diesen Rahmen.

use thiserror::Error;

/// Version der Protokollsprache. Wird im ersten Rahmen beider Seiten geprüft.
pub const PROTOKOLL_VERSION: u16 = 1;

/// Obergrenze eines Rahmens (1 MiB). Schützt gegen Längenfelder, die nicht zur
/// Verbindung passen.
pub const MAX_RAHMEN: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum RahmenFehler {
    #[error("Rahmen zu groß: {0} Byte")]
    ZuGross(usize),
    #[error("unvollständig")]
    Unvollstaendig,
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Baut einen Rahmen: Länge (4 Byte, big-endian) + JSON-Nutzlast.
pub fn schreibe_rahmen<T: serde::Serialize>(wert: &T) -> Result<Vec<u8>, RahmenFehler> {
    let json = serde_json::to_vec(wert)?;
    if json.len() > MAX_RAHMEN {
        return Err(RahmenFehler::ZuGross(json.len()));
    }
    let mut aus = Vec::with_capacity(4 + json.len());
    aus.extend_from_slice(&(json.len() as u32).to_be_bytes());
    aus.extend_from_slice(&json);
    Ok(aus)
}

/// Liest einen Rahmen aus einem Puffer. `Ok(None)` heißt: noch unvollständig,
/// weitere Bytes abwarten. Bei Erfolg wird der verbrauchte Teil gemeldet.
pub fn lese_rahmen<T: serde::de::DeserializeOwned>(
    puffer: &[u8],
) -> Result<Option<(T, usize)>, RahmenFehler> {
    if puffer.len() < 4 {
        return Ok(None);
    }
    let laenge = u32::from_be_bytes([puffer[0], puffer[1], puffer[2], puffer[3]]) as usize;
    if laenge > MAX_RAHMEN {
        return Err(RahmenFehler::ZuGross(laenge));
    }
    if puffer.len() < 4 + laenge {
        return Ok(None);
    }
    let wert = serde_json::from_slice(&puffer[4..4 + laenge])?;
    Ok(Some((wert, 4 + laenge)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rahmen_ist_umkehrbar() {
        let roh = schreibe_rahmen(&"hallo").unwrap();
        // JSON von "hallo" ist `"hallo"` — sieben Byte mit Anführungszeichen.
        assert_eq!(&roh[..4], &7u32.to_be_bytes());
        let (wert, verbraucht): (String, usize) = lese_rahmen(&roh).unwrap().unwrap();
        assert_eq!(wert, "hallo");
        assert_eq!(verbraucht, roh.len());
    }

    #[test]
    fn unvollstaendig_ergibt_none() {
        let roh = schreibe_rahmen(&"hallo").unwrap();
        assert!(lese_rahmen::<String>(&roh[..3]).unwrap().is_none());
        assert!(lese_rahmen::<String>(&roh[..6]).unwrap().is_none());
    }

    #[test]
    fn zu_grosse_laenge_wird_abgelehnt() {
        let mut puffer = ((MAX_RAHMEN + 1) as u32).to_be_bytes().to_vec();
        puffer.extend_from_slice(&[0; 8]);
        assert!(matches!(
            lese_rahmen::<String>(&puffer),
            Err(RahmenFehler::ZuGross(_))
        ));
    }

    #[test]
    fn zwei_rahmen_hintereinander() {
        let mut puffer = schreibe_rahmen(&1u8).unwrap();
        puffer.extend_from_slice(&schreibe_rahmen(&2u8).unwrap());
        let (a, n1): (u8, usize) = lese_rahmen(&puffer).unwrap().unwrap();
        let (b, n2): (u8, usize) = lese_rahmen(&puffer[n1..]).unwrap().unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(n1 + n2, puffer.len());
    }
}

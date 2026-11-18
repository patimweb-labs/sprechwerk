//! Anhänge Ende zu Ende (ADR 0012 §3): Dateien in Blöcken verschlüsseln,
//! hoch- und herunterladen, Bereiche lesen.
//!
//! # Verfahren
//!
//! Für **jede** Datei ein eigener Zufallsschlüssel (32 Byte). Er steht im
//! Nachrichtenumschlag (`inhalt::Datei::schluessel`), den nur Mitglieder
//! öffnen — der Server bekommt ihn nie.
//!
//! Die Datei wird in Blöcken zu [`BLOCK`] Byte mit ChaCha20-Poly1305
//! verschlüsselt:
//!
//! ```text
//! Nonce  = Blocknummer (8 Byte, big-endian) ‖ 0 0 0 ‖ Schluss (1 = letzter Block)
//! AAD    = "sprechwerk anhang v1|" ‖ Kennung
//! Block  = ChaCha20-Poly1305(Schlüssel, Nonce, AAD, Klartext ≤ 64 KiB)
//! ```
//!
//! Der Schlüssel ist je Datei neu, deshalb genügt ein Zähler als Nonce. Die
//! **Schlussmarke** macht Kürzen sichtbar (der letzte vorhandene Block wäre
//! nicht als letzter verschlüsselt), die Blocknummer Umstellen, die Kennung
//! in den Zusatzdaten Vertauschen. Das ist der STREAM-Aufbau (Hoang, Reyhanitabar,
//! Rogaway, Vizár 2015), nicht selbst erfunden — nur ausgeschrieben.
//!
//! Weil jeder Block für sich prüfbar ist, lässt sich ein **Bereich** lesen,
//! ohne die ganze Datei zu entschlüsseln: so spielt ein Video, bevor die
//! letzte Minute geprüft ist, und keine Datei muss ganz in den Speicher.

use anyhow::{anyhow, bail, ensure, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sprechwerk_protocol::anhang::{
    bloecke, geheim_groesse, klar_groesse, Datenanfrage, Datenantwort, BLOCK, MARKE,
};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

/// Der Schlüssel einer Datei.
#[derive(Clone, PartialEq, Eq)]
pub struct Dateischluessel([u8; 32]);

impl std::fmt::Debug for Dateischluessel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dateischluessel(…)")
    }
}

impl Dateischluessel {
    /// Ein neuer Zufallsschlüssel.
    pub fn neu() -> Self {
        let mut b = [0u8; 32];
        getrandom::getrandom(&mut b).expect("Zufall aus dem Betriebssystem");
        Self(b)
    }

    /// Aus der Form im Umschlag (64 Hexzeichen).
    pub fn aus_hex(text: &str) -> Option<Self> {
        let roh = hex::decode(text).ok()?;
        Some(Self(roh.try_into().ok()?))
    }

    /// Für den Umschlag.
    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }

    fn chiffre(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.0))
    }
}

/// Eine neue Kennung für eine Datei (16 Byte Zufall, Hex).
pub fn neue_id() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("Zufall aus dem Betriebssystem");
    hex::encode(b)
}

fn nonce(nummer: u64, letzter: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&nummer.to_be_bytes());
    n[11] = letzter as u8;
    n
}

fn zusatz(id: &str) -> Vec<u8> {
    format!("sprechwerk anhang v1|{id}").into_bytes()
}

/// Verschlüsselt Block `nummer` einer Datei mit `klar_gesamt` Byte.
pub fn block_verschluesseln(
    s: &Dateischluessel,
    id: &str,
    nummer: u64,
    klar_gesamt: u64,
    klar: &[u8],
) -> Vec<u8> {
    let letzter = nummer + 1 == bloecke(klar_gesamt);
    s.chiffre()
        .encrypt(
            Nonce::from_slice(&nonce(nummer, letzter)),
            Payload {
                msg: klar,
                aad: &zusatz(id),
            },
        )
        .expect("ChaCha20-Poly1305 verschlüsselt jede Länge")
}

/// Entschlüsselt Block `nummer`. Scheitert, wenn er verändert, vertauscht,
/// umgestellt oder fälschlich (nicht) der letzte ist.
pub fn block_entschluesseln(
    s: &Dateischluessel,
    id: &str,
    nummer: u64,
    klar_gesamt: u64,
    geheim: &[u8],
) -> Result<Vec<u8>> {
    let letzter = nummer + 1 == bloecke(klar_gesamt);
    s.chiffre()
        .decrypt(
            Nonce::from_slice(&nonce(nummer, letzter)),
            Payload {
                msg: geheim,
                aad: &zusatz(id),
            },
        )
        .map_err(|_| anyhow!("Block {nummer} ist verändert"))
}

/// Länge des Klartexts in Block `nummer`.
fn klar_im_block(klar_gesamt: u64, nummer: u64) -> u64 {
    (klar_gesamt - nummer * BLOCK).min(BLOCK)
}

// ---- Leitung ------------------------------------------------------------

async fn rahmen_schreiben(send: &mut quinn::SendStream, wert: &Datenanfrage) -> Result<()> {
    let bytes = sprechwerk_protocol::schreibe_rahmen(wert)?;
    send.write_all(&bytes).await?;
    Ok(())
}

async fn antwort_lesen(recv: &mut quinn::RecvStream) -> Result<Datenantwort> {
    let mut laenge = [0u8; 4];
    recv.read_exact(&mut laenge).await?;
    let laenge = u32::from_be_bytes(laenge) as usize;
    ensure!(laenge <= 64 * 1024, "Antwort zu groß");
    let mut json = vec![0u8; laenge];
    recv.read_exact(&mut json).await?;
    Ok(serde_json::from_slice(&json)?)
}

fn als_fehler(antwort: Datenantwort) -> anyhow::Error {
    match antwort {
        Datenantwort::Fehler { text, .. } => anyhow!(text),
        anderes => anyhow!("unerwartete Antwort: {anderes:?}"),
    }
}

/// Lädt `klar_gesamt` Byte aus `quelle` verschlüsselt hoch.
///
/// `fortschritt` bekommt die gesendeten Klartext-Byte (nach jedem Block).
/// Der Strom hat **niedrigere Vorrangstufe** als der Steuerstrom: ein Text
/// soll nicht hinter einem Video warten.
pub async fn hochladen(
    leitung: &quinn::Connection,
    id: &str,
    s: &Dateischluessel,
    mut quelle: impl AsyncRead + Unpin,
    klar_gesamt: u64,
    mut fortschritt: impl FnMut(u64),
) -> Result<()> {
    let (mut send, mut recv) = leitung.open_bi().await.context("Datenstrom")?;
    let _ = send.set_priority(-1);
    rahmen_schreiben(
        &mut send,
        &Datenanfrage::Hochladen {
            id: id.to_string(),
            groesse: geheim_groesse(klar_gesamt),
        },
    )
    .await?;
    let mut puffer = vec![0u8; BLOCK as usize];
    let mut gesendet = 0u64;
    for nummer in 0..bloecke(klar_gesamt) {
        let n = klar_im_block(klar_gesamt, nummer) as usize;
        quelle
            .read_exact(&mut puffer[..n])
            .await
            .context("Datei lesen (hat sie sich geändert?)")?;
        let block = block_verschluesseln(s, id, nummer, klar_gesamt, &puffer[..n]);
        // Lehnt der Server ab, schließt er den Strom: dann steht der Grund
        // in seiner Antwort, nicht im Schreibfehler.
        if send.write_all(&block).await.is_err() {
            return Err(als_fehler(antwort_lesen(&mut recv).await?));
        }
        gesendet += n as u64;
        fortschritt(gesendet);
    }
    send.finish()?;
    match antwort_lesen(&mut recv).await? {
        Datenantwort::Fertig => Ok(()),
        anderes => Err(als_fehler(anderes)),
    }
}

/// Lädt den **Geheimtext** einer Datei nach `ziel` (erst `ziel.teil`, am Ende
/// umbenannt). Entschlüsselt wird beim Lesen ([`Geheimdatei`]) — so liegt auf
/// der Platte nie Klartext, der nicht gewollt ist.
///
/// `fortschritt` bekommt die empfangenen Byte und die Gesamtgröße.
pub async fn herunterladen(
    leitung: &quinn::Connection,
    id: &str,
    ziel: &Path,
    mut fortschritt: impl FnMut(u64, u64),
) -> Result<()> {
    let (mut send, mut recv) = leitung.open_bi().await.context("Datenstrom")?;
    let _ = send.set_priority(-1);
    rahmen_schreiben(
        &mut send,
        &Datenanfrage::Herunterladen { id: id.to_string() },
    )
    .await?;
    send.finish()?;
    let groesse = match antwort_lesen(&mut recv).await? {
        Datenantwort::Daten { groesse } => groesse,
        anderes => return Err(als_fehler(anderes)),
    };
    ensure!(
        klar_groesse(groesse).is_some(),
        "Größe passt zu keiner Datei"
    );
    if let Some(eltern) = ziel.parent() {
        tokio::fs::create_dir_all(eltern).await?;
    }
    let teil = ziel.with_extension("teil");
    let mut datei = tokio::fs::File::create(&teil).await?;
    let mut puffer = vec![0u8; 256 * 1024];
    let mut empfangen = 0u64;
    while empfangen < groesse {
        let n = recv
            .read(&mut puffer)
            .await?
            .ok_or_else(|| anyhow!("Strom endet nach {empfangen} von {groesse} Byte"))?;
        let n = n.min((groesse - empfangen) as usize);
        datei.write_all(&puffer[..n]).await?;
        empfangen += n as u64;
        fortschritt(empfangen, groesse);
    }
    datei.flush().await?;
    drop(datei);
    tokio::fs::rename(&teil, ziel).await?;
    Ok(())
}

// ---- Lesen --------------------------------------------------------------

/// Eine heruntergeladene, noch verschlüsselte Datei, aus der sich Bereiche
/// entschlüsselt lesen lassen.
pub struct Geheimdatei {
    datei: std::fs::File,
    schluessel: Dateischluessel,
    id: String,
    klar: u64,
}

impl Geheimdatei {
    pub fn oeffnen(pfad: &Path, schluessel: Dateischluessel, id: &str) -> Result<Self> {
        let datei = std::fs::File::open(pfad)?;
        let laenge = datei.metadata()?.len();
        let klar = klar_groesse(laenge).ok_or_else(|| anyhow!("Größe passt zu keiner Datei"))?;
        Ok(Self {
            datei,
            schluessel,
            id: id.to_string(),
            klar,
        })
    }

    /// Größe des Klartexts.
    pub fn groesse(&self) -> u64 {
        self.klar
    }

    fn block(&mut self, nummer: u64) -> Result<Vec<u8>> {
        let laenge = klar_im_block(self.klar, nummer) + MARKE;
        self.datei.seek(SeekFrom::Start(nummer * (BLOCK + MARKE)))?;
        let mut geheim = vec![0u8; laenge as usize];
        self.datei.read_exact(&mut geheim)?;
        block_entschluesseln(&self.schluessel, &self.id, nummer, self.klar, &geheim)
    }

    /// Klartext von `von` (einschließlich) bis `bis` (ausschließlich).
    pub fn lesen(&mut self, von: u64, bis: u64) -> Result<Vec<u8>> {
        let bis = bis.min(self.klar);
        if von >= bis {
            return Ok(Vec::new());
        }
        let mut aus = Vec::with_capacity((bis - von) as usize);
        for nummer in von / BLOCK..=(bis - 1) / BLOCK {
            let block = self.block(nummer)?;
            let anfang = nummer * BLOCK;
            let a = von.saturating_sub(anfang) as usize;
            let e = ((bis - anfang) as usize).min(block.len());
            aus.extend_from_slice(&block[a..e]);
        }
        Ok(aus)
    }

    /// Schreibt den ganzen Klartext nach `ziel` — Block für Block. Scheitert
    /// ein Block, ist die Datei verändert; der Aufrufer verwirft das Ziel.
    pub fn entschluesseln_nach(&mut self, ziel: &mut impl Write) -> Result<()> {
        for nummer in 0..bloecke(self.klar) {
            ziel.write_all(&self.block(nummer)?)?;
        }
        ziel.flush()?;
        Ok(())
    }
}

/// Verschlüsselt einen Klartext ganz im Speicher (für Prüfungen und kleine
/// Dateien).
pub fn verschluesseln(s: &Dateischluessel, id: &str, klar: &[u8]) -> Vec<u8> {
    let gesamt = klar.len() as u64;
    let mut aus = Vec::with_capacity(geheim_groesse(gesamt) as usize);
    for nummer in 0..bloecke(gesamt) {
        let a = (nummer * BLOCK) as usize;
        let e = a + klar_im_block(gesamt, nummer) as usize;
        aus.extend(block_verschluesseln(s, id, nummer, gesamt, &klar[a..e]));
    }
    aus
}

/// Medientyp aus der Dateiendung. Nur was hier steht, zeigt das Fenster als
/// Bild oder Video — alles andere ist „Datei".
pub fn typ_aus_name(name: &str) -> &'static str {
    let endung = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match endung.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Prüft, dass eine fertig geladene Geheimdatei vollständig und unverändert
/// ist (alle Blöcke).
pub fn pruefen(pfad: &Path, schluessel: Dateischluessel, id: &str) -> Result<()> {
    let mut d = Geheimdatei::oeffnen(pfad, schluessel, id)?;
    let mut nichts = std::io::sink();
    d.entschluesseln_nach(&mut nichts)
}

/// Wie [`Geheimdatei::entschluesseln_nach`], aber in eine Datei — und bei
/// einem Fehler bleibt kein halber Klartext liegen.
pub fn entschluesseln_in_datei(
    quelle: &Path,
    schluessel: Dateischluessel,
    id: &str,
    ziel: &Path,
) -> Result<()> {
    let mut d = Geheimdatei::oeffnen(quelle, schluessel, id)?;
    let mut aus = std::io::BufWriter::new(std::fs::File::create(ziel)?);
    if let Err(fehler) = d.entschluesseln_nach(&mut aus) {
        drop(aus);
        let _ = std::fs::remove_file(ziel);
        bail!(fehler);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datei(name: &str, inhalt: &[u8]) -> std::path::PathBuf {
        let p =
            std::env::temp_dir().join(format!("sprechwerk-anhang-{name}-{}", std::process::id()));
        std::fs::write(&p, inhalt).unwrap();
        p
    }

    fn klartext(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn umkehrbar_in_allen_groessen() {
        let s = Dateischluessel::neu();
        let id = neue_id();
        for n in [
            0,
            1,
            BLOCK as usize - 1,
            BLOCK as usize,
            BLOCK as usize + 1,
            200_000,
        ] {
            let klar = klartext(n);
            let geheim = verschluesseln(&s, &id, &klar);
            assert_eq!(geheim.len() as u64, geheim_groesse(n as u64));
            let p = datei(&format!("um{n}"), &geheim);
            let mut d = Geheimdatei::oeffnen(&p, s.clone(), &id).unwrap();
            assert_eq!(d.groesse(), n as u64);
            let mut aus = Vec::new();
            d.entschluesseln_nach(&mut aus).unwrap();
            assert_eq!(aus, klar, "n = {n}");
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn bereiche_ueber_blockgrenzen() {
        let s = Dateischluessel::neu();
        let id = neue_id();
        let klar = klartext(3 * BLOCK as usize + 100);
        let p = datei("bereich", &verschluesseln(&s, &id, &klar));
        let mut d = Geheimdatei::oeffnen(&p, s, &id).unwrap();
        let b = BLOCK;
        for (von, bis) in [
            (0, 10),
            (b - 5, b + 5),
            (b, 2 * b),
            (10, 3 * b + 100),
            (3 * b + 50, 9 * b), // über das Ende: gekappt
        ] {
            let ende = bis.min(klar.len() as u64);
            assert_eq!(
                d.lesen(von, bis).unwrap(),
                klar[von as usize..ende as usize],
                "{von}..{bis}"
            );
        }
        assert!(d.lesen(5, 5).unwrap().is_empty());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn veraendert_gekuerzt_vertauscht_faellt_auf() {
        let s = Dateischluessel::neu();
        let id = neue_id();
        let klar = klartext(2 * BLOCK as usize + 10);
        let geheim = verschluesseln(&s, &id, &klar);

        // Ein Bit gekippt.
        let mut kaputt = geheim.clone();
        kaputt[100] ^= 1;
        let p = datei("bit", &kaputt);
        assert!(pruefen(&p, s.clone(), &id).is_err());

        // Um den letzten Block gekürzt: Größe passt noch, Schlussmarke nicht.
        let kurz = &geheim[..2 * (BLOCK + MARKE) as usize];
        let p2 = datei("kurz", kurz);
        assert!(pruefen(&p2, s.clone(), &id).is_err());

        // Andere Kennung (vom Server vertauscht).
        let p3 = datei("tausch", &geheim);
        assert!(pruefen(&p3, s.clone(), &neue_id()).is_err());
        // Anderer Schlüssel.
        assert!(pruefen(&p3, Dateischluessel::neu(), &id).is_err());
        // Richtig: geht.
        assert!(pruefen(&p3, s.clone(), &id).is_ok());

        // Entschlüsseln in eine Datei lässt bei Fehler nichts liegen.
        let ziel =
            std::env::temp_dir().join(format!("sprechwerk-anhang-ziel-{}", std::process::id()));
        assert!(entschluesseln_in_datei(&p, s.clone(), &id, &ziel).is_err());
        assert!(!ziel.exists());
        for p in [p, p2, p3] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn schluessel_in_hex() {
        let s = Dateischluessel::neu();
        assert_eq!(Dateischluessel::aus_hex(&s.hex()), Some(s));
        assert!(Dateischluessel::aus_hex("ab").is_none());
        assert!(Dateischluessel::aus_hex("zz").is_none());
        assert!(sprechwerk_protocol::anhang::ist_id(&neue_id()));
    }

    #[test]
    fn typen() {
        assert_eq!(typ_aus_name("Fest.JPG"), "image/jpeg");
        assert_eq!(typ_aus_name("clip.webm"), "video/webm");
        assert_eq!(typ_aus_name("bild.svg"), "application/octet-stream");
        assert_eq!(typ_aus_name("ohne"), "application/octet-stream");
    }
}

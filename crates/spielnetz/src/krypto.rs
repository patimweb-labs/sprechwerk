//! Ende-zu-Ende-Verschlüsselung zwischen Gastgeber und Teilnehmer (ADR 0016 §5).
//!
//! # Warum der Server nicht mitliest
//!
//! Stufe 1 leitet allen Spielverkehr über den Server (damit niemand die
//! öffentliche Adresse eines anderen sieht). Damit der Betreiber trotzdem nicht
//! mitlesen kann, verschlüsseln die beiden Enden selbst — mit einem Schlüssel,
//! den nur sie kennen.
//!
//! # Die Ableitung, genau
//!
//! ```text
//! geteilt = X25519(eigener_static_secret, oeffentlich_aus_kennung(gegenueber))
//! prk     = HKDF-SHA256-Extract(Salz = angebot als u32 big endian, ikm = geteilt)
//! ```
//!
//! Die Angebotsnummer im Salz bindet das Geheimnis an **diese** Freigabe:
//! dasselbe Paar, dasselbe Spiel, aber eine neue Freigabe ergibt andere
//! Schlüssel. Wer die Nummer nicht kennt, kann nicht mitrechnen; wer sie kennt
//! (der Server), hat daraus noch kein Geheimnis — dafür bräuchte er einen der
//! beiden Geheimschlüssel.
//!
//! Aus `prk` entstehen die Richtungsschlüssel. **Jede Seite bringt eigenen
//! Zufall mit**, sonst ergäbe ein zweites Beitreten zur selben Freigabe
//! dieselben Schlüssel mit Zählern ab 0 — und ein umgebauter Server könnte
//! durch Wiedereinspielen eines alten Salzes erzwingen, dass eine Seite Nonces
//! wiederholt (bei ChaCha20 fällt dann Klartext heraus).
//!
//! ```text
//! TCP  hin : HKDF-SHA256-Expand(HKDF-SHA256-Extract(
//!                               Salz = salz_teilnehmer[16] ++ salz_gastgeber[16],
//!                               ikm  = prk),
//!                               Info = "sprechwerk-spielnetz/1 tcp hin")
//! TCP  her : … "… tcp her"
//! UDP  hin : HKDF-SHA256-Expand(HKDF-SHA256-Extract(Salz = sitzung[8], ikm = prk),
//!                               Info = "sprechwerk-spielnetz/1 udp hin")
//! UDP  her : … "… udp her"
//! ```
//!
//! „hin“ ist immer Teilnehmer → Gastgeber, „her“ Gastgeber → Teilnehmer. Jede
//! Seite versiegelt mit ihrer Richtung und öffnet die andere; dadurch können
//! sich die beiden Richtungen nie denselben Nonce teilen.
//!
//! **TCP: beide Salze.** Jede Seite zieht 16 Zufallsbytes ([`strom_salz`]) und
//! schickt sie als erste Bytes **ihrer** Richtung. In die Ableitung gehen beide
//! ein, also hängt der Schlüssel jeder Richtung an beiden Seiten: spielt ein
//! umgebauter Server dem Gastgeber ein altes Teilnehmersalz ein, ist der
//! Schlüssel trotzdem ein anderer, weil das Salz des Gastgebers neu ist. Der
//! Teilnehmer sendet Nutzdaten erst, wenn das Salz des Gastgebers da ist (eine
//! Laufzeit mehr beim Verbindungsaufbau).
//!
//! **UDP: Sitzung je Absender.** Jeder Absender wählt bei der Erzeugung seines
//! Siegels 8 Zufallsbytes als Sitzung ([`SITZUNG_LAENGE`]); sie stehen jedem
//! Datagramm voran, und der Richtungsschlüssel entsteht aus Paarschlüssel und
//! Sitzung. Der Öffner führt die aktuelle Sitzung und eine kurze Liste
//! abgelöster: ein Datagramm unbekannter Sitzung wird erst entschlüsselt und
//! **nur bei Erfolg** übernommen; Datagramme abgelöster Sitzungen werden
//! verworfen ([`Kryptofehler::Abgeloest`]).
//!
//! # Auf der Leitung
//!
//! ```text
//! TCP-Strom : [salz 16][u16 BE Länge des Geheimtexts][Geheimtext] …  (Blöcke ≤ 16 KiB)
//! UDP       : [sitzung 8][u64 BE Zähler][Geheimtext]
//! ```
//!
//! Das Salz steht nur einmal am Anfang eines Stroms (die erste Nachricht jeder
//! Richtung), der Rest ist Blockfolge. Nonce ist überall
//! `[0u8; 4] ++ Zähler als u64 BE`. Zähler und Sitzung stehen bei UDP im
//! Klartext davor (der Empfänger muss sie lesen können, um Wiederholungen zu
//! erkennen), bei TCP ist der Zähler implizit die Blocknummer.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use x25519_dalek::StaticSecret;

use crate::protokoll::AngebotId;
use sprechwerk_adapters::verschluesselt::oeffentlich_aus_kennung;

/// Größter Klartextblock eines TCP-Stroms.
pub const STROM_BLOCK: usize = 16 * 1024;

/// Länge des Salzes je TCP-Strom.
pub const STROM_SALZ_LAENGE: usize = 16;

/// Aufschlag durch die Echtheitsprüfung (Poly1305).
pub const AUFSCHLAG: usize = 16;

/// Länge des Zählers vor einem UDP-Geheimtext.
pub const ZAEHLER_LAENGE: usize = 8;

/// Länge der Sitzungskennung vor jedem UDP-Datagramm.
pub const SITZUNG_LAENGE: usize = 8;

/// So viele abgelöste UDP-Sitzungen merkt sich der Öffner.
pub const ABGELOEST_MAX: usize = 8;

/// So viele Zähler merkt sich der Wiederholschutz (Bitmaske wie IPsec).
pub const FENSTER: u64 = 64;

const INFO_TCP_HIN: &[u8] = b"sprechwerk-spielnetz/1 tcp hin";
const INFO_TCP_HER: &[u8] = b"sprechwerk-spielnetz/1 tcp her";
const INFO_UDP_HIN: &[u8] = b"sprechwerk-spielnetz/1 udp hin";
const INFO_UDP_HER: &[u8] = b"sprechwerk-spielnetz/1 udp her";

/// Welche Seite eines Paares man ist. Bestimmt, welche Richtung versiegelt und
/// welche geöffnet wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rolle {
    Gastgeber,
    Teilnehmer,
}

/// Was beim Öffnen schiefgehen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Kryptofehler {
    /// Die Prüfsumme passt nicht: falscher Schlüssel oder verändertes Byte.
    #[error("Geheimtext manipuliert")]
    Manipuliert,
    /// Diesen Zähler haben wir schon gesehen.
    #[error("Datagramm wiederholt")]
    Wiederholt,
    /// Der Zähler liegt hinter dem Wiederholfenster.
    #[error("Datagramm zu alt")]
    ZuAlt,
    /// Das Datagramm gehört zu einer abgelösten Sitzung (zweites Beitreten).
    #[error("Datagramm einer abgelösten Sitzung")]
    Abgeloest,
    /// Zu wenig Bytes für Sitzung, Zähler oder Prüfsumme.
    #[error("Datagramm zu kurz")]
    ZuKurz,
}

/// Das gemeinsame Geheimnis eines Paares für **ein** Angebot.
#[derive(Clone)]
pub struct Paar {
    prk: [u8; 32],
}

impl std::fmt::Debug for Paar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Schlüsselmaterial gehört in kein Protokoll.
        f.debug_struct("Paar").finish_non_exhaustive()
    }
}

impl Paar {
    /// Leitet das Geheimnis aus dem eigenen Austauschschlüssel und der Kennung
    /// des Gegenübers ab.
    ///
    /// `None`, wenn die Kennung kein gültiger Ed25519-Punkt ist — dann gibt es
    /// niemanden, mit dem man sprechen könnte.
    pub fn neu(eigen: &StaticSecret, gegenueber_kennung: &str, angebot: AngebotId) -> Option<Self> {
        let gegenueber = oeffentlich_aus_kennung(gegenueber_kennung)?;
        let geteilt = eigen.diffie_hellman(&gegenueber);
        // Extract mit der Angebotsnummer als Salz: bindet das Geheimnis an die
        // Freigabe, ohne dass die Nummer selbst geheim sein muss.
        let (prk, _) = Hkdf::<Sha256>::extract(Some(&angebot.to_be_bytes()), geteilt.as_bytes());
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&prk);
        Some(Self { prk: bytes })
    }

    /// Schlüssel für **einen** TCP-Strom.
    ///
    /// Beide Seiten stecken ihre eigenen 16 Zufallsbytes ([`strom_salz`]) in die
    /// Ableitung; jede schickt ihr Salz als erste Bytes ihrer Richtung. Damit
    /// hängt der Schlüssel an beiden Seiten, und ein wiedereingespieltes Salz
    /// einer Seite allein genügt nicht, um dieselben Nonces zu erzwingen.
    pub fn strom(
        &self,
        salz_teilnehmer: &[u8; STROM_SALZ_LAENGE],
        salz_gastgeber: &[u8; STROM_SALZ_LAENGE],
        rolle: Rolle,
    ) -> (Stromsiegel, Stromoeffner) {
        let salze = [salz_teilnehmer.as_slice(), salz_gastgeber.as_slice()].concat();
        let hin = ableiten(&self.prk, Some(salze.as_slice()), INFO_TCP_HIN);
        let her = ableiten(&self.prk, Some(salze.as_slice()), INFO_TCP_HER);
        match rolle {
            Rolle::Gastgeber => (Stromsiegel::neu(her), Stromoeffner::neu(hin)),
            Rolle::Teilnehmer => (Stromsiegel::neu(hin), Stromoeffner::neu(her)),
        }
    }

    /// Schlüssel für die Datagramme dieses Paares.
    ///
    /// Das Siegel wählt dabei 8 Zufallsbytes als Sitzung: ein zweites Beitreten
    /// zur selben Freigabe (neues [`Paar::datagramme`]) ergibt also andere
    /// Schlüssel und Zähler ab 0 auf neuen Nonces. Der Öffner erfährt die
    /// Sitzung des Gegenübers aus dessen erstem Datagramm.
    pub fn datagramme(&self, rolle: Rolle) -> (Datagrammsiegel, Datagrammoeffner) {
        let sitzung = neue_sitzung();
        match rolle {
            Rolle::Gastgeber => (
                Datagrammsiegel::neu(&self.prk, INFO_UDP_HER, sitzung),
                Datagrammoeffner::neu(&self.prk, INFO_UDP_HIN),
            ),
            Rolle::Teilnehmer => (
                Datagrammsiegel::neu(&self.prk, INFO_UDP_HIN, sitzung),
                Datagrammoeffner::neu(&self.prk, INFO_UDP_HER),
            ),
        }
    }
}

/// 16 Zufallsbytes als Salz für einen TCP-Strom.
///
/// Jede Seite zieht eins und schickt es als erste Bytes ihrer Richtung; beide
/// Salze gehen in die Ableitung ein.
pub fn strom_salz() -> [u8; STROM_SALZ_LAENGE] {
    let mut salz = [0u8; STROM_SALZ_LAENGE];
    getrandom::getrandom(&mut salz).expect("Zufall aus dem Betriebssystem");
    salz
}

/// 8 Zufallsbytes als Sitzungskennung eines UDP-Absenders.
fn neue_sitzung() -> [u8; SITZUNG_LAENGE] {
    let mut sitzung = [0u8; SITZUNG_LAENGE];
    getrandom::getrandom(&mut sitzung).expect("Zufall aus dem Betriebssystem");
    sitzung
}

/// HKDF-Expand aus dem gespeicherten `prk`, mit optionalem Salz.
fn ableiten(prk: &[u8; 32], salz: Option<&[u8]>, info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(salz, prk);
    let mut schluessel = [0u8; 32];
    // 32 Byte liegen weit unter der Höchstlänge von HKDF-SHA256 (255 × 32).
    hk.expand(info, &mut schluessel)
        .expect("HKDF-Expand: 32 Byte passen immer");
    schluessel
}

/// Nonce aus einem Zähler: 4 Nullbytes, dann der Zähler big endian.
fn nonce(zaehler: u64) -> Nonce {
    let mut roh = [0u8; 12];
    roh[4..].copy_from_slice(&zaehler.to_be_bytes());
    *Nonce::from_slice(&roh)
}

/// Versiegelt einen TCP-Strom: Blöcke mit Längenpräfix.
pub struct Stromsiegel {
    chiffre: ChaCha20Poly1305,
    zaehler: u64,
}

impl Stromsiegel {
    fn neu(schluessel: [u8; 32]) -> Self {
        Self {
            chiffre: ChaCha20Poly1305::new(Key::from_slice(&schluessel)),
            zaehler: 0,
        }
    }

    /// Versiegelt beliebig viel Klartext in Blöcke zu höchstens [`STROM_BLOCK`].
    ///
    /// Leerer Klartext ergibt keine Bytes — es gibt nichts zu sagen, und ein
    /// leerer Block wäre nur Aufschlag.
    pub fn versiegeln(&mut self, klartext: &[u8]) -> Vec<u8> {
        let mut aus = Vec::with_capacity(klartext.len() + AUFSCHLAG + 2);
        for block in klartext.chunks(STROM_BLOCK) {
            let geheim = self
                .chiffre
                .encrypt(&nonce(self.zaehler), block)
                .expect("ChaCha20-Poly1305 versiegelt immer");
            aus.extend_from_slice(&(geheim.len() as u16).to_be_bytes());
            aus.extend_from_slice(&geheim);
            // Kein Überlauf in Menschenzeit; `wrapping_add` macht daraus kein
            // Panik-Risiko in einer Debug-Build-Prüfung.
            self.zaehler = self.zaehler.wrapping_add(1);
        }
        aus
    }
}

/// Öffnet einen TCP-Strom, auch wenn die Bytes zerstückelt ankommen.
pub struct Stromoeffner {
    chiffre: ChaCha20Poly1305,
    zaehler: u64,
    /// Noch nicht ausgewertete Bytes (halber Längenkopf, halber Block).
    puffer: Vec<u8>,
}

impl Stromoeffner {
    fn neu(schluessel: [u8; 32]) -> Self {
        Self {
            chiffre: ChaCha20Poly1305::new(Key::from_slice(&schluessel)),
            zaehler: 0,
            puffer: Vec::new(),
        }
    }

    /// Nimmt Bytes an und gibt den Klartext aller **vollständigen** Blöcke
    /// zurück. Ein halber Block bleibt liegen und kommt beim nächsten Aufruf.
    pub fn oeffnen(&mut self, eingang: &[u8]) -> Result<Vec<u8>, Kryptofehler> {
        self.puffer.extend_from_slice(eingang);
        let mut klar = Vec::new();
        loop {
            if self.puffer.len() < 2 {
                break;
            }
            let laenge = u16::from_be_bytes([self.puffer[0], self.puffer[1]]) as usize;
            if self.puffer.len() < 2 + laenge {
                break;
            }
            let block = self
                .chiffre
                .decrypt(&nonce(self.zaehler), &self.puffer[2..2 + laenge]);
            // Der Puffer wird in jedem Fall verbraucht: ein manipulierter Block
            // beendet den Strom, und ein erneuter Versuch mit denselben Bytes
            // würde nur denselben Fehler ergeben.
            self.puffer.drain(..2 + laenge);
            let block = block.map_err(|_| Kryptofehler::Manipuliert)?;
            klar.extend_from_slice(&block);
            self.zaehler = self.zaehler.wrapping_add(1);
        }
        Ok(klar)
    }
}

/// Versiegelt UDP-Datagramme: `[sitzung][zähler][geheimtext]`.
///
/// Die Sitzung ist der eigene Zufall dieses Siegels. Sie steht im Klartext
/// davor, damit der Empfänger weiß, mit welchem Schlüssel er rechnen muss; sie
/// ist kein Geheimnis, sondern der Grund, warum zwei Beitritte zu derselben
/// Freigabe nie dieselben Nonces ergeben.
pub struct Datagrammsiegel {
    chiffre: ChaCha20Poly1305,
    sitzung: [u8; SITZUNG_LAENGE],
    zaehler: u64,
}

impl Datagrammsiegel {
    fn neu(prk: &[u8; 32], info: &[u8], sitzung: [u8; SITZUNG_LAENGE]) -> Self {
        Self {
            chiffre: ChaCha20Poly1305::new(Key::from_slice(&ableiten(
                prk,
                Some(sitzung.as_slice()),
                info,
            ))),
            sitzung,
            zaehler: 0,
        }
    }

    /// Die Sitzungskennung, die jedem Datagramm voransteht.
    pub fn sitzung(&self) -> [u8; SITZUNG_LAENGE] {
        self.sitzung
    }

    /// Versiegelt ein Datagramm und zählt hoch.
    pub fn versiegeln(&mut self, klartext: &[u8]) -> Vec<u8> {
        let geheim = self
            .chiffre
            .encrypt(&nonce(self.zaehler), klartext)
            .expect("ChaCha20-Poly1305 versiegelt immer");
        let mut aus = Vec::with_capacity(SITZUNG_LAENGE + ZAEHLER_LAENGE + geheim.len());
        aus.extend_from_slice(&self.sitzung);
        aus.extend_from_slice(&self.zaehler.to_be_bytes());
        aus.extend_from_slice(&geheim);
        self.zaehler = self.zaehler.wrapping_add(1);
        aus
    }
}

/// Eine Sitzung aus Sicht des Öffners: Kennung, Chiffre und Wiederholfenster.
struct Sitzung {
    kennung: [u8; SITZUNG_LAENGE],
    chiffre: ChaCha20Poly1305,
    /// Höchster bisher angenommener Zähler.
    hoechste: u64,
    /// Bitmaske der gesehenen Zähler, Bit 0 = `hoechste`.
    fenster: u64,
    /// Ob überhaupt schon eins angenommen wurde (dann gilt `hoechste`).
    gesehen: bool,
}

impl Sitzung {
    fn neu(prk: &[u8; 32], info: &[u8], kennung: [u8; SITZUNG_LAENGE]) -> Self {
        Self {
            chiffre: ChaCha20Poly1305::new(Key::from_slice(&ableiten(
                prk,
                Some(kennung.as_slice()),
                info,
            ))),
            kennung,
            hoechste: 0,
            fenster: 0,
            gesehen: false,
        }
    }

    /// Prüft nur lesend, ob dieser Zähler zugelassen wäre.
    fn pruefe(&self, zaehler: u64) -> Result<(), Kryptofehler> {
        if !self.gesehen || zaehler > self.hoechste {
            return Ok(());
        }
        let abstand = self.hoechste - zaehler;
        if abstand >= FENSTER {
            return Err(Kryptofehler::ZuAlt);
        }
        if self.fenster & (1u64 << abstand) != 0 {
            return Err(Kryptofehler::Wiederholt);
        }
        Ok(())
    }

    /// Schiebt das Fenster fort — erst nach erfolgreicher Prüfsumme.
    fn fortschreiben(&mut self, zaehler: u64) {
        if !self.gesehen {
            self.gesehen = true;
            self.hoechste = zaehler;
            self.fenster = 1;
        } else if zaehler > self.hoechste {
            let abstand = zaehler - self.hoechste;
            self.fenster = if abstand >= FENSTER {
                0
            } else {
                self.fenster << abstand
            };
            self.fenster |= 1;
            self.hoechste = zaehler;
        } else {
            let abstand = self.hoechste - zaehler;
            self.fenster |= 1u64 << abstand;
        }
    }
}

/// Öffnet UDP-Datagramme, verwirft Wiederholungen (Gleitfenster wie IPsec) und
/// hält Schritt, wenn der Absender die Sitzung wechselt.
///
/// Das Fenster sind die letzten [`FENSTER`] Zählerstände einschließlich des
/// höchsten, als Bitmaske: Bit 0 ist der höchste, Bit 63 der älteste.
/// **Verschoben wird erst nach erfolgreicher Entschlüsselung** — sonst könnte
/// ein gefälschtes Datagramm mit hoher Nummer echte Pakete aussperren. Dasselbe
/// gilt für den Sitzungswechsel: eine unbekannte Sitzung wird erst zum
/// Kandidaten, dann entschlüsselt, und nur bei gültiger Prüfsumme übernommen.
pub struct Datagrammoeffner {
    prk: [u8; 32],
    /// Info der Richtung, die geöffnet wird.
    info: &'static [u8],
    /// Die Sitzung, die gerade gilt. `None`, bis das erste Datagramm da ist.
    aktuelle: Option<Sitzung>,
    /// Kennungen abgelöster Sitzungen, älteste zuerst. Höchstens
    /// [`ABGELOEST_MAX`] Einträge; fällt eine heraus, gilt sie wieder als
    /// unbekannt (Restrisiko aus ADR 0016 §5, siehe Test
    /// `hoechstens_acht_sitzungen_bleiben_abgeloest`).
    abgeloest: Vec<[u8; SITZUNG_LAENGE]>,
}

impl Datagrammoeffner {
    fn neu(prk: &[u8; 32], info: &'static [u8]) -> Self {
        Self {
            prk: *prk,
            info,
            aktuelle: None,
            abgeloest: Vec::new(),
        }
    }

    /// Öffnet ein Datagramm. Reihenfolge: Länge, dann — bei bekannter Sitzung —
    /// Wiederholung/Alter (nur lesend), dann die Prüfsumme, zuletzt der Zustand.
    /// Eine unbekannte Sitzung wird erst nach geglückter Entschlüsselung
    /// übernommen; eine früher schon abgelöste bekommt [`Kryptofehler::Abgeloest`].
    pub fn oeffnen(&mut self, d: &[u8]) -> Result<Vec<u8>, Kryptofehler> {
        const KOPF: usize = SITZUNG_LAENGE + ZAEHLER_LAENGE;
        if d.len() < KOPF + AUFSCHLAG {
            return Err(Kryptofehler::ZuKurz);
        }
        let mut kennung = [0u8; SITZUNG_LAENGE];
        kennung.copy_from_slice(&d[..SITZUNG_LAENGE]);
        let mut roh = [0u8; ZAEHLER_LAENGE];
        roh.copy_from_slice(&d[SITZUNG_LAENGE..KOPF]);
        let zaehler = u64::from_be_bytes(roh);
        let geheim = &d[KOPF..];

        if let Some(aktuelle) = self.aktuelle.as_mut() {
            if aktuelle.kennung == kennung {
                // Erst schauen, dann rechnen: ein wiederholtes Datagramm kostet
                // keine Entschlüsselung, und ein manipuliertes darf das Fenster
                // nicht bewegen.
                aktuelle.pruefe(zaehler)?;
                let klar = aktuelle
                    .chiffre
                    .decrypt(&nonce(zaehler), geheim)
                    .map_err(|_| Kryptofehler::Manipuliert)?;
                aktuelle.fortschreiben(zaehler);
                return Ok(klar);
            }
            if self.abgeloest.contains(&kennung) {
                return Err(Kryptofehler::Abgeloest);
            }
        }

        // Unbekannte Sitzung: Kandidatenschlüssel ableiten, entschlüsseln, und
        // nur bei Erfolg wechseln. Ein manipuliertes Datagramm mit erfundener
        // Sitzung lässt den Zustand damit unverändert.
        let mut kandidat = Sitzung::neu(&self.prk, self.info, kennung);
        let klar = kandidat
            .chiffre
            .decrypt(&nonce(zaehler), geheim)
            .map_err(|_| Kryptofehler::Manipuliert)?;
        kandidat.fortschreiben(zaehler);
        if let Some(alt) = self.aktuelle.replace(kandidat) {
            self.abgeloest.push(alt.kennung);
            if self.abgeloest.len() > ABGELOEST_MAX {
                self.abgeloest.remove(0);
            }
        }
        Ok(klar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_adapters::Schluessel;

    struct Seiten {
        gastgeber: Paar,
        teilnehmer: Paar,
        kennung_gastgeber: String,
        kennung_teilnehmer: String,
    }

    /// Zwei Seiten, die dasselbe ableiten — der Normalfall.
    fn paar(angebot: AngebotId) -> Seiten {
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        Seiten {
            gastgeber: Paar::neu(&a.austausch_geheimnis(), &b.kennung(), angebot)
                .expect("Ableitung Gastgeber"),
            teilnehmer: Paar::neu(&b.austausch_geheimnis(), &a.kennung(), angebot)
                .expect("Ableitung Teilnehmer"),
            kennung_gastgeber: a.kennung(),
            kennung_teilnehmer: b.kennung(),
        }
    }

    #[test]
    fn beide_seiten_leiten_dasselbe_ab() {
        let s = paar(1);
        assert_eq!(s.gastgeber.prk, s.teilnehmer.prk);
    }

    #[test]
    fn kennung_ohne_punkt_ergibt_none() {
        let a = Schluessel::neu();
        assert!(Paar::neu(&a.austausch_geheimnis(), "nicht hex", 1).is_none());
        assert!(Paar::neu(&a.austausch_geheimnis(), &"00".repeat(31), 1).is_none());
        assert!(Paar::neu(&a.austausch_geheimnis(), "", 1).is_none());
        // Ungerade Hexlänge.
        assert!(Paar::neu(&a.austausch_geheimnis(), &"0".repeat(63), 1).is_none());
    }

    #[test]
    fn strom_laeuft_in_beide_richtungen() {
        let s = paar(1);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel_a, mut oeffner_a) = s.gastgeber.strom(&salz_t, &salz_g, Rolle::Gastgeber);
        let (mut siegel_b, mut oeffner_b) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);

        // Teilnehmer → Gastgeber
        let geheim = siegel_b.versiegeln(b"hallo Gastgeber");
        assert_eq!(
            oeffner_a.oeffnen(&geheim).expect("öffnen"),
            b"hallo Gastgeber"
        );
        // Gastgeber → Teilnehmer
        let geheim = siegel_a.versiegeln(b"hallo Teilnehmer");
        assert_eq!(
            oeffner_b.oeffnen(&geheim).expect("öffnen"),
            b"hallo Teilnehmer"
        );
    }

    #[test]
    fn anderes_angebot_oeffnet_nicht() {
        let s = paar(1);
        let t = paar(2);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = t.gastgeber.strom(&salz_t, &salz_g, Rolle::Gastgeber);
        let geheim = siegel.versiegeln(b"geheim");
        assert_eq!(oeffner.oeffnen(&geheim), Err(Kryptofehler::Manipuliert));
    }

    #[test]
    fn anderes_salz_oeffnet_nicht() {
        let s = paar(1);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let salz_fremd = strom_salz();
        assert_ne!(salz_t, salz_g, "Zufall soll verschieden sein");
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.strom(&salz_t, &salz_fremd, Rolle::Gastgeber);
        let geheim = siegel.versiegeln(b"geheim");
        assert_eq!(oeffner.oeffnen(&geheim), Err(Kryptofehler::Manipuliert));
    }

    #[test]
    fn vertauschte_salze_scheitern() {
        // Wer Teilnehmer- und Gastgebersalz verwechselt, rechnet mit anderen
        // Schlüsseln — die Richtungen sind nicht symmetrisch.
        let s = paar(1);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.strom(&salz_g, &salz_t, Rolle::Gastgeber);
        let geheim = siegel.versiegeln(b"geheim");
        assert_eq!(oeffner.oeffnen(&geheim), Err(Kryptofehler::Manipuliert));
    }

    #[test]
    fn gleiches_teilnehmersalz_aber_neues_gastgebersalz_aendert_den_geheimtext() {
        // Der Kern des Befunds: spielt ein umgebauter Server dem Gastgeber ein
        // altes Teilnehmersalz ein, darf der Geheimtext des Gastgebers trotzdem
        // nicht derselbe sein — sonst wiederholte er Nonces ab Zähler 0.
        let s = paar(1);
        let salz_t = strom_salz();
        let salz_g1 = strom_salz();
        let salz_g2 = strom_salz();
        let (mut siegel_1, _) = s.gastgeber.strom(&salz_t, &salz_g1, Rolle::Gastgeber);
        let (mut siegel_2, _) = s.gastgeber.strom(&salz_t, &salz_g2, Rolle::Gastgeber);
        let geheim_1 = siegel_1.versiegeln(b"dieselbe Nachricht");
        let geheim_2 = siegel_2.versiegeln(b"dieselbe Nachricht");
        assert_ne!(geheim_1, geheim_2);

        // Und der Teilnehmer, der das neue Salz des Gastgebers gelesen hat,
        // öffnet das neue Siegel; mit dem alten Salz ginge es nicht.
        let (_, mut oeffner_neu) = s.teilnehmer.strom(&salz_t, &salz_g2, Rolle::Teilnehmer);
        let (_, mut oeffner_alt) = s.teilnehmer.strom(&salz_t, &salz_g1, Rolle::Teilnehmer);
        assert_eq!(
            oeffner_neu.oeffnen(&geheim_2).expect("öffnen"),
            b"dieselbe Nachricht"
        );
        assert_eq!(
            oeffner_alt.oeffnen(&geheim_2),
            Err(Kryptofehler::Manipuliert)
        );
    }

    #[test]
    fn manipuliertes_byte_im_strom_wird_erkannt() {
        let s = paar(3);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.strom(&salz_t, &salz_g, Rolle::Gastgeber);
        let mut geheim = siegel.versiegeln(b"zwanzig Byte Klartext");
        let letzte = geheim.len() - 1;
        geheim[letzte] ^= 0x01;
        assert_eq!(oeffner.oeffnen(&geheim), Err(Kryptofehler::Manipuliert));
    }

    #[test]
    fn zerstueckelter_strom_ergibt_denselben_klartext() {
        let s = paar(4);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.strom(&salz_t, &salz_g, Rolle::Gastgeber);
        // Drei Blöcke, also zwei volle und ein Rest.
        let klartext: Vec<u8> = (0..STROM_BLOCK * 2 + 500)
            .map(|i| (i % 251) as u8)
            .collect();
        let geheim = siegel.versiegeln(&klartext);

        let mut gesammelt = Vec::new();
        for byte in &geheim {
            gesammelt.extend_from_slice(&oeffner.oeffnen(&[*byte]).expect("öffnen"));
        }
        assert_eq!(gesammelt, klartext);
    }

    #[test]
    fn leerer_strom_ergibt_keine_bytes() {
        let s = paar(5);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, mut oeffner) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        assert!(siegel.versiegeln(&[]).is_empty());
        assert!(oeffner.oeffnen(&[]).expect("öffnen").is_empty());
    }

    #[test]
    fn grosser_klartext_ueber_mehrere_bloecke() {
        let s = paar(6);
        let salz_t = strom_salz();
        let salz_g = strom_salz();
        let (mut siegel, _) = s.teilnehmer.strom(&salz_t, &salz_g, Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.strom(&salz_t, &salz_g, Rolle::Gastgeber);
        let klartext: Vec<u8> = (0..100 * 1024).map(|i| (i % 253) as u8).collect();
        let geheim = siegel.versiegeln(&klartext);
        // 100 KiB in 16-KiB-Blöcken: sieben Blöcke, je 2 Byte Länge + 16 Byte
        // Aufschlag.
        assert_eq!(geheim.len(), klartext.len() + 7 * (2 + AUFSCHLAG));
        assert_eq!(oeffner.oeffnen(&geheim).expect("öffnen"), klartext);
    }

    #[test]
    fn datagramme_laufen_in_beide_richtungen() {
        let s = paar(7);
        let (mut siegel_a, mut oeffner_a) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let (mut siegel_b, mut oeffner_b) = s.teilnehmer.datagramme(Rolle::Teilnehmer);

        assert_eq!(
            oeffner_a
                .oeffnen(&siegel_b.versiegeln(b"hin"))
                .expect("öffnen"),
            b"hin"
        );
        assert_eq!(
            oeffner_b
                .oeffnen(&siegel_a.versiegeln(b"her"))
                .expect("öffnen"),
            b"her"
        );
    }

    #[test]
    fn doppeltes_datagramm_ist_wiederholt() {
        let s = paar(8);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let geheim = siegel.versiegeln(b"einmal");
        assert_eq!(oeffner.oeffnen(&geheim).expect("öffnen"), b"einmal");
        assert_eq!(oeffner.oeffnen(&geheim), Err(Kryptofehler::Wiederholt));
    }

    #[test]
    fn vertauschte_reihenfolge_im_fenster_wird_angenommen() {
        let s = paar(9);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let d: Vec<Vec<u8>> = (0..4).map(|i| siegel.versiegeln(&[i])).collect();
        // Der dritte zuerst, dann der erste — beides innerhalb von 64.
        assert_eq!(oeffner.oeffnen(&d[2]).expect("öffnen"), vec![2]);
        assert_eq!(oeffner.oeffnen(&d[0]).expect("öffnen"), vec![0]);
        assert_eq!(oeffner.oeffnen(&d[1]).expect("öffnen"), vec![1]);
        assert_eq!(oeffner.oeffnen(&d[2]), Err(Kryptofehler::Wiederholt));
    }

    #[test]
    fn zu_alte_datagramme_werden_verworfen() {
        let s = paar(10);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let d: Vec<Vec<u8>> = (0..=70).map(|i| siegel.versiegeln(&[i as u8])).collect();
        // 70 annehmen, dann den allerersten: 70 Schritte zurück ist außerhalb.
        assert!(oeffner.oeffnen(&d[70]).is_ok());
        assert_eq!(oeffner.oeffnen(&d[0]), Err(Kryptofehler::ZuAlt));
        // 63 Schritte zurück liegt noch im Fenster, 64 nicht mehr.
        assert!(oeffner.oeffnen(&d[7]).is_ok());
        assert_eq!(oeffner.oeffnen(&d[6]), Err(Kryptofehler::ZuAlt));
    }

    #[test]
    fn manipuliertes_datagramm_verschiebt_das_fenster_nicht() {
        let s = paar(11);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        assert!(oeffner.oeffnen(&siegel.versiegeln(b"erst")).is_ok());

        let echt = siegel.versiegeln(b"zweit");
        let mut falsch = echt.clone();
        let letzte = falsch.len() - 1;
        falsch[letzte] ^= 0xff;
        assert_eq!(oeffner.oeffnen(&falsch), Err(Kryptofehler::Manipuliert));
        // Das echte Datagramm mit demselben Zähler geht danach noch durch.
        assert_eq!(oeffner.oeffnen(&echt).expect("öffnen"), b"zweit");
    }

    #[test]
    fn zu_kurzes_datagramm_ist_zu_kurz() {
        let s = paar(12);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        assert_eq!(oeffner.oeffnen(&[]), Err(Kryptofehler::ZuKurz));
        const KOPF: usize = SITZUNG_LAENGE + ZAEHLER_LAENGE;
        assert_eq!(
            oeffner.oeffnen(&[0u8; KOPF + AUFSCHLAG - 1]),
            Err(Kryptofehler::ZuKurz)
        );
        // Genau der Kopf plus Prüfsumme reicht: leerer Klartext.
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let leer = siegel.versiegeln(&[]);
        assert_eq!(leer.len(), KOPF + AUFSCHLAG);
        assert!(oeffner.oeffnen(&leer).expect("öffnen").is_empty());
    }

    #[test]
    fn datagramm_beginnt_mit_der_sitzung_dann_dem_zaehler() {
        let s = paar(15);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let sitzung = siegel.sitzung();
        let erst = siegel.versiegeln(b"eins");
        assert_eq!(&erst[..SITZUNG_LAENGE], &sitzung[..]);
        assert_eq!(
            &erst[SITZUNG_LAENGE..SITZUNG_LAENGE + ZAEHLER_LAENGE],
            &[0u8; 8]
        );
        let zweit = siegel.versiegeln(b"zwei");
        assert_eq!(&zweit[..SITZUNG_LAENGE], &sitzung[..]);
        assert_eq!(
            &zweit[SITZUNG_LAENGE..SITZUNG_LAENGE + ZAEHLER_LAENGE],
            &1u64.to_be_bytes()
        );
    }

    #[test]
    fn zweites_beitreten_ergibt_neue_sitzung_und_andere_geheimtexte() {
        // Kern des Befunds: gleiches Paar, gleiche Freigabe, gleicher Klartext,
        // gleicher Zähler — trotzdem andere Geheimtexte, weil die Sitzung neu
        // ist. Sonst wäre der Nonce ab 0 derselbe.
        let s = paar(16);
        let (mut erstes, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (mut zweites, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        assert_ne!(erstes.sitzung(), zweites.sitzung());
        assert_ne!(
            erstes.versiegeln(b"derselbe Klartext"),
            zweites.versiegeln(b"derselbe Klartext")
        );
    }

    #[test]
    fn oeffner_wechselt_auf_die_neue_sitzung_und_lehnt_die_alte_ab() {
        let s = paar(17);
        let (mut alt, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);

        // Erstes Beitreten: das Datagramm bringt dem Öffner die Sitzung bei.
        let alt_0 = alt.versiegeln(b"alt-0");
        assert_eq!(oeffner.oeffnen(&alt_0).expect("öffnen"), b"alt-0");
        let alt_1 = alt.versiegeln(b"alt-1");
        assert_eq!(oeffner.oeffnen(&alt_1).expect("öffnen"), b"alt-1");

        // Zweites Beitreten: neues Siegel, neue Sitzung, Zähler wieder 0.
        let (mut neu, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let neu_0 = neu.versiegeln(b"neu-0");
        assert_eq!(oeffner.oeffnen(&neu_0).expect("öffnen"), b"neu-0");
        // Die neue Sitzung zählt für sich: derselbe Zähler noch einmal wäre
        // jetzt eine Wiederholung.
        assert_eq!(oeffner.oeffnen(&neu_0), Err(Kryptofehler::Wiederholt));

        // Die alte Sitzung ist abgelöst — auch ein frisches Datagramm von ihr.
        let alt_2 = alt.versiegeln(b"alt-2");
        assert_eq!(oeffner.oeffnen(&alt_2), Err(Kryptofehler::Abgeloest));
        assert_eq!(oeffner.oeffnen(&alt_0), Err(Kryptofehler::Abgeloest));

        // Die neue Sitzung läuft weiter.
        assert_eq!(
            oeffner.oeffnen(&neu.versiegeln(b"neu-1")).expect("öffnen"),
            b"neu-1"
        );
    }

    #[test]
    fn hoechstens_acht_sitzungen_bleiben_abgeloest() {
        let s = paar(18);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let mut datagramme = Vec::new();
        for _ in 0..(ABGELOEST_MAX + 2) {
            let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
            let d = siegel.versiegeln(b"da");
            assert_eq!(oeffner.oeffnen(&d).expect("öffnen"), b"da");
            datagramme.push(d);
        }
        assert_eq!(oeffner.abgeloest.len(), ABGELOEST_MAX);

        // Die zuletzt abgelöste Sitzung ist noch bekannt.
        assert_eq!(
            oeffner.oeffnen(&datagramme[datagramme.len() - 2]),
            Err(Kryptofehler::Abgeloest)
        );
        // Die älteste ist aus der Liste gefallen und damit wieder „unbekannt“;
        // ihr Datagramm wird wie eine ganz alte Aufzeichnung als neue Sitzung
        // angenommen. Das ist die Grenze aus ADR 0016 §5 (Restrisiko: eine
        // ganze alte Sitzung lässt sich erneut einspielen) — lesen oder fälschen
        // kann der Server damit nicht.
        assert!(oeffner.oeffnen(&datagramme[0]).is_ok());
    }

    #[test]
    fn manipuliertes_datagramm_mit_unbekannter_sitzung_aendert_nichts() {
        let s = paar(19);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = s.gastgeber.datagramme(Rolle::Gastgeber);
        let echt = siegel.versiegeln(b"echt");
        assert_eq!(oeffner.oeffnen(&echt).expect("öffnen"), b"echt");

        // Erfundene Sitzung, erfundener Inhalt: kein Wechsel, kein Zustandsrest.
        let mut falsch = echt.clone();
        falsch[..SITZUNG_LAENGE].copy_from_slice(&[0xAA; SITZUNG_LAENGE]);
        let letzte = falsch.len() - 1;
        falsch[letzte] ^= 0xff;
        assert_eq!(oeffner.oeffnen(&falsch), Err(Kryptofehler::Manipuliert));

        // Das echte Siegel geht unverändert weiter (Zähler 1).
        assert_eq!(
            oeffner
                .oeffnen(&siegel.versiegeln(b"weiter"))
                .expect("öffnen"),
            b"weiter"
        );
        // Und eine echte Sitzung wird nicht schon durch den Fehlversuch
        // verbraucht: derselbe Zähler 0 gilt noch als offen.
        let (mut zweit, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        assert_eq!(
            oeffner
                .oeffnen(&zweit.versiegeln(b"zweit"))
                .expect("öffnen"),
            b"zweit"
        );
    }

    #[test]
    fn fremder_schluessel_oeffnet_nicht() {
        let s = paar(13);
        let f = paar(13);
        let (mut siegel, _) = s.teilnehmer.datagramme(Rolle::Teilnehmer);
        let (_, mut oeffner) = f.gastgeber.datagramme(Rolle::Gastgeber);
        assert_eq!(
            oeffner.oeffnen(&siegel.versiegeln(b"geheim")),
            Err(Kryptofehler::Manipuliert)
        );
    }

    #[test]
    fn salz_ist_zufaellig_und_lang_genug() {
        let a = strom_salz();
        let b = strom_salz();
        assert_eq!(a.len(), STROM_SALZ_LAENGE);
        assert_ne!(a, b);
    }

    #[test]
    fn kennungen_sind_die_der_schluessel() {
        // Der Test oben baut die Seiten aus echten Kennungen; hier nur die
        // Gegenprobe, dass beide verschieden sind (zwei Schlüssel, zwei Kennungen).
        let s = paar(14);
        assert_ne!(s.kennung_gastgeber, s.kennung_teilnehmer);
    }
}

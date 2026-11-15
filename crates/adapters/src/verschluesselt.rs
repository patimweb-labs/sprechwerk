//! Ende-zu-Ende-Verschlüsselung für Sprache — je Raum einschaltbar.
//!
//! # Was hier passiert
//!
//! ```text
//! A                                   B
//! │  Raumschlüssel K (32 Byte)        │
//! │  ── Umschlag: X25519(A,B) ──►     │  B öffnet, hat K
//! │  ── Sprache: AEAD(K, folge) ──►   │  B entschlüsselt
//! ```
//!
//! Der Server sieht in beiden Fällen nur Geheimtext. Er kann **weiterleiten und
//! Rechte prüfen**, weil der Sprachkopf (Version, Codec, Folge, Sprecher)
//! lesbar bleibt — nur die Nutzlast ist verschlüsselt.
//!
//! # Warum keine eigene Krypto
//!
//! X25519 (Schlüsselaustausch), HKDF-SHA256 (Ableitung) und
//! ChaCha20-Poly1305 (Verschlüsseln mit Echtheitsprüfung) sind erprobte
//! Verfahren aus `dalek` und `RustCrypto` — dieselben Bausteine, die TLS 1.3
//! verwendet. Was hier selbst geschrieben ist, ist nur die **Verpackung**
//! (welche Schlüssel, welche Zusatzdaten, welche Folgenummer im Nonce).
//!
//! # Der Umschlag ist signiert
//!
//! Ohne Unterschrift könnte der Server einen eigenen Schlüssel unterschieden
//! und mithören. Deshalb signiert der Absender `(Epoche, Raum, Umschlag)` mit
//! seinem Ed25519-Schlüssel; der Empfänger prüft die Unterschrift gegen die
//! Kennung des Absenders, die er vom Server kennt (sie ist öffentlich).

use anyhow::{anyhow, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// Größe eines Raumschlüssels.
pub const SCHLUESSEL_LEN: usize = 32;
/// Nonce-Größe von ChaCha20-Poly1305.
pub const NONCE_LEN: usize = 12;
/// Aufschlag durch die Verschlüsselung (Echtheitsprüfung).
pub const AUFSCHLAG: usize = 16;

/// Ein X25519-Geheimnis, abgeleitet aus dem Anmeldeschlüssel.
///
/// **Warum abgeleitet und nicht ein zweiter Schlüssel in der Datei?** Die
/// Schlüsseldatei bleibt damit unverändert (32 Byte hexadezimal), und es gibt
/// keine zweite Wahrheit, die man beim Umzug vergessen kann. Das Verfahren ist
/// dasselbe, das libsodium für `crypto_sign_ed25519_sk_to_curve25519` benutzt:
/// SHA-512 über den geheimen Schlüssel, dann klemmen.
pub fn geheimnis_aus_anmeldeschluessel(geheim: &[u8; 32]) -> StaticSecret {
    use sha2::Digest;
    // **SHA-512, nicht SHA-256**: Ed25519 bildet seinen Scala­r als
    // `clamp(SHA-512(seed)[0..32])`. Genau dieser Wert ist auch der
    // X25519-Geheimschlüssel — so machen es libsodium und alle anderen.
    let hash = sha2::Sha512::digest(geheim);
    let mut klemme = [0u8; 32];
    klemme.copy_from_slice(&hash[..32]);
    klemme[0] &= 248;
    klemme[31] &= 127;
    klemme[31] |= 64;
    StaticSecret::from(klemme)
}

/// Rechnet einen Ed25519-**öffentlichen** Schlüssel (Hex) in den passenden
/// X25519-Schlüssel um.
///
/// Das ist die Umrechnung zwischen den beiden Kurvenformen desselben Punktes
/// (Edwards → Montgomery). Sie ist verlustfrei und ohne Geheimnis möglich —
/// deshalb braucht der Schlüsselaustausch **keine** zweite Schlüsseldatei und
/// keine zusätzliche Angabe im Protokoll: jeder kann den Austauschschlüssel
/// des anderen aus dessen Kennung ausrechnen.
pub fn oeffentlich_aus_kennung(kennung_hex: &str) -> Option<PublicKey> {
    let roh = hex::decode(kennung_hex).ok()?;
    let bytes: [u8; 32] = roh.try_into().ok()?;
    let punkt = curve25519_dalek::edwards::CompressedEdwardsY(bytes).decompress()?;
    Some(PublicKey::from(punkt.to_montgomery().to_bytes()))
}

/// Ein fertiger Raumschlüssel mit seiner Epoche.
#[derive(Clone)]
pub struct Raumschluessel {
    /// Zählt hoch, sobald jemand den Raum verlässt: alter Geheimtext wird
    /// damit wertlos.
    pub epoche: u32,
    schluessel: [u8; SCHLUESSEL_LEN],
}

impl std::fmt::Debug for Raumschluessel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Schlüssel niemals in Protokolle schreiben.
        f.debug_struct("Raumschluessel")
            .field("epoche", &self.epoche)
            .finish_non_exhaustive()
    }
}

impl Raumschluessel {
    /// Erzeugt einen neuen Schlüssel aus dem Zufall des Betriebssystems.
    pub fn neu(epoche: u32) -> Self {
        let mut schluessel = [0u8; SCHLUESSEL_LEN];
        getrandom::getrandom(&mut schluessel).expect("Zufall");
        Self { epoche, schluessel }
    }

    /// Aus rohen Bytes (nach dem Öffnen eines Umschlags).
    pub fn aus_bytes(epoche: u32, bytes: [u8; SCHLUESSEL_LEN]) -> Self {
        Self {
            epoche,
            schluessel: bytes,
        }
    }

    pub fn bytes(&self) -> [u8; SCHLUESSEL_LEN] {
        self.schluessel
    }

    /// Kennung dieses Schlüssels für den Sprachkopf (1 Byte).
    pub fn kennung(&self) -> u8 {
        self.schluessel[0]
    }

    /// Verschlüsselt einen Opus-Rahmen. Der Nonce enthält die Folgenummer —
    /// damit ist jeder Rahmen eindeutig und Umsortieren fällt auf.
    pub fn verschluesseln(&self, folge: u16, raum: &str, daten: &[u8]) -> Result<Vec<u8>> {
        let nonce = self.nonce(folge);
        let aad = zusatzdaten(raum, self.epoche);
        let chiffre = ChaCha20Poly1305::new(Key::from_slice(&self.schluessel))
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: daten,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Verschlüsseln fehlgeschlagen"))?;
        Ok(chiffre)
    }

    /// Entschlüsselt einen Rahmen. Schlägt fehl, wenn er nicht zu diesem
    /// Schlüssel gehört oder verändert wurde.
    pub fn entschluesseln(&self, folge: u16, raum: &str, daten: &[u8]) -> Result<Vec<u8>> {
        let nonce = self.nonce(folge);
        let aad = zusatzdaten(raum, self.epoche);
        ChaCha20Poly1305::new(Key::from_slice(&self.schluessel))
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: daten,
                    aad: &aad,
                },
            )
            .map_err(|_| {
                anyhow!("Entschlüsseln fehlgeschlagen (falscher Schlüssel oder verändert)")
            })
    }

    /// Nonce: Folgenummer (2) + Zähler aus dem Schlüssel (10), damit dieselbe
    /// Folgenummer in einer neuen Epoche nicht denselben Nonce ergibt.
    fn nonce(&self, folge: u16) -> [u8; NONCE_LEN] {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[..2].copy_from_slice(&folge.to_be_bytes());
        nonce[2..].copy_from_slice(&self.schluessel[1..11]);
        nonce
    }
}

/// Zusatzdaten, die mitgeprüft werden (Raum und Epoche).
///
/// Damit ist ein Rahmen aus Raum A in Raum B wertlos, und ein Rahmen aus einer
/// alten Epoche wird abgelehnt.
fn zusatzdaten(raum: &str, epoche: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(raum.len() + 4);
    aad.extend_from_slice(raum.as_bytes());
    aad.extend_from_slice(&epoche.to_be_bytes());
    aad
}

/// Ein Umschlag: der Raumschlüssel, für **einen** Empfänger verschlüsselt.
///
/// ```text
/// [0..32)   ephemerer öffentlicher X25519-Schlüssel
/// [32..44)  Zufall für den Nonce  → kommt in den Nonce
/// [44..)    ChaCha20-Poly1305( K_raum )
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Umschlag {
    pub bytes: Vec<u8>,
}

impl Umschlag {
    /// Verpackt `schluessel` für den Empfänger mit öffentlichem X25519-Schlüssel.
    pub fn verpacken(
        absender: &StaticSecret,
        empfaenger: &PublicKey,
        raum: &str,
        schluessel: &Raumschluessel,
    ) -> Result<Self> {
        // Ephemeres Schlüsselpaar: der Absender-Langzeitschlüssel bleibt aus
        // der Ableitung heraus (Forward Secrecy je Umschlag).
        let mut eph_geheim = [0u8; 32];
        getrandom::getrandom(&mut eph_geheim).map_err(|e| anyhow!("Zufall: {e}"))?;
        let eph = StaticSecret::from(eph_geheim);
        let gemeinsam = eph.diffie_hellman(empfaenger);
        // Zweiter Anteil: der **statische** Austausch zwischen Absender und
        // Empfänger. Damit ist der Umschlag an genau diese beiden gebunden —
        // ein Fremder kann ihn auch mit dem ephemeren Teil nicht öffnen.
        let statisch = absender.diffie_hellman(empfaenger);
        let kex = gemeinsame_ableitung(
            gemeinsam.as_bytes(),
            statisch.as_bytes(),
            raum,
            schluessel.epoche,
        )?;

        let mut zufall = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut zufall).map_err(|e| anyhow!("Zufall: {e}"))?;
        let nonce = Nonce::from_slice(&zufall);
        let aad = zusatzdaten(raum, schluessel.epoche);
        let chiffre = ChaCha20Poly1305::new(Key::from_slice(&kex))
            .encrypt(
                nonce,
                Payload {
                    msg: &schluessel.schluessel,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Umschlag verschlüsseln fehlgeschlagen"))?;

        let mut bytes = Vec::with_capacity(32 + NONCE_LEN + chiffre.len());
        bytes.extend_from_slice(PublicKey::from(&eph).as_bytes());
        bytes.extend_from_slice(&zufall);
        bytes.extend_from_slice(&chiffre);
        Ok(Self { bytes })
    }

    /// Öffnet den Umschlag mit dem eigenen Geheimnis.
    pub fn oeffnen(
        &self,
        empfaenger: &StaticSecret,
        absender: &PublicKey,
        raum: &str,
        epoche: u32,
    ) -> Result<Raumschluessel> {
        if self.bytes.len() < 32 + NONCE_LEN + SCHLUESSEL_LEN + AUFSCHLAG {
            return Err(anyhow!("Umschlag zu kurz"));
        }
        let (eph_bytes, rest) = self.bytes.split_at(32);
        let (zufall, chiffre) = rest.split_at(NONCE_LEN);
        let eph = PublicKey::from(<[u8; 32]>::try_from(eph_bytes)?);
        let gemeinsam = empfaenger.diffie_hellman(&eph);
        let statisch = empfaenger.diffie_hellman(absender);
        let kex = gemeinsame_ableitung(gemeinsam.as_bytes(), statisch.as_bytes(), raum, epoche)?;

        let aad = zusatzdaten(raum, epoche);
        let klar = ChaCha20Poly1305::new(Key::from_slice(&kex))
            .decrypt(
                Nonce::from_slice(zufall),
                Payload {
                    msg: chiffre,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Umschlag öffnet nicht (falscher Empfänger oder verändert)"))?;
        let bytes: [u8; SCHLUESSEL_LEN] = klar
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Umschlag enthält keinen 32-Byte-Schlüssel"))?;
        Ok(Raumschluessel::aus_bytes(epoche, bytes))
    }
}

/// HKDF-Ableitung für den Umschlag.
///
/// Es gehen **beide** Diffie-Hellman-Ergebnisse ein (ephemer gegen Empfänger
/// und ephemer gegen Absender). Damit kann nur jemand öffnen, der den
/// ephemeren Schlüssel kennt *und* den Umschlag vom genannten Absender hat.
fn gemeinsame_ableitung(
    ephemer: &[u8; 32],
    statisch: &[u8; 32],
    raum: &str,
    epoche: u32,
) -> Result<[u8; 32]> {
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(ephemer);
    ikm.extend_from_slice(statisch);
    let mut salt = Vec::with_capacity(raum.len() + 4);
    salt.extend_from_slice(raum.as_bytes());
    salt.extend_from_slice(&epoche.to_be_bytes());
    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut aus = [0u8; 32];
    hk.expand(b"sprechwerk raumschluessel", &mut aus)
        .map_err(|_| anyhow!("Schlüsselableitung fehlgeschlagen"))?;
    Ok(aus)
}

// ──────────────── Austausch je Raumsitzung: hybrid und mit Forward Secrecy ──
//
// **ADR 0010, Schritt 2.** Der Umschlag oben bindet den Empfänger an seinen
// *langlebigen* Austauschschlüssel: wer die Schlüsseldatei später erbeutet, kann
// jede aufgezeichnete Umlage öffnen. Und er ist klassisch (X25519) — ein
// Quantenrechner mit Mitschnitt von heute bräche ihn.
//
// Beides behebt derselbe Umbau: jeder Client erzeugt beim Betreten eines Raums
// **ephemere** Austauschschlüssel (X25519 **und** ML-KEM-768), veröffentlicht
// die öffentlichen Hälften (signiert mit dem Anmeldeschlüssel) und wirft die
// geheimen beim Verlassen weg. Ein Umschlag geht dann gegen diese Sitzung —
// alte Aufzeichnungen sind nach dem Verlassen wertlos, und ein Quantenrechner
// müsste **beide** Verfahren brechen.

use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};

/// Länge eines X25519-Schlüssels.
pub const X25519_LEN: usize = 32;
/// Länge des öffentlichen ML-KEM-768-Schlüssels (FIPS-203).
pub const KEM_OEFFENTLICH_LEN: usize = 1184;
/// Länge eines ML-KEM-768-Umschlags.
pub const KEM_UMSCHLAG_LEN: usize = 1088;
/// Was ein Client veröffentlicht: X25519-Schlüssel und KEM-Schlüssel.
pub const AUSTAUSCH_LEN: usize = X25519_LEN + KEM_OEFFENTLICH_LEN;
/// Größe eines hybriden Umschlags (ohne Hex).
pub const HYBRIDER_UMSCHLAG_LEN: usize =
    X25519_LEN + KEM_UMSCHLAG_LEN + NONCE_LEN + SCHLUESSEL_LEN + AUFSCHLAG;

// Über `KemCore` statt über den Parametersatz: der heißt in der Kiste intern
// `MlKem768Params` und ist nicht Teil der öffentlichen Schnittstelle.
type Kapselung = <MlKem768 as KemCore>::EncapsulationKey;
type Dekapselung = <MlKem768 as KemCore>::DecapsulationKey;

/// Ein **ephemerer** Austauschschlüssel für eine Raumsitzung.
///
/// Er lebt genau so lange wie der Raumaufenthalt: beim Betreten erzeugt, beim
/// Verlassen verworfen. Genau das macht Forward Secrecy aus — nicht die
/// Verschlüsselung selbst, sondern das Wegwerfen der Geheimnisse.
pub struct Austausch {
    x25519: StaticSecret,
    kem: Dekapselung,
    /// Öffentliche Hälften (X25519 ‖ ML-KEM), wie sie verschickt werden.
    oeffentlich: Vec<u8>,
}

impl std::fmt::Debug for Austausch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Geheimnisse gehören nicht in Protokolle.
        f.debug_struct("Austausch")
            .field("oeffentlich", &hex::encode(&self.oeffentlich[..8]))
            .finish_non_exhaustive()
    }
}

impl Austausch {
    /// Erzeugt ein Paar aus dem Zufall des Betriebssystems.
    pub fn neu() -> Result<Self> {
        let mut geheim = [0u8; 32];
        getrandom::getrandom(&mut geheim).map_err(|e| anyhow!("Zufall: {e}"))?;
        let x25519 = StaticSecret::from(geheim);
        let (kem, kapselung) = MlKem768::generate(&mut zufall_quelle()?);
        let mut oeffentlich = Vec::with_capacity(AUSTAUSCH_LEN);
        oeffentlich.extend_from_slice(PublicKey::from(&x25519).as_bytes());
        oeffentlich.extend_from_slice(kapselung.as_bytes().as_slice());
        Ok(Self {
            x25519,
            kem,
            oeffentlich,
        })
    }

    /// Die öffentliche Hälfte als Hex — das ist, was in den Raum geht.
    pub fn oeffentlich(&self) -> String {
        hex::encode(&self.oeffentlich)
    }

    /// Öffnet einen hybriden Umschlag, der an **diese** Sitzung ging.
    pub fn oeffnen(&self, umschlag: &Umschlag, raum: &str, epoche: u32) -> Result<Raumschluessel> {
        let bytes = &umschlag.bytes;
        if bytes.len() != HYBRIDER_UMSCHLAG_LEN {
            return Err(anyhow!(
                "Umschlag hat {} Byte, erwartet {}",
                bytes.len(),
                HYBRIDER_UMSCHLAG_LEN
            ));
        }
        let (absender_x, rest) = bytes.split_at(X25519_LEN);
        let (kem_umschlag, rest) = rest.split_at(KEM_UMSCHLAG_LEN);
        let (zufall, chiffre) = rest.split_at(NONCE_LEN);

        let absender = PublicKey::from(<[u8; 32]>::try_from(absender_x)?);
        let ss_x = self.x25519.diffie_hellman(&absender);
        // Ein Punkt kleiner Ordnung ergibt ein Null-Geheimnis: „hybrid" hieße
        // dann nur noch ML-KEM. Ablehnen statt still schwächer werden.
        if !ss_x.was_contributory() {
            return Err(anyhow!("Umschlag trägt einen ungültigen X25519-Wert"));
        }
        let kem_teiler: ml_kem::Ciphertext<MlKem768> = kem_umschlag
            .try_into()
            .map_err(|_| anyhow!("KEM-Anteil hat die falsche Länge"))?;
        let ss_kem = self
            .kem
            .decapsulate(&kem_teiler)
            .map_err(|_| anyhow!("KEM-Anteil öffnet nicht"))?;

        let eigen = PublicKey::from(&self.x25519);
        let kex = ableitung_hybrid(
            ss_x.as_bytes(),
            ss_kem.as_slice(),
            absender.as_bytes(),
            eigen.as_bytes(),
            raum,
            epoche,
        )?;
        let aad = zusatzdaten(raum, epoche);
        let klar = ChaCha20Poly1305::new(Key::from_slice(&kex))
            .decrypt(
                Nonce::from_slice(zufall),
                Payload {
                    msg: chiffre,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Umschlag öffnet nicht (falsche Sitzung oder verändert)"))?;
        let bytes: [u8; SCHLUESSEL_LEN] = klar
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Umschlag enthält keinen 32-Byte-Schlüssel"))?;
        Ok(Raumschluessel::aus_bytes(epoche, bytes))
    }
}

/// Die veröffentlichte Hälfte eines anderen Teilnehmers.
pub struct FremderAustausch {
    x25519: PublicKey,
    kem: Kapselung,
}

impl FremderAustausch {
    /// Liest, was ein Mitglied in den Raum gestellt hat.
    ///
    /// Die Zuordnung „wer" kommt **nicht** von hier, sondern aus der
    /// Ed25519-Unterschrift am Rahmen: dieser Wert ist nur ein Schlüssel, die
    /// Identität steht daneben.
    pub fn aus_hex(text: &str) -> Result<Self> {
        let roh = hex::decode(text.trim()).map_err(|_| anyhow!("Austauschschlüssel kein Hex"))?;
        if roh.len() != AUSTAUSCH_LEN {
            return Err(anyhow!(
                "Austauschschlüssel hat {} Byte, erwartet {}",
                roh.len(),
                AUSTAUSCH_LEN
            ));
        }
        let (x, kem) = roh.split_at(X25519_LEN);
        let x25519 = PublicKey::from(<[u8; 32]>::try_from(x)?);
        let kem_bytes: ml_kem::Encoded<Kapselung> = kem
            .try_into()
            .map_err(|_| anyhow!("KEM-Anteil hat die falsche Länge"))?;
        Ok(Self {
            x25519,
            kem: Kapselung::from_bytes(&kem_bytes),
        })
    }
}

/// Verpackt einen Raumschlüssel **hybrid** für eine Sitzung.
///
/// Der Absender braucht dafür nur seine eigene Sitzung (für den ephemeren
/// X25519-Teil) und die veröffentlichte Hälfte des Empfängers.
pub fn verpacken_hybrid(
    empfaenger: &FremderAustausch,
    raum: &str,
    schluessel: &Raumschluessel,
) -> Result<Umschlag> {
    let mut geheim = [0u8; 32];
    getrandom::getrandom(&mut geheim).map_err(|e| anyhow!("Zufall: {e}"))?;
    let eph = StaticSecret::from(geheim);
    let eph_oeffentlich = PublicKey::from(&eph);
    let ss_x = eph.diffie_hellman(&empfaenger.x25519);
    if !ss_x.was_contributory() {
        return Err(anyhow!("Austauschschlüssel des Empfängers ist ungültig"));
    }
    let (kem_umschlag, ss_kem) = empfaenger
        .kem
        .encapsulate(&mut zufall_quelle()?)
        .map_err(|_| anyhow!("KEM-Verpacken fehlgeschlagen"))?;

    let kex = ableitung_hybrid(
        ss_x.as_bytes(),
        ss_kem.as_slice(),
        eph_oeffentlich.as_bytes(),
        empfaenger.x25519.as_bytes(),
        raum,
        schluessel.epoche,
    )?;
    let mut zufall = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut zufall).map_err(|e| anyhow!("Zufall: {e}"))?;
    let aad = zusatzdaten(raum, schluessel.epoche);
    let chiffre = ChaCha20Poly1305::new(Key::from_slice(&kex))
        .encrypt(
            Nonce::from_slice(&zufall),
            Payload {
                msg: &schluessel.schluessel,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow!("Umschlag verschlüsseln fehlgeschlagen"))?;

    let mut bytes = Vec::with_capacity(HYBRIDER_UMSCHLAG_LEN);
    bytes.extend_from_slice(eph_oeffentlich.as_bytes());
    bytes.extend_from_slice(kem_umschlag.as_slice());
    bytes.extend_from_slice(&zufall);
    bytes.extend_from_slice(&chiffre);
    Ok(Umschlag { bytes })
}

/// HKDF über **beide** Geheimnisse und die X25519-Werte des Austauschs.
///
/// Aufbau wie beim Kombinierer von X-Wing (draft-connolly-cfrg-xwing):
/// `ss_kem ‖ ss_x ‖ X25519(eph) ‖ X25519(empfänger)`. Die öffentlichen
/// X25519-Werte gehören hinein, weil X25519 — anders als ML-KEM — nicht
/// IND-CCA ist: erst mit ihnen hängt der Schlüssel an **diesem** Austausch und
/// nicht nur am geteilten Punkt. Den KEM-Umschlag selbst braucht es nicht
/// (ML-KEM bindet ihn schon).
///
/// Die Reihenfolge und die Beschriftung sind Teil des Vertrags: ändert sich
/// hier etwas, öffnet keine Gegenstelle mehr — und zwar auf beiden Seiten
/// gleichzeitig, nicht still.
fn ableitung_hybrid(
    ss_x: &[u8; 32],
    ss_kem: &[u8],
    x_absender: &[u8; 32],
    x_empfaenger: &[u8; 32],
    raum: &str,
    epoche: u32,
) -> Result<[u8; 32]> {
    let mut ikm = Vec::with_capacity(ss_kem.len() + 3 * X25519_LEN);
    ikm.extend_from_slice(ss_kem);
    ikm.extend_from_slice(ss_x);
    ikm.extend_from_slice(x_absender);
    ikm.extend_from_slice(x_empfaenger);
    let mut salt = Vec::with_capacity(raum.len() + 4);
    salt.extend_from_slice(raum.as_bytes());
    salt.extend_from_slice(&epoche.to_be_bytes());
    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut aus = [0u8; 32];
    hk.expand(b"sprechwerk raumschluessel hybrid", &mut aus)
        .map_err(|_| anyhow!("Schlüsselableitung fehlgeschlagen"))?;
    Ok(aus)
}

/// Ein Zufallsgenerator für ML-KEM (dessen Schnittstelle verlangt `CryptoRngCore`).
fn zufall_quelle() -> Result<impl rand_core::CryptoRngCore> {
    Ok(rand_core::OsRng)
}

/// Was in einem Schlüsselpaket steht, bevor es signiert wird.
///
/// Die Unterschrift deckt Raum, Epoche und den Umschlag ab — so kann der Server
/// kein Paket in einen anderen Raum oder eine andere Epoche umhängen, und er
/// kann keinen eigenen Schlüssel unterschieden (er kann die Unterschrift des
/// Absenders nicht fälschen).
pub fn zu_signieren(raum: &str, epoche: u32, umschlag: &[u8]) -> String {
    let mut text = format!("{raum}|{epoche}|");
    text.push_str(&hex::encode(umschlag));
    text
}

/// Verschlüsselt die Nutzlast eines fertigen Sprach-Datagramms.
///
/// Der Kopf (Version, Folge, Sprecher) bleibt stehen und bekommt nur die
/// Kennung „verschlüsselt". Der Server kann damit weiterleiten und prüfen; er
/// sieht vom Inhalt nichts.
pub fn rahmen_verschluesseln(
    datagramm: &[u8],
    schluessel: &Raumschluessel,
    raum: &str,
) -> Result<Vec<u8>> {
    use sprechwerk_protocol::{Sprachkopf, CODEC_VERSCHLUESSELT};
    let (kopf, nutzlast) =
        Sprachkopf::lesen(datagramm).ok_or_else(|| anyhow!("kein gültiger Sprachrahmen"))?;
    let geheim = schluessel.verschluesseln(kopf.folge, raum, nutzlast)?;
    let mut neuer_kopf = kopf;
    neuer_kopf.codec = CODEC_VERSCHLUESSELT;
    Ok(neuer_kopf.schreiben(&geheim))
}

/// Entschlüsselt die Nutzlast eines verschlüsselten Sprach-Datagramms.
///
/// Liefert den Rahmen so zurück, wie ihn der Empfänger erwartet (Codec Opus).
pub fn rahmen_entschluesseln(
    datagramm: &[u8],
    schluessel: &Raumschluessel,
    raum: &str,
) -> Result<Vec<u8>> {
    use sprechwerk_protocol::{Sprachkopf, CODEC_OPUS, CODEC_VERSCHLUESSELT};
    let (kopf, nutzlast) =
        Sprachkopf::lesen(datagramm).ok_or_else(|| anyhow!("kein gültiger Sprachrahmen"))?;
    if kopf.codec != CODEC_VERSCHLUESSELT {
        return Err(anyhow!("Rahmen ist nicht verschlüsselt"));
    }
    let klar = schluessel.entschluesseln(kopf.folge, raum, nutzlast)?;
    let mut neuer_kopf = kopf;
    neuer_kopf.codec = CODEC_OPUS;
    Ok(neuer_kopf.schreiben(&klar))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paar() -> (StaticSecret, PublicKey, StaticSecret, PublicKey) {
        let a = StaticSecret::from([7u8; 32]);
        let b = StaticSecret::from([9u8; 32]);
        (
            a.clone(),
            PublicKey::from(&a),
            b.clone(),
            PublicKey::from(&b),
        )
    }

    #[test]
    fn austauschschluessel_passt_zur_kennung() {
        // Der aus dem Geheimnis abgeleitete X25519-Schlüssel muss genau der
        // umgerechnete Ed25519-Schlüssel sein — sonst könnten sich zwei
        // Clients nicht finden.
        let geheim = [3u8; 32];
        let x = geheimnis_aus_anmeldeschluessel(&geheim);
        let ed = ed25519_dalek::SigningKey::from_bytes(&geheim);
        let kennung = hex::encode(ed.verifying_key().to_bytes());
        assert_eq!(
            PublicKey::from(&x),
            oeffentlich_aus_kennung(&kennung).expect("Umrechnung"),
            "abgeleiteter Austauschschlüssel muss zur Kennung passen"
        );
    }

    #[test]
    fn ableitung_ist_stabil_und_verschieden() {
        let a = geheimnis_aus_anmeldeschluessel(&[1u8; 32]);
        let b = geheimnis_aus_anmeldeschluessel(&[1u8; 32]);
        assert_eq!(
            PublicKey::from(&a),
            PublicKey::from(&b),
            "gleicher Same → gleicher Schlüssel"
        );
        let c = geheimnis_aus_anmeldeschluessel(&[2u8; 32]);
        assert_ne!(PublicKey::from(&a), PublicKey::from(&c));
    }

    #[test]
    fn umschlag_kommt_beim_empfaenger_an() {
        let (a, a_pub, b, b_pub) = paar();
        let raum = "raum_1";
        let schluessel = Raumschluessel::neu(1);
        let umschlag = Umschlag::verpacken(&a, &b_pub, raum, &schluessel).unwrap();
        let geoeffnet = umschlag.oeffnen(&b, &a_pub, raum, 1).unwrap();
        assert_eq!(geoeffnet.epoche, 1);
        assert_eq!(geoeffnet.bytes(), schluessel.bytes());
    }

    #[test]
    fn fremder_kann_den_umschlag_nicht_oeffnen() {
        let (a, a_pub, _b, b_pub) = paar();
        let fremd = StaticSecret::from([42u8; 32]);
        let umschlag = Umschlag::verpacken(&a, &b_pub, "raum_1", &Raumschluessel::neu(1)).unwrap();
        assert!(umschlag.oeffnen(&fremd, &a_pub, "raum_1", 1).is_err());
    }

    #[test]
    fn anderer_raum_oder_andere_epoche_scheitert() {
        let (a, a_pub, b, b_pub) = paar();
        let umschlag = Umschlag::verpacken(&a, &b_pub, "raum_1", &Raumschluessel::neu(4)).unwrap();
        assert!(
            umschlag.oeffnen(&b, &a_pub, "raum_2", 4).is_err(),
            "Raum gehört zu den Zusatzdaten"
        );
        assert!(
            umschlag.oeffnen(&b, &a_pub, "raum_1", 5).is_err(),
            "Epoche gehört zu den Zusatzdaten"
        );
    }

    #[test]
    fn sprache_ist_verschluesselt_und_wieder_lesbar() {
        let schluessel = Raumschluessel::neu(2);
        let rohdaten = b"Opus-Rahmen mit Sprache";
        let geheim = schluessel.verschluesseln(17, "raum_9", rohdaten).unwrap();
        assert_ne!(
            &geheim[..],
            &rohdaten[..],
            "Klartext darf nicht durchscheinen"
        );
        assert!(
            !geheim.windows(5).any(|w| w == b"Sprac"),
            "kein Klartext im Geheimtext"
        );
        assert_eq!(geheim.len(), rohdaten.len() + AUFSCHLAG);
        let zurueck = schluessel.entschluesseln(17, "raum_9", &geheim).unwrap();
        assert_eq!(zurueck, rohdaten);
    }

    #[test]
    fn verkehrte_folgenummer_und_falscher_raum_scheitern() {
        let schluessel = Raumschluessel::neu(1);
        let geheim = schluessel.verschluesseln(3, "raum_1", b"abc").unwrap();
        assert!(schluessel.entschluesseln(4, "raum_1", &geheim).is_err());
        assert!(schluessel.entschluesseln(3, "raum_2", &geheim).is_err());
    }

    #[test]
    fn fremder_schluessel_scheitert() {
        let a = Raumschluessel::neu(1);
        let b = Raumschluessel::neu(1);
        let geheim = a.verschluesseln(0, "raum_1", b"abc").unwrap();
        assert!(b.entschluesseln(0, "raum_1", &geheim).is_err());
    }

    #[test]
    fn verdrehtes_bit_faellt_auf() {
        let schluessel = Raumschluessel::neu(1);
        let mut geheim = schluessel.verschluesseln(0, "raum_1", b"abcdef").unwrap();
        geheim[2] ^= 0x01;
        assert!(schluessel.entschluesseln(0, "raum_1", &geheim).is_err());
    }

    #[test]
    fn sprachrahmen_wird_verschluesselt_und_wieder_klar() {
        use sprechwerk_protocol::{Sprachkopf, CODEC_OPUS, CODEC_VERSCHLUESSELT};
        let schluessel = Raumschluessel::neu(1);
        let klar = Sprachkopf::neu(7).mit_von(3).schreiben(b"Opusdaten");
        let geheim = rahmen_verschluesseln(&klar, &schluessel, "raum_1").unwrap();

        // Kopf bleibt lesbar (der Server braucht ihn), Nutzlast nicht.
        let (kopf, nutzlast) = Sprachkopf::lesen(&geheim).unwrap();
        assert_eq!(kopf.folge, 7);
        assert_eq!(kopf.von, 3);
        assert_eq!(kopf.codec, CODEC_VERSCHLUESSELT);
        assert!(!nutzlast.windows(4).any(|w| w == b"Opus"));

        let zurueck = rahmen_entschluesseln(&geheim, &schluessel, "raum_1").unwrap();
        let (kopf2, nutzlast2) = Sprachkopf::lesen(&zurueck).unwrap();
        assert_eq!(kopf2.codec, CODEC_OPUS);
        assert_eq!(nutzlast2, b"Opusdaten");
    }

    #[test]
    fn klarer_rahmen_wird_nicht_als_verschluesselt_ausgegeben() {
        use sprechwerk_protocol::Sprachkopf;
        let schluessel = Raumschluessel::neu(1);
        let klar = Sprachkopf::neu(1).schreiben(b"abc");
        assert!(rahmen_entschluesseln(&klar, &schluessel, "raum_1").is_err());
    }

    #[test]
    fn zwei_epochen_haben_verschiedene_schluessel() {
        let alt = Raumschluessel::neu(1);
        let neu = Raumschluessel::neu(2);
        let geheim = alt.verschluesseln(0, "raum_1", b"abc").unwrap();
        assert!(neu.entschluesseln(0, "raum_1", &geheim).is_err());
    }

    // ── Schritt 2 (ADR 0010): hybrid, mit Forward Secrecy ──────────────────

    fn umschlag_lage() -> Raumschluessel {
        Raumschluessel::neu(7)
    }

    #[test]
    fn hybrider_umschlag_geht_fuer_die_richtige_sitzung_auf() {
        let empfaenger = Austausch::neu().unwrap();
        let fremd = FremderAustausch::aus_hex(&empfaenger.oeffentlich()).unwrap();
        let raum = "raum_a";
        let schluessel = umschlag_lage();

        let umschlag = verpacken_hybrid(&fremd, raum, &schluessel).unwrap();
        assert_eq!(umschlag.bytes.len(), HYBRIDER_UMSCHLAG_LEN);
        let geoeffnet = empfaenger
            .oeffnen(&umschlag, raum, schluessel.epoche)
            .unwrap();
        assert_eq!(geoeffnet.bytes(), schluessel.bytes());
        assert_eq!(geoeffnet.epoche, schluessel.epoche);
    }

    #[test]
    fn eine_andere_sitzung_oeffnet_den_umschlag_nicht() {
        // Das ist Forward Secrecy in einem Satz: der alte Umschlag gehört zu
        // einer Sitzung, die es nicht mehr gibt — auch mit der Schlüsseldatei
        // des Mitglieds kommt niemand mehr hinein.
        let alte = Austausch::neu().unwrap();
        let fremd = FremderAustausch::aus_hex(&alte.oeffentlich()).unwrap();
        let umschlag = verpacken_hybrid(&fremd, "raum_a", &umschlag_lage()).unwrap();

        let neue = Austausch::neu().unwrap();
        assert!(neue.oeffnen(&umschlag, "raum_a", 7).is_err());
    }

    #[test]
    fn ein_verfaelschter_hybrider_umschlag_faellt_auf() {
        let empfaenger = Austausch::neu().unwrap();
        let fremd = FremderAustausch::aus_hex(&empfaenger.oeffentlich()).unwrap();
        let mut umschlag = verpacken_hybrid(&fremd, "raum_a", &umschlag_lage()).unwrap();
        // Ein Bit im KEM-Anteil kippen: der geteilte Schlüssel wird ein anderer.
        umschlag.bytes[X25519_LEN + 40] ^= 1;
        assert!(empfaenger.oeffnen(&umschlag, "raum_a", 7).is_err());
    }

    #[test]
    fn raum_und_epoche_gehoeren_zum_umschlag() {
        let empfaenger = Austausch::neu().unwrap();
        let fremd = FremderAustausch::aus_hex(&empfaenger.oeffentlich()).unwrap();
        let umschlag = verpacken_hybrid(&fremd, "raum_a", &umschlag_lage()).unwrap();
        assert!(
            empfaenger.oeffnen(&umschlag, "raum_b", 7).is_err(),
            "ein Umschlag aus Raum A gilt nicht in Raum B"
        );
        assert!(
            empfaenger.oeffnen(&umschlag, "raum_a", 8).is_err(),
            "und nicht in einer anderen Epoche"
        );
    }

    #[test]
    fn die_veroeffentlichte_haelfte_ist_die_erwartete_groesse() {
        // 32 Byte X25519 + 1184 Byte ML-KEM-768 = 1216 Byte, hex 2432 Zeichen.
        // Wer hier etwas ändert, ändert den Vertrag mit der Gegenstelle.
        let a = Austausch::neu().unwrap();
        assert_eq!(a.oeffentlich().len(), AUSTAUSCH_LEN * 2);
        assert!(FremderAustausch::aus_hex(&a.oeffentlich()).is_ok());
    }

    #[test]
    fn kaputte_veroeffentlichungen_werden_abgelehnt() {
        assert!(FremderAustausch::aus_hex("").is_err());
        assert!(FremderAustausch::aus_hex("00").is_err());
        assert!(FremderAustausch::aus_hex(&"zz".repeat(AUSTAUSCH_LEN)).is_err());
    }

    #[test]
    fn ein_x25519_punkt_kleiner_ordnung_wird_abgelehnt() {
        // Der Nullpunkt (Ordnung 1) macht das klassische Geheimnis zu Nullen.
        // Ohne Prüfung bliebe nur ML-KEM übrig, und niemand merkte es.
        let empfaenger = Austausch::neu().unwrap();
        let fremd = FremderAustausch::aus_hex(&empfaenger.oeffentlich()).unwrap();
        let mut umschlag = verpacken_hybrid(&fremd, "raum_a", &umschlag_lage()).unwrap();
        umschlag.bytes[..X25519_LEN].fill(0);
        let fehler = empfaenger.oeffnen(&umschlag, "raum_a", 7).unwrap_err();
        assert!(fehler.to_string().contains("X25519"), "{fehler}");

        // Und umgekehrt: an einen veröffentlichten Nullpunkt wird nicht verpackt.
        let mut boese = hex::decode(empfaenger.oeffentlich()).unwrap();
        boese[..X25519_LEN].fill(0);
        let boese = FremderAustausch::aus_hex(&hex::encode(boese)).unwrap();
        assert!(verpacken_hybrid(&boese, "raum_a", &umschlag_lage()).is_err());
    }

    #[test]
    fn die_ableitung_haengt_an_den_x25519_werten() {
        // Gleiche Geheimnisse, anderer Austausch: anderer Schlüssel.
        let ss = [7u8; 32];
        let kem = [9u8; 32];
        let a = ableitung_hybrid(&ss, &kem, &[1; 32], &[2; 32], "r", 1).unwrap();
        let b = ableitung_hybrid(&ss, &kem, &[3; 32], &[2; 32], "r", 1).unwrap();
        let c = ableitung_hybrid(&ss, &kem, &[1; 32], &[4; 32], "r", 1).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ein_umschlag_der_falschen_laenge_wird_abgelehnt() {
        let a = Austausch::neu().unwrap();
        let kurz = Umschlag {
            bytes: vec![0u8; 10],
        };
        assert!(a.oeffnen(&kurz, "raum_a", 7).is_err());
    }
}

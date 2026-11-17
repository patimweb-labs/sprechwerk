//! Serverschlüssel: Chatnachrichten Ende zu Ende (ADR 0012).
//!
//! # Was hier passiert
//!
//! ```text
//! erster Client           Server (sieht nur Kennungen)        später Kommender
//! S = Zufall
//! ── Anlegen{id(S)} ──►   merkt id
//!                         ◄── Vorhanden{} ──────────────────── hat nichts
//! ◄── Anfrage{fuer, kennung, id} ──
//! Umschlag(S → kennung), unterschrieben
//! ── Paket ──────────────► leitet weiter ──────────────────►  prüft, öffnet,
//!                                                              id(S) == id?
//! ── Text: e1.id.AEAD(S, inhalt) ──► speichert Geheimtext ──►  entschlüsselt
//! ```
//!
//! Der Server kennt `S` nie. Er kann eine Nachricht nicht umhängen (Raum und
//! Verfasser stehen in den Zusatzdaten) und kein Paket fälschen (Unterschrift,
//! und die Kennung ist die Prüfsumme des Schlüssels).
//!
//! **Was er kann:** sich selbst als Mitglied anmelden — dann bekommt er den
//! Schlüssel wie jedes Mitglied, und er steht in der Mitgliederliste. Das ist
//! die Grenze jeder Gruppenverschlüsselung ohne Abgleich der Kennungen
//! außerhalb des Servers (ADR 0012, „Nicht versprochen").
//!
//! Krypto wie bei der Sprache (`verschluesselt`): ChaCha20-Poly1305, Umschlag
//! mit X25519 + HKDF, Ed25519-Unterschrift. Nichts davon ist selbst gebaut,
//! nur die Verpackung.

use crate::verschluesselt::{oeffentlich_aus_kennung, zu_signieren, Raumschluessel, Umschlag};
use crate::Schluessel;
use anyhow::{anyhow, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sha2::{Digest, Sha256};
use sprechwerk_domain::{MemberId, Message, RoomId};
use sprechwerk_protocol::inhalt::{Gelesen, Inhalt};
use sprechwerk_protocol::{geheimtext, ClientFrame, Schluesselstand};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Wie oft ein Client, dem ein Schlüssel fehlt, sich neu meldet. Der Server
/// bittet daraufhin einen Halter (höchstens im selben Abstand).
pub const NACHMELDEN: Duration = Duration::from_secs(5);

/// Ein Serverschlüssel.
#[derive(Clone)]
pub struct Serverschluessel {
    bytes: [u8; 32],
}

impl std::fmt::Debug for Serverschluessel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nie den Schlüssel selbst ausgeben.
        f.debug_struct("Serverschluessel")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

impl Serverschluessel {
    pub fn neu() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Zufall");
        Self { bytes }
    }

    pub fn aus_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Die Kennung: Prüfsumme des Schlüssels. Wer den Schlüssel hat, kann sie
    /// nachrechnen — einen falschen Schlüssel unter dieser Kennung gibt es nicht.
    pub fn id(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"sprechwerk serverschluessel v1");
        h.update(self.bytes);
        hex::encode(&h.finalize()[..16])
    }

    fn chiffre(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.bytes))
    }

    /// Verschlüsselt einen Inhalt für `raum`, geschrieben von `verfasser`.
    pub fn verschluesseln(&self, raum: RoomId, verfasser: MemberId, inhalt: &Inhalt) -> String {
        let id = self.id();
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).expect("Zufall");
        let aad = geheimtext::zusatzdaten(&raum.text(), &verfasser.text(), &id);
        let chiffre = self
            .chiffre()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &inhalt.schreiben(),
                    aad: &aad,
                },
            )
            .expect("ChaCha20-Poly1305 verschlüsselt jede Länge unter 256 GiB");
        let mut daten = Vec::with_capacity(12 + chiffre.len());
        daten.extend_from_slice(&nonce);
        daten.extend_from_slice(&chiffre);
        geheimtext::zusammensetzen(&id, &hex::encode(daten))
    }

    /// Entschlüsselt die Daten einer Hülle. Fehler heißt: verändert,
    /// umgehängt oder ein anderer Schlüssel.
    pub fn entschluesseln(
        &self,
        raum: RoomId,
        verfasser: MemberId,
        daten_hex: &str,
    ) -> Result<Vec<u8>> {
        let daten = hex::decode(daten_hex)?;
        if daten.len() < geheimtext::MIN_DATEN_BYTES {
            return Err(anyhow!("zu kurz"));
        }
        let (nonce, chiffre) = daten.split_at(12);
        let aad = geheimtext::zusatzdaten(&raum.text(), &verfasser.text(), &self.id());
        self.chiffre()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: chiffre,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Nachricht lässt sich nicht öffnen"))
    }
}

/// Was aus einer gespeicherten Nachricht wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lesbar {
    Inhalt(Inhalt),
    /// Eine Art, die dieser Client nicht kennt.
    Unbekannt {
        art: String,
    },
    /// Der Schlüssel fehlt (noch) — kommt, sobald ein Halter online ist.
    SchluesselFehlt,
    /// Verändert, umgehängt oder kaputt.
    Unlesbar,
    /// Gelöscht (leerer Text).
    Leer,
}

/// Grund, warum nicht geschrieben werden kann.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchreibFehler {
    #[error("Der Schlüssel zum Schreiben fehlt noch — er kommt, sobald ein Mitglied online ist, das ihn hat.")]
    SchluesselFehlt,
    #[error("Nachricht ist zu lang (höchstens {max} Zeichen)", max = sprechwerk_domain::MAX_MESSAGE_LEN)]
    ZuLang,
    #[error("Nachricht ist leer")]
    Leer,
}

/// Die Serverschlüssel dieses Clients für **einen** Server.
pub struct Bund {
    datei: Option<PathBuf>,
    /// Fingerabdruck des Servers — darunter stehen die Schlüssel in der Datei.
    server: String,
    schluessel: BTreeMap<String, Serverschluessel>,
    stand: Schluesselstand,
    /// Selbst erzeugt und angemeldet, aber noch nicht bestätigt.
    vorschlag: Option<Serverschluessel>,
    gemeldet: Option<Instant>,
}

/// Dateiform: `{ "<fingerabdruck>": ["<schlüssel hex>", …] }`.
type Datei = BTreeMap<String, Vec<String>>;

impl Bund {
    /// Lädt die gemerkten Schlüssel für `server` (Zertifikat-Fingerabdruck).
    /// Ohne Datei (Tests) lebt der Bund nur im Speicher.
    pub fn laden(datei: Option<PathBuf>, server: &str) -> Self {
        let mut schluessel = BTreeMap::new();
        if let Some(pfad) = &datei {
            let gelesen: Datei = std::fs::read(pfad)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            for hex_text in gelesen.get(server).into_iter().flatten() {
                if let Some(bytes) = hex::decode(hex_text)
                    .ok()
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                {
                    let s = Serverschluessel::aus_bytes(bytes);
                    schluessel.insert(s.id(), s);
                }
            }
        }
        Self {
            datei,
            server: server.to_string(),
            schluessel,
            stand: Schluesselstand::default(),
            vorschlag: None,
            gemeldet: None,
        }
    }

    fn speichern(&self) {
        let Some(pfad) = &self.datei else { return };
        let mut alles: Datei = std::fs::read(pfad)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        alles.insert(
            self.server.clone(),
            self.schluessel
                .values()
                .map(|s| hex::encode(s.bytes))
                .collect(),
        );
        if let Ok(json) = serde_json::to_vec_pretty(&alles) {
            if let Err(fehler) = crate::geheim_schreiben(pfad, &json) {
                eprintln!("Serverschlüssel nicht gespeichert: {fehler}");
            }
        }
    }

    fn merke(&mut self, s: Serverschluessel) {
        let id = s.id();
        if self.schluessel.insert(id, s).is_none() {
            self.speichern();
        }
    }

    /// Kann gerade geschrieben werden?
    pub fn kann_schreiben(&self) -> bool {
        self.stand
            .aktuell
            .as_ref()
            .is_some_and(|id| self.schluessel.contains_key(id))
    }

    /// Fehlt ein Schlüssel, den es auf dem Server gibt?
    pub fn fehlt_etwas(&self) -> bool {
        self.stand
            .alle
            .iter()
            .any(|id| !self.schluessel.contains_key(id))
    }

    fn vorhanden(&mut self) -> ClientFrame {
        self.gemeldet = Some(Instant::now());
        ClientFrame::ServerschluesselVorhanden {
            ids: self
                .stand
                .alle
                .iter()
                .filter(|id| self.schluessel.contains_key(*id))
                .cloned()
                .collect(),
        }
    }

    /// Neuer Stand vom Server (Begrüßung, Änderung). Liefert, was zu senden ist.
    ///
    /// * Kein aktueller Schlüssel: einen erzeugen und anmelden.
    /// * Der eigene Vorschlag wurde angenommen: übernehmen.
    /// * Immer: melden, was man hält — der Server vermittelt, was fehlt.
    pub fn stand(&mut self, stand: Schluesselstand) -> Vec<ClientFrame> {
        self.stand = stand;
        let mut aus = Vec::new();
        match self.stand.aktuell.clone() {
            None => {
                if self.vorschlag.is_none() {
                    let neu = Serverschluessel::neu();
                    aus.push(ClientFrame::ServerschluesselAnlegen { id: neu.id() });
                    self.vorschlag = Some(neu);
                }
            }
            Some(aktuell) => {
                if let Some(v) = self.vorschlag.take() {
                    // Angenommen — sonst war ein anderer schneller, und unser
                    // Vorschlag wurde nie benutzt.
                    if v.id() == aktuell {
                        self.merke(v);
                    }
                }
            }
        }
        aus.push(self.vorhanden());
        aus
    }

    /// Solange etwas fehlt: im Abstand [`NACHMELDEN`] erneut melden.
    pub fn nachmelden(&mut self) -> Option<ClientFrame> {
        let faellig = self.gemeldet.is_none_or(|w| w.elapsed() >= NACHMELDEN);
        (self.fehlt_etwas() && faellig).then(|| self.vorhanden())
    }

    /// Bitte des Servers: `fuer` (öffentlicher Schlüssel `kennung`) fehlt `id`.
    pub fn anfrage(
        &self,
        fuer: MemberId,
        kennung: &str,
        id: &str,
        eigener: &Schluessel,
    ) -> Result<ClientFrame> {
        let s = self
            .schluessel
            .get(id)
            .ok_or_else(|| anyhow!("diesen Serverschlüssel habe ich nicht"))?;
        let empfaenger =
            oeffentlich_aus_kennung(kennung).ok_or_else(|| anyhow!("Kennung unbrauchbar"))?;
        let zweck = zweck(id);
        let umschlag = Umschlag::verpacken(
            &eigener.austausch_geheimnis(),
            &empfaenger,
            &zweck,
            &Raumschluessel::aus_bytes(0, s.bytes),
        )?;
        let unterschrift = eigener.unterschreibe(&zu_signieren(&zweck, 0, &umschlag.bytes));
        Ok(ClientFrame::ServerschluesselPaket {
            fuer,
            id: id.to_string(),
            daten: hex::encode(&umschlag.bytes),
            unterschrift,
        })
    }

    /// Ein Paket annehmen: Unterschrift prüfen, öffnen, Kennung nachrechnen.
    /// Liefert die neue Meldung an den Server.
    pub fn paket(
        &mut self,
        von_kennung: &str,
        id: &str,
        daten: &str,
        unterschrift: &str,
        eigener: &Schluessel,
    ) -> Result<ClientFrame> {
        let roh = hex::decode(daten)?;
        let zweck = zweck(id);
        if !crate::pruefe_beweis(von_kennung, &zu_signieren(&zweck, 0, &roh), unterschrift) {
            return Err(anyhow!("Unterschrift des Schlüsselpakets stimmt nicht"));
        }
        let absender =
            oeffentlich_aus_kennung(von_kennung).ok_or_else(|| anyhow!("Kennung unbrauchbar"))?;
        let k = Umschlag { bytes: roh }.oeffnen(
            &eigener.austausch_geheimnis(),
            &absender,
            &zweck,
            0,
        )?;
        let s = Serverschluessel::aus_bytes(k.bytes());
        if s.id() != id {
            return Err(anyhow!("Schlüssel passt nicht zu seiner Kennung"));
        }
        self.merke(s);
        Ok(self.vorhanden())
    }

    /// Verschlüsselt mit dem aktuellen Schlüssel.
    pub fn verschluesseln(
        &self,
        raum: RoomId,
        ich: MemberId,
        inhalt: &Inhalt,
    ) -> Result<String, SchreibFehler> {
        match inhalt {
            Inhalt::Text { text, .. } => {
                if text.trim().is_empty() {
                    return Err(SchreibFehler::Leer);
                }
                if text.chars().count() > sprechwerk_domain::MAX_MESSAGE_LEN {
                    return Err(SchreibFehler::ZuLang);
                }
            }
            Inhalt::Reaktion { zeichen, .. } => {
                if zeichen.trim().is_empty() {
                    return Err(SchreibFehler::Leer);
                }
                if zeichen.len() > sprechwerk_protocol::inhalt::MAX_ZEICHEN_BYTES {
                    return Err(SchreibFehler::ZuLang);
                }
            }
        }
        // Mit Vorschaubild kann der Umschlag wachsen — über der Grenze nähme
        // ihn der Server nicht an.
        if inhalt.schreiben().len() > sprechwerk_protocol::geheimtext::MAX_INHALT_BYTES {
            return Err(SchreibFehler::ZuLang);
        }
        let s = self
            .stand
            .aktuell
            .as_ref()
            .and_then(|id| self.schluessel.get(id))
            .ok_or(SchreibFehler::SchluesselFehlt)?;
        Ok(s.verschluesseln(raum, ich, inhalt))
    }

    /// Liest eine gespeicherte Nachricht.
    pub fn lesen(&self, n: &Message) -> Lesbar {
        if n.text.is_empty() {
            return Lesbar::Leer;
        }
        let Some(huelle) = geheimtext::zerlegen(&n.text) else {
            return Lesbar::Unlesbar;
        };
        let Some(s) = self.schluessel.get(huelle.id) else {
            return Lesbar::SchluesselFehlt;
        };
        match s.entschluesseln(n.raum, n.verfasser, huelle.daten) {
            Ok(bytes) => match Inhalt::lesen(&bytes) {
                Gelesen::Inhalt(i) => Lesbar::Inhalt(i),
                Gelesen::Unbekannt { art } => Lesbar::Unbekannt { art },
                Gelesen::Kaputt => Lesbar::Unlesbar,
            },
            Err(_) => Lesbar::Unlesbar,
        }
    }
}

/// Wofür ein Umschlag gilt — damit ein Paket für einen Raumschlüssel nicht als
/// Serverschlüssel durchgeht und umgekehrt.
fn zweck(id: &str) -> String {
    format!("serverschluessel:{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nachricht(raum: RoomId, verfasser: MemberId, text: String) -> Message {
        Message {
            id: sprechwerk_domain::MessageId::neu(),
            raum,
            verfasser,
            verfasser_name: "Ada".into(),
            text,
            zeit: 0,
            geloescht: false,
        }
    }

    fn text(t: &str) -> Inhalt {
        Inhalt::text(t)
    }

    fn stand(id: &str) -> Schluesselstand {
        Schluesselstand {
            aktuell: Some(id.into()),
            alle: vec![id.into()],
        }
    }

    /// Erster Client: legt an, übernimmt nach der Bestätigung, schreibt.
    #[test]
    fn erster_legt_an_und_schreibt() {
        let mut ada = Bund::laden(None, "fp");
        let aus = ada.stand(Schluesselstand::default());
        let ClientFrame::ServerschluesselAnlegen { id } = &aus[0] else {
            panic!("erwartet Anlegen: {aus:?}");
        };
        assert!(!ada.kann_schreiben(), "vor der Bestätigung nicht");
        // Ein zweiter Stand ohne Schlüssel erzeugt keinen zweiten Vorschlag.
        assert!(ada
            .stand(Schluesselstand::default())
            .iter()
            .all(|f| !matches!(f, ClientFrame::ServerschluesselAnlegen { .. })));
        ada.stand(stand(id));
        assert!(ada.kann_schreiben());

        let raum = RoomId::neu();
        let ich = MemberId::neu();
        let geheim = ada.verschluesseln(raum, ich, &text("Kasse: 12 €")).unwrap();
        assert!(!geheim.contains("Kasse"), "Klartext im Geheimtext");
        assert_eq!(geheimtext::zerlegen(&geheim).unwrap().id, id.as_str());
        assert_eq!(
            ada.lesen(&nachricht(raum, ich, geheim)),
            Lesbar::Inhalt(text("Kasse: 12 €"))
        );
    }

    /// Kommt ein anderer zuvor, wird der eigene Vorschlag verworfen.
    #[test]
    fn zu_spaet_verwirft_den_vorschlag() {
        let mut b = Bund::laden(None, "fp");
        b.stand(Schluesselstand::default());
        b.stand(stand(&"cd".repeat(16)));
        assert!(!b.kann_schreiben());
        assert!(b.fehlt_etwas());
        assert!(
            b.schluessel.is_empty(),
            "verworfener Vorschlag wurde gemerkt"
        );
    }

    /// Der Server hängt um oder schreibt einem anderen zu: unlesbar.
    #[test]
    fn umgehaengt_ist_unlesbar() {
        let mut ada = Bund::laden(None, "fp");
        let ClientFrame::ServerschluesselAnlegen { id } =
            ada.stand(Schluesselstand::default()).remove(0)
        else {
            panic!()
        };
        ada.stand(stand(&id));
        let (raum, ich) = (RoomId::neu(), MemberId::neu());
        let geheim = ada.verschluesseln(raum, ich, &text("hallo")).unwrap();
        assert_eq!(
            ada.lesen(&nachricht(RoomId::neu(), ich, geheim.clone())),
            Lesbar::Unlesbar
        );
        assert_eq!(
            ada.lesen(&nachricht(raum, MemberId::neu(), geheim.clone())),
            Lesbar::Unlesbar
        );
        // Ein Zeichen verändert.
        let mut kaputt = geheim.clone();
        let letzte = kaputt.pop().unwrap();
        kaputt.push(if letzte == '0' { '1' } else { '0' });
        assert_eq!(ada.lesen(&nachricht(raum, ich, kaputt)), Lesbar::Unlesbar);
        // Ohne Schlüssel: wartet, nicht kaputt.
        let leer = Bund::laden(None, "fp");
        assert_eq!(
            leer.lesen(&nachricht(raum, ich, geheim)),
            Lesbar::SchluesselFehlt
        );
        assert_eq!(
            leer.lesen(&nachricht(raum, ich, String::new())),
            Lesbar::Leer
        );
    }

    /// Weitergabe von Ada an Bosse über ein unterschriebenes Paket.
    #[test]
    fn weitergabe_mit_unterschrift_und_pruefsumme() {
        let ada_k = Schluessel::neu();
        let bosse_k = Schluessel::neu();
        let mut ada = Bund::laden(None, "fp");
        let ClientFrame::ServerschluesselAnlegen { id } =
            ada.stand(Schluesselstand::default()).remove(0)
        else {
            panic!()
        };
        ada.stand(stand(&id));
        let mut bosse = Bund::laden(None, "fp");
        bosse.stand(stand(&id));
        assert!(bosse.fehlt_etwas());

        let bosse_id = MemberId::neu();
        let ClientFrame::ServerschluesselPaket {
            fuer,
            id: pid,
            daten,
            unterschrift,
        } = ada
            .anfrage(bosse_id, &bosse_k.kennung(), &id, &ada_k)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(fuer, bosse_id);
        assert!(
            !daten.contains(&hex::encode(ada.schluessel[&id].bytes)),
            "Schlüssel offen im Paket"
        );

        // Gefälschte Unterschrift (der Server behauptet, Clemens schicke es).
        let clemens = Schluessel::neu();
        assert!(bosse
            .paket(&clemens.kennung(), &pid, &daten, &unterschrift, &bosse_k)
            .is_err());
        // Für jemand anderen verpackt: lässt sich nicht öffnen.
        assert!(bosse
            .paket(&ada_k.kennung(), &pid, &daten, &unterschrift, &clemens)
            .is_err());
        // Unter falscher Kennung angeboten.
        let falsch = "ef".repeat(16);
        assert!(bosse
            .paket(&ada_k.kennung(), &falsch, &daten, &unterschrift, &bosse_k)
            .is_err());

        let meldung = bosse
            .paket(&ada_k.kennung(), &pid, &daten, &unterschrift, &bosse_k)
            .unwrap();
        assert_eq!(
            meldung,
            ClientFrame::ServerschluesselVorhanden {
                ids: vec![id.clone()]
            }
        );
        assert!(bosse.kann_schreiben());
        let (raum, ich) = (RoomId::neu(), MemberId::neu());
        let von_ada = ada.verschluesseln(raum, ich, &text("Willkommen")).unwrap();
        assert_eq!(
            bosse.lesen(&nachricht(raum, ich, von_ada)),
            Lesbar::Inhalt(text("Willkommen"))
        );
    }

    #[test]
    fn schreiben_prueft_laenge_und_schluessel() {
        let leer = Bund::laden(None, "fp");
        let (raum, ich) = (RoomId::neu(), MemberId::neu());
        assert_eq!(
            leer.verschluesseln(raum, ich, &text("x")),
            Err(SchreibFehler::SchluesselFehlt)
        );
        let mut ada = Bund::laden(None, "fp");
        let ClientFrame::ServerschluesselAnlegen { id } =
            ada.stand(Schluesselstand::default()).remove(0)
        else {
            panic!()
        };
        ada.stand(stand(&id));
        assert_eq!(
            ada.verschluesseln(raum, ich, &text("  ")),
            Err(SchreibFehler::Leer)
        );
        let lang = "ä".repeat(sprechwerk_domain::MAX_MESSAGE_LEN + 1);
        assert_eq!(
            ada.verschluesseln(raum, ich, &text(&lang)),
            Err(SchreibFehler::ZuLang)
        );
        // An der Grenze, mit Mehrbyte-Zeichen: passt in die Hülle.
        let genau = "😀".repeat(sprechwerk_domain::MAX_MESSAGE_LEN);
        let geheim = ada.verschluesseln(raum, ich, &text(&genau)).unwrap();
        assert!(geheimtext::zerlegen(&geheim).is_some());
    }

    /// Gemerkt je Server, in einer Datei nur für den Eigentümer.
    #[test]
    fn bund_ueberlebt_den_neustart() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-bund-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let datei = dir.join("serverschluessel.json");
        let mut a = Bund::laden(Some(datei.clone()), "server-a");
        let ClientFrame::ServerschluesselAnlegen { id } =
            a.stand(Schluesselstand::default()).remove(0)
        else {
            panic!()
        };
        a.stand(stand(&id));
        let mut b = Bund::laden(Some(datei.clone()), "server-b");
        let ClientFrame::ServerschluesselAnlegen { id: id_b } =
            b.stand(Schluesselstand::default()).remove(0)
        else {
            panic!()
        };
        b.stand(stand(&id_b));

        let mut wieder = Bund::laden(Some(datei.clone()), "server-a");
        wieder.stand(stand(&id));
        assert!(wieder.kann_schreiben());
        assert!(
            !wieder.schluessel.contains_key(&id_b),
            "Schlüssel eines anderen Servers"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let rechte = std::fs::metadata(&datei).unwrap().permissions().mode() & 0o777;
            assert_eq!(rechte, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn meldet_nach_solange_etwas_fehlt() {
        let mut b = Bund::laden(None, "fp");
        b.stand(stand(&"cd".repeat(16)));
        assert!(b.nachmelden().is_none(), "gerade gemeldet");
        b.gemeldet = Some(Instant::now() - NACHMELDEN);
        assert!(matches!(
            b.nachmelden(),
            Some(ClientFrame::ServerschluesselVorhanden { .. })
        ));
        // Nichts fehlt: keine Meldung.
        let mut voll = Bund::laden(None, "fp");
        voll.stand(Schluesselstand::default());
        voll.gemeldet = None;
        assert!(voll.nachmelden().is_none());
    }
}

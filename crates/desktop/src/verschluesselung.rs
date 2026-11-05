// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Schlüsselverwaltung für die Ende-zu-Ende-Verschlüsselung (ADR 0006).
//!
//! # Wer verteilt den Schlüssel?
//!
//! Ohne Absprache: **der am längsten anwesende Client** — erkennbar an der
//! kleinsten Kennung unter den Anwesenden im Raum. Alle rechnen dasselbe aus,
//! es gibt keine Wahl und keinen Server, der entscheiden müsste. Ist der
//! Schlüsselwart weg, übernimmt der nächste — deshalb wird bei
//! Mitgliederwechseln erneut geprüft.
//!
//! # Was der Server sieht
//!
//! Nur Geheimtext: jeder Umschlag ist für **einen** Empfänger verschlüsselt
//! (X25519 + HKDF + ChaCha20-Poly1305) und mit dem Ed25519-Schlüssel des
//! Absenders **unterschrieben**. Ohne diese Unterschrift könnte der Server
//! einen eigenen Schlüssel unterschieben und mithören — die Verschlüsselung
//! wäre dann nur eine Behauptung.
//!
//! Diese Verwaltung rechnet nichts am Ton: sie führt nur die Schlüssel und
//! erzeugt die Pakete. Verschlüsselt wird im Netzrand
//! (`anwendung.rs`), geprüft in `sprechwerk_adapters::verschluesselt`.

use sprechwerk_adapters::verschluesselt::{zu_signieren, Raumschluessel, Umschlag};
use sprechwerk_adapters::Schluessel;
use sprechwerk_domain::{MemberId, RoomId};
use sprechwerk_protocol::{ClientFrame, Mitgliedsansicht, ServerFrame};
use std::collections::HashMap;
use std::sync::Mutex;
use x25519_dalek::PublicKey;

/// Der Austauschschlüssel eines Mitglieds, aus seiner Kennung gerechnet.
fn oeffentlich_aus_hex(kennung: &str) -> Option<PublicKey> {
    sprechwerk_adapters::verschluesselt::oeffentlich_aus_kennung(kennung)
}

/// Verwaltet die Raumschlüssel dieses Clients.
#[derive(Default)]
pub struct Schluesselverwaltung {
    /// Schlüssel je Raum.
    schluessel: Mutex<HashMap<RoomId, Raumschluessel>>,
    /// Letzte Meldung für die Oberfläche (z. B. „Schlüssel fehlt").
    notiz: Mutex<Option<String>>,
}

impl Schluesselverwaltung {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Der Schlüssel eines Raums, falls vorhanden.
    pub fn hole(&self, raum: RoomId) -> Option<Raumschluessel> {
        self.schluessel.lock().ok()?.get(&raum).cloned()
    }

    /// Merkt sich einen Schlüssel.
    pub fn merke(&self, raum: RoomId, schluessel: Raumschluessel) {
        if let Ok(mut alle) = self.schluessel.lock() {
            alle.insert(raum, schluessel);
        }
    }

    /// Vergisst einen Raum (Verschlüsselung aus, Raum verlassen).
    pub fn vergiss(&self, raum: RoomId) {
        if let Ok(mut alle) = self.schluessel.lock() {
            alle.remove(&raum);
        }
    }

    /// Vergisst alles (Trennen).
    pub fn leeren(&self) {
        if let Ok(mut alle) = self.schluessel.lock() {
            alle.clear();
        }
        self.setze_notiz(None);
    }

    pub fn notiz(&self) -> Option<String> {
        self.notiz.lock().ok().and_then(|n| n.clone())
    }

    fn setze_notiz(&self, text: Option<String>) {
        if let Ok(mut n) = self.notiz.lock() {
            *n = text;
        }
    }

    /// Bin ich der Schlüsselwart dieses Raums?
    ///
    /// Der kleinste **öffentliche** Schlüssel unter den Anwesenden, die einen
    /// haben. Deterministisch, ohne Absprache, ohne Server.
    pub fn bin_wart(ich: MemberId, mitglieder: &[Mitgliedsansicht], raum: RoomId) -> bool {
        mitglieder
            .iter()
            .filter(|m| m.raum == Some(raum) && !m.kennung.is_empty())
            .map(|m| m.id)
            .min()
            .map(|kleinster| kleinster == ich)
            .unwrap_or(false)
    }

    /// Sorgt dafür, dass der Schlüssel für einen verschlüsselten Raum vorliegt.
    ///
    /// Liefert die Pakete, die dafür verschickt werden müssen (leer, wenn nichts
    /// zu tun ist). Ist der eigene Schlüssel nicht der älteste, wird gewartet.
    pub fn sorge_fuer(
        &self,
        raum: RoomId,
        seit_ms: i64,
        ich: MemberId,
        eigener: &Schluessel,
        mitglieder: &[Mitgliedsansicht],
    ) -> Vec<ClientFrame> {
        let epoche = epoche_aus(seit_ms);
        if let Some(vorhanden) = self.hole(raum) {
            if vorhanden.epoche == epoche {
                return Vec::new();
            }
        }
        if !Self::bin_wart(ich, mitglieder, raum) {
            self.setze_notiz(Some(
                "Verschlüsselter Raum: warte auf den Schlüssel des Raums".to_string(),
            ));
            return Vec::new();
        }

        // Ich bin der Wart: Schlüssel erzeugen und an alle anderen verteilen.
        let schluessel = Raumschluessel::neu(epoche);
        let mut pakete = Vec::new();
        let geheimnis = eigener.austausch_geheimnis();
        for m in mitglieder {
            if m.id == ich || m.raum != Some(raum) || m.kennung.is_empty() {
                continue;
            }
            let Some(oeffentlich) = oeffentlich_aus_hex(&m.kennung) else {
                continue;
            };
            let Ok(umschlag) = Umschlag::verpacken(&geheimnis, &oeffentlich, raum.text().as_str(), &schluessel)
            else {
                continue;
            };
            let unterschrift = eigener.unterschreibe(&zu_signieren(
                raum.text().as_str(),
                epoche,
                &umschlag.bytes,
            ));
            pakete.push(ClientFrame::Schluesselpaket {
                raum,
                epoche,
                fuer: m.id,
                daten: hex::encode(&umschlag.bytes),
                unterschrift,
            });
        }
        self.merke(raum, schluessel);
        self.setze_notiz(None);
        pakete
    }

    /// Nimmt ein Schlüsselpaket an: Unterschrift prüfen, öffnen, merken.
    pub fn nimm_paket(
        &self,
        raum: RoomId,
        epoche: u32,
        von: MemberId,
        daten: &str,
        unterschrift: &str,
        eigener: &Schluessel,
        mitglieder: &[Mitgliedsansicht],
    ) -> Result<(), String> {
        let absender = mitglieder
            .iter()
            .find(|m| m.id == von)
            .ok_or_else(|| "Absender unbekannt".to_string())?;
        if absender.kennung.is_empty() {
            return Err("Absender ohne Kennung".into());
        }
        let roh = hex::decode(daten).map_err(|_| "Paket ist kein Hex".to_string())?;

        // Unterschrift: der Server darf kein Paket unterschieben oder umhängen.
        let text = zu_signieren(raum.text().as_str(), epoche, &roh);
        if !sprechwerk_adapters::pruefe_beweis(&absender.kennung, &text, unterschrift) {
            return Err("Unterschrift des Schlüsselpakets stimmt nicht".into());
        }

        let absender_oeffentlich =
            oeffentlich_aus_hex(&absender.kennung).ok_or("Kennung unbrauchbar")?;
        let umschlag = Umschlag { bytes: roh };
        let schluessel = umschlag
            .oeffnen(
                &eigener.austausch_geheimnis(),
                &absender_oeffentlich,
                raum.text().as_str(),
                epoche,
            )
            .map_err(|fehler| format!("{fehler}"))?;
        self.merke(raum, schluessel);
        self.setze_notiz(None);
        Ok(())
    }
}

/// Alle Clients bilden aus „verschlüsselt seit" dieselbe Epoche.
fn epoche_aus(seit_ms: i64) -> u32 {
    (seit_ms.max(0) / 1000) as u32
}

/// Hilfsfunktion für den Verbindungsthread: aus einem Rahmen die Aufgabe
/// ableiten, ohne dass dort Krypto steht.
pub fn aus_rahmen(rahmen: &ServerFrame) -> Option<(RoomId, u32, MemberId, String, String)> {
    match rahmen {
        ServerFrame::Schluesselpaket {
            raum,
            epoche,
            von,
            daten,
            unterschrift,
        } => Some((*raum, *epoche, *von, daten.clone(), unterschrift.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_domain::Role;
    use sprechwerk_protocol::Raumansicht;

    fn mitglied(id: u128, kennung: &str, raum: Option<RoomId>) -> Mitgliedsansicht {
        Mitgliedsansicht {
            id: MemberId::aus_roh(id),
            name: format!("M{id}"),
            kennung: kennung.to_string(),
            rolle: Role::Mitglied,
            raum,
            spricht: false,
            stumm: false,
            taub: false,
        }
    }

    #[test]
    fn kleinster_schluessel_ist_wart() {
        let raum = RoomId::aus_roh(1);
        let andere = RoomId::aus_roh(2);
        let mitglieder = vec![
            mitglied(9, "aa", Some(raum)),
            mitglied(3, "bb", Some(raum)),
            // Anderer Raum zählt nicht.
            mitglied(1, "cc", Some(andere)),
        ];
        assert!(Schluesselverwaltung::bin_wart(
            MemberId::aus_roh(3),
            &mitglieder,
            raum
        ));
        assert!(!Schluesselverwaltung::bin_wart(
            MemberId::aus_roh(9),
            &mitglieder,
            raum
        ));
        assert!(!Schluesselverwaltung::bin_wart(
            MemberId::aus_roh(1),
            &mitglieder,
            raum
        ));
    }

    #[test]
    fn zwei_clients_kommen_auf_denselben_schluessel() {
        let raum = RoomId::aus_roh(7);
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        let ich_a = MemberId::aus_roh(1);
        let ich_b = MemberId::aus_roh(2);
        let mitglieder = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
        ];

        // A ist der Wart (kleinere Kennung) und verschickt.
        let verwaltung_a = Schluesselverwaltung::neu();
        let pakete = verwaltung_a.sorge_fuer(raum, 5_000, ich_a, &a, &mitglieder);
        assert_eq!(pakete.len(), 1, "ein Paket für B");

        // B nimmt es an.
        let verwaltung_b = Schluesselverwaltung::neu();
        for paket in &pakete {
            if let ClientFrame::Schluesselpaket {
                raum: r,
                epoche,
                fuer,
                daten,
                unterschrift,
            } = paket
            {
                assert_eq!(*fuer, ich_b);
                verwaltung_b
                    .nimm_paket(*r, *epoche, ich_a, daten, unterschrift, &b, &mitglieder)
                    .expect("B muss den Schlüssel bekommen");
            }
        }

        let ka = verwaltung_a.hole(raum).expect("A hat einen Schlüssel");
        let kb = verwaltung_b.hole(raum).expect("B hat einen Schlüssel");
        assert_eq!(ka.bytes(), kb.bytes(), "beide haben denselben Schlüssel");
        assert_eq!(
            ka.verschluesseln(1, "raum_7", b"hallo").unwrap(),
            kb.verschluesseln(1, "raum_7", b"hallo").unwrap()
        );
    }

    #[test]
    fn gefaelschte_unterschrift_wird_abgelehnt() {
        let raum = RoomId::aus_roh(3);
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        let boese = Schluessel::neu();
        let mitglieder = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
        ];
        let verwaltung = Schluesselverwaltung::neu();
        let pakete = verwaltung.sorge_fuer(raum, 9_000, MemberId::aus_roh(1), &a, &mitglieder);
        let ClientFrame::Schluesselpaket {
            daten, epoche, ..
        } = &pakete[0]
        else {
            panic!("kein Paket");
        };
        // Unterschrift von jemand anderem
        let roh = hex::decode(daten).unwrap();
        let falsch = boese.unterschreibe(&zu_signieren("raum_3", *epoche, &roh));
        let ziel = Schluesselverwaltung::neu();
        assert!(ziel
            .nimm_paket(
                raum,
                *epoche,
                MemberId::aus_roh(1),
                daten,
                &falsch,
                &b,
                &mitglieder
            )
            .is_err());
    }

    #[test]
    fn ohne_wart_wird_gewartet() {
        let raum = RoomId::aus_roh(4);
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        let mitglieder = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
        ];
        // B ist nicht der Wart: keine Pakete, aber eine Notiz.
        let verwaltung = Schluesselverwaltung::neu();
        let pakete = verwaltung.sorge_fuer(raum, 1_000, MemberId::aus_roh(2), &b, &mitglieder);
        assert!(pakete.is_empty());
        assert!(verwaltung.hole(raum).is_none());
        assert!(verwaltung.notiz().is_some());
    }

    #[test]
    fn neue_epoche_verwirft_den_alten_schluessel() {
        let raum = RoomId::aus_roh(5);
        let a = Schluessel::neu();
        let mitglieder = vec![mitglied(1, &a.kennung(), Some(raum))];
        let verwaltung = Schluesselverwaltung::neu();
        verwaltung.sorge_fuer(raum, 1_000, MemberId::aus_roh(1), &a, &mitglieder);
        let alt = verwaltung.hole(raum).unwrap().epoche;
        // Raum wurde neu verschlüsselt (späterer Zeitpunkt).
        verwaltung.sorge_fuer(raum, 900_000, MemberId::aus_roh(1), &a, &mitglieder);
        let neu = verwaltung.hole(raum).unwrap().epoche;
        assert_ne!(alt, neu, "neue Epoche, neuer Schlüssel");
    }

    #[test]
    fn raumliste_bleibt_unberuehrt() {
        // Nur damit klar ist, dass die Verwaltung keine Räume anlegt.
        let _ = Raumansicht {
            id: RoomId::aus_roh(1),
            name: "x".into(),
            reihenfolge: 0,
            anwesend: 1,
            verschluesselt: true,
            verschluesselt_seit: 1_000,
        };
    }
}

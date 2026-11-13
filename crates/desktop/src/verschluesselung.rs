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
/// Wie lange der **vorige** Schlüssel nach einem Wechsel noch gilt.
///
/// Der Wechsel braucht einen Rahmen Zeit (gemessen 35–50 ms, siehe ADR 0006):
/// die Verbliebenen haben den neuen Schlüssel noch nicht, wenn der erste
/// Rahmen damit schon unterwegs ist. Ohne Übergang gingen diese Rahmen
/// verloren — mit ihm nicht. Zu lang darf das Fenster nicht sein, sonst gilt
/// ein Schlüssel weiter, obwohl jemand den Raum verlassen hat.
pub const UEBERGANG: std::time::Duration = std::time::Duration::from_millis(1_500);

/// Wie lange ein Wartender zwischen zwei Nachforderungen wartet.
///
/// Ohne Nachforderung bliebe er für immer still, wenn das Schlüsselpaket
/// verloren ging (der Wart hat den Schlüssel ja und schickt von selbst nichts).
pub const ANFRAGE_ABSTAND: std::time::Duration = std::time::Duration::from_millis(1_000);

pub struct Schluesselverwaltung {
    /// Schlüssel je Raum.
    schluessel: Mutex<HashMap<RoomId, Raumschluessel>>,
    /// Letzte Meldung für die Oberfläche (z. B. „Schlüssel fehlt").
    notiz: Mutex<Option<String>>,
    /// Der vorige Schlüssel je Raum, mit dem Zeitpunkt des Wechsels — nur für
    /// das Übergangsfenster ([`UEBERGANG`]).
    vorher: Mutex<HashMap<RoomId, (Raumschluessel, std::time::Instant)>>,
    /// Dauer des Übergangsfensters (in Tests kürzer).
    uebergang: std::time::Duration,
    /// Wann zuletzt nach dem Schlüssel gefragt wurde (je Raum).
    angefragt: Mutex<HashMap<RoomId, std::time::Instant>>,
    /// Für welche Anwesenden in dieser Epoche schon verteilt wurde.
    ///
    /// Nötig, weil ein **Späterkommender** sonst nie einen Schlüssel bekäme: der
    /// Wart hat ja einen, und `sorge_fuer` täte nichts. Ändert sich die Liste der
    /// Anwesenden, wird erneut verteilt — das deckt Kommen, Gehen und
    /// Wiederkommen ab (jedes ändert die Liste).
    ///
    /// Geht ein Schlüsselpaket **verloren**, fragt der Wartende selbst nach
    /// (`SchluesselAnfrage`), und zwar höchstens alle [`ANFRAGE_ABSTAND`].
    /// Geht auch die Bitte verloren, wiederholt sie sich nach diesem Abstand —
    /// deshalb ist die Frist kurz (1 s) und nicht Minuten. Geprüft ist beides
    /// Ende zu Ende: `verlorenes_schluesselpaket_wird_nachgefordert` und
    /// `verlorene_bitte_wiederholt_sich`.
    verteilt: Mutex<HashMap<RoomId, (u32, Vec<MemberId>)>>,
}

impl Default for Schluesselverwaltung {
    fn default() -> Self {
        Self {
            schluessel: Mutex::new(HashMap::new()),
            vorher: Mutex::new(HashMap::new()),
            uebergang: UEBERGANG,
            angefragt: Mutex::new(HashMap::new()),
            notiz: Mutex::new(None),
            verteilt: Mutex::new(HashMap::new()),
        }
    }
}

impl Schluesselverwaltung {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Setzt die Dauer des Übergangsfensters (nur für Prüfungen gedacht).
    pub fn mit_uebergang(dauer: std::time::Duration) -> Self {
        Self {
            uebergang: dauer,
            ..Self::default()
        }
    }

    /// Der Schlüssel eines Raums, falls vorhanden.
    pub fn hole(&self, raum: RoomId) -> Option<Raumschluessel> {
        self.schluessel.lock().ok()?.get(&raum).cloned()
    }

    /// Merkt sich einen Schlüssel.
    ///
    /// Wechselt dabei die Epoche, bleibt der **vorige** Schlüssel noch für
    /// [`UEBERGANG`] gültig. Das ist die Überlappung aus ADR 0006 §6: ohne sie
    /// gingen die Rahmen verloren, die während des Wechsels unterwegs sind.
    pub fn merke(&self, raum: RoomId, schluessel: Raumschluessel) {
        if let Ok(mut alle) = self.schluessel.lock() {
            if let Some(alt) = alle.get(&raum) {
                if alt.epoche != schluessel.epoche {
                    if let Ok(mut vorher) = self.vorher.lock() {
                        vorher.insert(raum, (alt.clone(), std::time::Instant::now()));
                    }
                }
            }
            alle.insert(raum, schluessel);
        }
    }

    /// Entschlüsselt einen Rahmen — mit dem aktuellen Schlüssel, sonst mit dem
    /// vorigen, solange das Übergangsfenster offen ist.
    pub fn entschluesseln(&self, raum: RoomId, datagramm: &[u8]) -> Option<Vec<u8>> {
        let raum_text = raum.text();
        if let Some(schluessel) = self.hole(raum) {
            if let Ok(klar) = sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(
                datagramm,
                &schluessel,
                &raum_text,
            ) {
                return Some(klar);
            }
        }
        let (alt, gewechselt) = {
            let vorher = self.vorher.lock().ok()?;
            vorher.get(&raum).cloned()?
        };
        if gewechselt.elapsed() > self.uebergang {
            return None;
        }
        sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(datagramm, &alt, &raum_text).ok()
    }

    /// Vergisst einen Raum (Verschlüsselung aus, Raum verlassen).
    pub fn vergiss(&self, raum: RoomId) {
        if let Ok(mut alle) = self.schluessel.lock() {
            alle.remove(&raum);
        }
        if let Ok(mut vorher) = self.vorher.lock() {
            vorher.remove(&raum);
        }
        if let Ok(mut angefragt) = self.angefragt.lock() {
            angefragt.remove(&raum);
        }
        if let Ok(mut verteilt) = self.verteilt.lock() {
            verteilt.remove(&raum);
        }
    }

    /// Vergisst alles (Trennen).
    pub fn leeren(&self) {
        if let Ok(mut alle) = self.schluessel.lock() {
            alle.clear();
        }
        if let Ok(mut vorher) = self.vorher.lock() {
            vorher.clear();
        }
        if let Ok(mut angefragt) = self.angefragt.lock() {
            angefragt.clear();
        }
        if let Ok(mut verteilt) = self.verteilt.lock() {
            verteilt.clear();
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
    /// Der **am längsten anwesende** Client, der eine Kennung hat (ADR 0006 §2).
    /// Deterministisch aus der Mitgliederliste, ohne Absprache, ohne Server.
    ///
    /// Warum nicht „kleinste Kennung" (so stand es hier bis 2026-11-13): kam
    /// jemand mit kleinerer Kennung dazu, wechselte der Wart **mitten in der
    /// Epoche**. Der Neue erzeugte seinen eigenen Schlüssel und verteilte ihn;
    /// erreichte der Verteilung nicht alle, gab es zwei Schlüssel für dieselbe
    /// Epoche — die Betroffenen hörten einander nicht mehr, ohne Fehlermeldung.
    /// Mit „am längsten anwesend" kann ein Neuankömmling den Wart nicht
    /// ablösen.
    pub fn bin_wart(ich: MemberId, mitglieder: &[Mitgliedsansicht], raum: RoomId) -> bool {
        mitglieder
            .iter()
            .filter(|m| m.raum == Some(raum) && !m.kennung.is_empty())
            .min_by_key(|m| (m.raum_seit, m.id))
            .map(|aeltester| aeltester.id == ich)
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

        // Wer ist gerade da? (ohne mich, nur mit Kennung)
        let anwesend: Vec<MemberId> = mitglieder
            .iter()
            .filter(|m| m.id != ich && m.raum == Some(raum) && !m.kennung.is_empty())
            .map(|m| m.id)
            .collect();

        // Ich habe den Schlüssel dieser Epoche schon.
        if let Some(vorhanden) = self.hole(raum) {
            if vorhanden.epoche == epoche {
                if !Self::bin_wart(ich, mitglieder, raum) {
                    return Vec::new();
                }
                // Als Wart: nur dann erneut verteilen, wenn sich die Runde
                // geändert hat (jemand kam, ging oder kam wieder). Sonst käme
                // bei jedem Aufruf dasselbe Paket noch einmal.
                let schon = self
                    .verteilt
                    .lock()
                    .ok()
                    .and_then(|v| v.get(&raum).cloned());
                if let Some((stand_epoche, stand)) = schon {
                    if stand_epoche == epoche && stand == anwesend {
                        return Vec::new();
                    }
                }
                let pakete = self.verpacke(raum, epoche, ich, eigener, mitglieder, &vorhanden);
                self.merke_verteilt(raum, epoche, anwesend);
                return pakete;
            }
        }
        if !Self::bin_wart(ich, mitglieder, raum) {
            self.setze_notiz(Some(
                "Verschlüsselter Raum: warte auf den Schlüssel des Raums".to_string(),
            ));
            // Vielleicht ging das Paket verloren — dann selbst nachfragen.
            // Nicht bei jedem Aufruf (das wäre Funklärm), sondern im Abstand.
            let fällig = {
                let Ok(mut angefragt) = self.angefragt.lock() else {
                    return Vec::new();
                };
                match angefragt.get(&raum) {
                    Some(wann) if wann.elapsed() < ANFRAGE_ABSTAND => false,
                    _ => {
                        angefragt.insert(raum, std::time::Instant::now());
                        true
                    }
                }
            };
            return if fällig {
                vec![ClientFrame::SchluesselAnfrage { raum }]
            } else {
                Vec::new()
            };
        }

        // Ich bin der Wart: Schlüssel erzeugen und an alle anderen verteilen.
        let schluessel = Raumschluessel::neu(epoche);
        let pakete = self.verpacke(raum, epoche, ich, eigener, mitglieder, &schluessel);
        self.merke(raum, schluessel);
        self.merke_verteilt(raum, epoche, anwesend);
        self.setze_notiz(None);
        pakete
    }

    fn merke_verteilt(&self, raum: RoomId, epoche: u32, anwesend: Vec<MemberId>) {
        if let Ok(mut verteilt) = self.verteilt.lock() {
            verteilt.insert(raum, (epoche, anwesend));
        }
    }

    /// Verpackt den Raumschlüssel für **alle** anderen Anwesenden.
    fn verpacke(
        &self,
        raum: RoomId,
        epoche: u32,
        ich: MemberId,
        eigener: &Schluessel,
        mitglieder: &[Mitgliedsansicht],
        schluessel: &Raumschluessel,
    ) -> Vec<ClientFrame> {
        let mut pakete = Vec::new();
        let geheimnis = eigener.austausch_geheimnis();
        for m in mitglieder {
            if m.id == ich || m.raum != Some(raum) || m.kennung.is_empty() {
                continue;
            }
            let Some(oeffentlich) = oeffentlich_aus_hex(&m.kennung) else {
                continue;
            };
            let Ok(umschlag) =
                Umschlag::verpacken(&geheimnis, &oeffentlich, raum.text().as_str(), &schluessel)
            else {
                continue;
            };
            let unterschrift =
                eigener.unterschreibe(&zu_signieren(raum.text().as_str(), epoche, &umschlag.bytes));
            pakete.push(ClientFrame::Schluesselpaket {
                raum,
                epoche,
                fuer: m.id,
                daten: hex::encode(&umschlag.bytes),
                unterschrift,
            });
        }
        pakete
    }

    /// Vergisst, wem schon verteilt wurde — die nächste Runde beginnt von vorn.
    ///
    /// Gerufen, wenn jemand um den Schlüssel bittet (sein Paket ging verloren).
    pub fn verteilung_erneuern(&self, raum: RoomId) {
        if let Ok(mut verteilt) = self.verteilt.lock() {
            verteilt.remove(&raum);
        }
    }

    /// Nimmt ein Schlüsselpaket an: Unterschrift prüfen, öffnen, merken.
    pub fn nimm_paket(
        &self,
        paket: Paket<'_>,
        eigener: &Schluessel,
        mitglieder: &[Mitgliedsansicht],
    ) -> Result<(), String> {
        let Paket {
            raum,
            epoche,
            von,
            daten,
            unterschrift,
        } = paket;
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

/// Ein Schlüsselpaket, wie der Server es weiterreicht.
///
/// Ein Wert statt fünf einzelner Angaben: `nimm_paket` bleibt damit lesbar,
/// und ein Aufrufer kann Raum, Epoche und Absender nicht vertauschen.
pub struct Paket<'a> {
    /// Raum, für den der Schlüssel gilt.
    pub raum: RoomId,
    /// Epoche des Schlüssels.
    pub epoche: u32,
    /// Wer den Schlüssel verschickt hat.
    pub von: MemberId,
    /// Der Umschlag (Hex).
    pub daten: &'a str,
    /// Unterschrift des Absenders über den Umschlag (Hex).
    pub unterschrift: &'a str,
}

/// Hilfsfunktion für den Verbindungsthread: aus einem Rahmen die Aufgabe
/// ableiten, ohne dass dort Krypto steht.
pub fn aus_rahmen(rahmen: &ServerFrame) -> Option<Paket<'_>> {
    match rahmen {
        ServerFrame::Schluesselpaket {
            raum,
            epoche,
            von,
            daten,
            unterschrift,
        } => Some(Paket {
            raum: *raum,
            epoche: *epoche,
            von: *von,
            daten,
            unterschrift,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_domain::Role;
    use sprechwerk_protocol::Raumansicht;

    fn mitglied(id: u128, kennung: &str, raum: Option<RoomId>) -> Mitgliedsansicht {
        mitglied_seit(id, kennung, raum, 0)
    }

    /// Wie [`mitglied`], aber mit Anwesenheitszeit — sie entscheidet, wer Wart ist.
    fn mitglied_seit(
        id: u128,
        kennung: &str,
        raum: Option<RoomId>,
        raum_seit: i64,
    ) -> Mitgliedsansicht {
        Mitgliedsansicht {
            id: MemberId::aus_roh(id),
            name: format!("M{id}"),
            kennung: kennung.to_string(),
            rolle: Role::Mitglied,
            raum,
            spricht: false,
            stumm: false,
            taub: false,
            raum_seit,
        }
    }

    /// Der **am längsten Anwesende** ist Wart — nicht der mit der kleinsten
    /// Kennung. Sonst übernähme ein Neuankömmling den Raum mitten in der
    /// Epoche, erzeugte einen zweiten Schlüssel und die Betroffenen hörten
    /// einander nicht mehr.
    #[test]
    fn aeltester_ist_wart_und_bleibt_es() {
        let raum = RoomId::aus_roh(2);
        let alt = Schluessel::neu();
        let neu = Schluessel::neu();
        // Der Neue hat die **kleinere** Kennung und die kleinere Id.
        let mitglieder = vec![
            mitglied_seit(9, &alt.kennung(), Some(raum), 1_000),
            mitglied_seit(1, &neu.kennung(), Some(raum), 5_000),
        ];
        assert!(
            Schluesselverwaltung::bin_wart(MemberId::aus_roh(9), &mitglieder, raum),
            "der Neuankömmling hat den Wart abgelöst"
        );
        assert!(
            !Schluesselverwaltung::bin_wart(MemberId::aus_roh(1), &mitglieder, raum),
            "der Neuankömmling gilt als Wart"
        );

        // Ohne Anwesenheitszeit (ältere Rahmen) entscheidet die Id — dieselbe
        // Wahl in allen Clients, darauf kommt es an.
        let ohne_zeit = vec![
            mitglied(9, &alt.kennung(), Some(raum)),
            mitglied(1, &neu.kennung(), Some(raum)),
        ];
        assert!(
            Schluesselverwaltung::bin_wart(MemberId::aus_roh(1), &ohne_zeit, raum),
            "bei Gleichstand muss die Id entscheiden"
        );
        // Wer keinen Schlüssel hat (leere Kennung), ist nie Wart.
        let ohne_kennung = vec![
            mitglied_seit(1, "", Some(raum), 0),
            mitglied_seit(2, &alt.kennung(), Some(raum), 9_000),
        ];
        assert!(
            !Schluesselverwaltung::bin_wart(MemberId::aus_roh(1), &ohne_kennung, raum),
            "ohne Kennung kann niemand Wart sein"
        );
        assert!(Schluesselverwaltung::bin_wart(
            MemberId::aus_roh(2),
            &ohne_kennung,
            raum
        ));
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

    /// Beim Wechsel gilt der alte Schlüssel noch kurz — aber nicht ewig.
    ///
    /// Das ist die Überlappung aus ADR 0006 §6: der erste Rahmen der neuen
    /// Epoche ist schon unterwegs, während der Empfänger noch umstellt.
    #[test]
    fn uebergang_laesst_den_alten_schluessel_kurz_gelten() {
        let raum = RoomId::aus_roh(21);
        let text = raum.text();
        let alt = Raumschluessel::neu(5);
        let neu = Raumschluessel::neu(6);

        let verwaltung = Schluesselverwaltung::mit_uebergang(std::time::Duration::from_millis(120));
        verwaltung.merke(raum, alt.clone());

        // Ein Rahmen der alten Epoche …
        let kopf = sprechwerk_protocol::Sprachkopf::neu(1);
        let alt_rahmen = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &kopf.schreiben(b"alt"),
            &alt,
            &text,
        )
        .expect("verschlüsseln");
        let neu_rahmen = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &kopf.schreiben(b"neu"),
            &neu,
            &text,
        )
        .expect("verschlüsseln");

        // … wird vor dem Wechsel gelesen.
        let klar = verwaltung.entschluesseln(raum, &alt_rahmen).expect("alt vor dem Wechsel");
        assert_eq!(
            sprechwerk_protocol::Sprachkopf::lesen(&klar).expect("Kopf").1,
            b"alt",
            "der alte Rahmen wurde nicht richtig geöffnet"
        );

        // Wechsel.
        verwaltung.merke(raum, neu.clone());

        // Kurz danach: beide gehen.
        assert!(
            verwaltung.entschluesseln(raum, &alt_rahmen).is_some(),
            "der Rahmen aus dem Wechselfenster ging verloren"
        );
        assert!(
            verwaltung.entschluesseln(raum, &neu_rahmen).is_some(),
            "der neue Rahmen geht nicht"
        );

        // Nach dem Fenster: der alte Schlüssel ist wertlos.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            verwaltung.entschluesseln(raum, &alt_rahmen).is_none(),
            "der alte Schlüssel gilt zu lange"
        );
        assert!(
            verwaltung.entschluesseln(raum, &neu_rahmen).is_some(),
            "der neue Schlüssel muss weiter gelten"
        );
    }

    /// Ein **Späterkommender** muss den Schlüssel bekommen.
    ///
    /// Der Fehler, den der Ende-zu-Ende-Test aufgedeckt hat: der Wart hatte den
    /// Schlüssel der Epoche schon und tat deshalb nichts — wer danach kam, sah
    /// nur „warte auf den Schlüssel des Raums" und hörte nichts. Aufgefallen,
    /// weil jemand nach einem Abbruch in den verschlüsselten Raum zurückkam.
    #[test]
    fn spaeterkommender_bekommt_den_schluessel() {
        let raum = RoomId::aus_roh(11);
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        let ich_a = MemberId::aus_roh(1);
        let ich_b = MemberId::aus_roh(2);
        let ich_c = MemberId::aus_roh(3);
        let c = Schluessel::neu();

        let verwaltung = Schluesselverwaltung::neu();
        let zu_zweit = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
        ];
        let erste = verwaltung.sorge_fuer(raum, 5_000, ich_a, &a, &zu_zweit);
        assert_eq!(erste.len(), 1, "ein Paket für B");
        assert!(verwaltung.hole(raum).is_some());

        // Keine Änderung: nichts erneut schicken.
        assert!(
            verwaltung.sorge_fuer(raum, 5_000, ich_a, &a, &zu_zweit).is_empty(),
            "dieselbe Runde darf nicht erneut Pakete erzeugen"
        );

        // C kommt dazu — er braucht ein Paket.
        let zu_dritt = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
            mitglied(3, &c.kennung(), Some(raum)),
        ];
        let zweite = verwaltung.sorge_fuer(raum, 5_000, ich_a, &a, &zu_dritt);
        assert_eq!(zweite.len(), 2, "B und C müssen den Schlüssel bekommen");
        assert!(
            zweite.iter().any(|p| matches!(p, ClientFrame::Schluesselpaket { fuer, .. } if *fuer == ich_c)),
            "kein Paket für den Späterkommenden"
        );

        // Und C kann ihn öffnen (gleiche Epoche, gültige Unterschrift).
        let verwaltung_c = Schluesselverwaltung::neu();
        for paket in &zweite {
            let ClientFrame::Schluesselpaket {
                raum: r,
                epoche,
                fuer,
                daten,
                unterschrift,
            } = paket
            else {
                continue;
            };
            if *fuer != ich_c {
                continue;
            }
            // `von` ist auf der Leitung der Absender; hier bin ich es selbst.
            let _ = verwaltung_c.nimm_paket(
                Paket {
                    raum: *r,
                    epoche: *epoche,
                    von: ich_a,
                    daten,
                    unterschrift,
                },
                &c,
                &zu_dritt,
            );
        }
        assert_eq!(
            verwaltung_c.hole(raum).map(|s| s.epoche),
            Some(super::epoche_aus(5_000)),
            "der Späterkommende hat keinen Schlüssel"
        );
    }

    /// Wer um den Schlüssel bittet, löst beim Wart eine neue Verteilung aus.
    #[test]
    fn nachforderung_loest_neue_verteilung_aus() {
        let raum = RoomId::aus_roh(31);
        let a = Schluessel::neu();
        let b = Schluessel::neu();
        let ich_a = MemberId::aus_roh(1);
        let mitglieder = vec![
            mitglied(1, &a.kennung(), Some(raum)),
            mitglied(2, &b.kennung(), Some(raum)),
        ];
        let verwaltung = Schluesselverwaltung::neu();
        assert_eq!(
            verwaltung
                .sorge_fuer(raum, 7_000, ich_a, &a, &mitglieder)
                .len(),
            1
        );
        // Alles verteilt: nichts mehr zu tun.
        assert!(verwaltung
            .sorge_fuer(raum, 7_000, ich_a, &a, &mitglieder)
            .is_empty());

        // Jemand hat nichts bekommen und fragt nach.
        verwaltung.verteilung_erneuern(raum);
        assert_eq!(
            verwaltung
                .sorge_fuer(raum, 7_000, ich_a, &a, &mitglieder)
                .len(),
            1,
            "nach der Nachforderung muss erneut verteilt werden"
        );
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
                    .nimm_paket(
                        Paket {
                            raum: *r,
                            epoche: *epoche,
                            von: ich_a,
                            daten,
                            unterschrift,
                        },
                        &b,
                        &mitglieder,
                    )
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
        let ClientFrame::Schluesselpaket { daten, epoche, .. } = &pakete[0] else {
            panic!("kein Paket");
        };
        // Unterschrift von jemand anderem
        let roh = hex::decode(daten).unwrap();
        let falsch = boese.unterschreibe(&zu_signieren("raum_3", *epoche, &roh));
        let ziel = Schluesselverwaltung::neu();
        assert!(ziel
            .nimm_paket(
                Paket {
                    raum,
                    epoche: *epoche,
                    von: MemberId::aus_roh(1),
                    daten,
                    unterschrift: &falsch,
                },
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
        // B ist nicht der Wart: keine Schlüsselpakete, aber eine Notiz — und
        // **eine Nachforderung**, damit ein verlorenes Paket nicht ewig
        // unbemerkt bleibt.
        let verwaltung = Schluesselverwaltung::neu();
        let pakete = verwaltung.sorge_fuer(raum, 1_000, MemberId::aus_roh(2), &b, &mitglieder);
        assert_eq!(pakete.len(), 1, "der Wartende muss nachfragen");
        assert!(
            matches!(pakete[0], ClientFrame::SchluesselAnfrage { raum: r } if r == raum),
            "keine Nachforderung: {:?}",
            pakete[0]
        );
        assert!(verwaltung.hole(raum).is_none());
        assert!(verwaltung.notiz().is_some());

        // Aber nicht bei jedem Aufruf — sonst wäre es Funklärm.
        assert!(
            verwaltung
                .sorge_fuer(raum, 1_000, MemberId::aus_roh(2), &b, &mitglieder)
                .is_empty(),
            "die Nachforderung muss im Abstand kommen, nicht bei jedem Aufruf"
        );
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
    // -----------------------------------------------------------------
    // Ende zu Ende: zwei bis drei echte Endpunkte über den echten Server
    // -----------------------------------------------------------------

    /// Nur **ein** Prüfserver zur Zeit.
    ///
    /// Ohne diese Sperre liefen zwei Ende-zu-Ende-Tests gleichzeitig, zögen sich
    /// im schlimmsten Fall denselben freien Port und redeten mit dem Server des
    /// anderen — ein Fehlschlag, der nach einem Krypto-Fehler aussieht und
    /// keiner ist.
    static E2E: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Ein laufender Serverprozess für die Prüfung unten.
    struct Pruefserver {
        kind: std::process::Child,
        adresse: std::net::SocketAddr,
        verzeichnis: std::path::PathBuf,
        /// Hält die Sperre, solange der Server lebt.
        _sperre: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Pruefserver {
        fn drop(&mut self) {
            let _ = self.kind.kill();
            let _ = self.kind.wait();
            let _ = std::fs::remove_dir_all(&self.verzeichnis);
        }
    }

    fn freier_port() -> u16 {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").expect("Port");
        s.local_addr().expect("Adresse").port()
    }

    /// Startet den **echten** Server aus dem Arbeitsbereich.
    ///
    /// `crates/desktop` liegt außerhalb des Arbeitsbereichs (Tauri hat eine
    /// eigene Sperrdatei), deshalb wird das Programm über seinen Pfad gesucht.
    /// Fehlt es, sagt der Test, was zu tun ist — statt still zu überspringen.
    fn server_starten(name: &str) -> Pruefserver {
        let sperre = E2E.lock().unwrap_or_else(|g| g.into_inner());
        let programm = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/sprechwerk");
        assert!(
            programm.exists(),
            "{} fehlt — vorher `cargo build -p sprechwerk-server` im Arbeitsbereich ausführen",
            programm.display()
        );
        let verzeichnis = std::env::temp_dir().join(format!(
            "sprechwerk-e2e-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&verzeichnis);
        std::fs::create_dir_all(&verzeichnis).expect("Verzeichnis");
        let port = freier_port();
        let mut kind = std::process::Command::new(&programm)
            .env("SPRECHWERK_DATEN", &verzeichnis)
            .env("SPRECHWERK_PORT", port.to_string())
            .env("SPRECHWERK_BIND", "127.0.0.1")
            .env("SPRECHWERK_NAME", "E2E")
            .env("SPRECHWERK_SICHERUNG_AKTIV", "0")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Server startet");

        use std::io::BufRead;
        let stdout = kind.stdout.take().expect("stdout");
        let mut leser = std::io::BufReader::new(stdout);
        let mut bereit = false;
        for _ in 0..200 {
            let mut zeile = String::new();
            if leser.read_line(&mut zeile).unwrap_or(0) == 0 {
                break;
            }
            if zeile.contains("lauscht auf") {
                bereit = true;
                break;
            }
        }
        assert!(bereit, "Server meldet sich nicht");
        std::thread::spawn(move || {
            let mut rest = String::new();
            let _ = std::io::Read::read_to_string(&mut leser, &mut rest);
        });

        Pruefserver {
            kind,
            adresse: format!("127.0.0.1:{port}").parse().expect("Adresse"),
            verzeichnis,
            _sperre: sperre,
        }
    }

    /// Ein Teilnehmer im Test: Verbindung, Signaturschlüssel, Schlüsselverwaltung.
    struct Teilnehmer {
        verbindung: sprechwerk_adapters::Verbindung,
        schluessel: Schluessel,
        verwaltung: Schluesselverwaltung,
        name: &'static str,
        /// Wie viele Schlüsselpakete absichtlich verloren gehen sollen
        /// (Prüfung der Nachforderung).
        verlieren: std::cell::Cell<usize>,
        verloren: std::cell::Cell<usize>,
        /// Wie oft nach dem Schlüssel gefragt wurde (erzeugt).
        anfragen: std::cell::Cell<usize>,
        /// Wie viele Bitten absichtlich verloren gehen sollen.
        bitten_verlieren: std::cell::Cell<usize>,
        /// Wie viele Bitten tatsächlich verloren gingen.
        bitten_verloren: std::cell::Cell<usize>,
    }

    impl Teilnehmer {
        async fn neu(adresse: std::net::SocketAddr, name: &'static str) -> Self {
            Self::mit(adresse, name, Schluessel::neu()).await
        }

        /// Mit **vorhandener** Kennung — so kommt jemand nach einem Abbruch
        /// zurück.
        async fn mit(
            adresse: std::net::SocketAddr,
            name: &'static str,
            schluessel: Schluessel,
        ) -> Self {
            let verbindung = sprechwerk_adapters::Verbindung::oeffnen(
                adresse, &schluessel, name, None,
            )
            .await
            .expect("Verbindung");
            Self {
                verbindung,
                schluessel,
                verwaltung: Schluesselverwaltung::neu(),
                name,
                verlieren: std::cell::Cell::new(0),
                verloren: std::cell::Cell::new(0),
                anfragen: std::cell::Cell::new(0),
                bitten_verlieren: std::cell::Cell::new(0),
                bitten_verloren: std::cell::Cell::new(0),
            }
        }

        fn raum(&self) -> Option<RoomId> {
            self.verbindung.raum
        }

        fn seit(&self, raum: RoomId) -> i64 {
            self.verbindung
                .raeume
                .iter()
                .find(|r| r.id == raum)
                .map(|r| r.verschluesselt_seit)
                .unwrap_or(0)
        }

        /// Was der Client nach jedem Rahmen tut: Schlüssel erzeugen und verteilen.
        async fn sorge(&mut self, raum: RoomId) {
            let seit = self.seit(raum);
            let pakete = self.verwaltung.sorge_fuer(
                raum,
                seit,
                self.verbindung.ich,
                &self.schluessel,
                &self.verbindung.mitglieder,
            );
            for paket in pakete {
                if matches!(paket, ClientFrame::SchluesselAnfrage { .. }) {
                    self.anfragen.set(self.anfragen.get() + 1);
                    if self.bitten_verlieren.get() > 0 {
                        // Auch die Bitte kann im Netz verloren gehen.
                        self.bitten_verlieren.set(self.bitten_verlieren.get() - 1);
                        self.bitten_verloren.set(self.bitten_verloren.get() + 1);
                        continue;
                    }
                }
                let _ = self.verbindung.sende(paket).await;
            }
        }

        /// Reagiert auf Rahmen der Schlüsselverwaltung.
        ///
        /// Das ist dieselbe Arbeit, die `anwendung.rs` im Betrieb macht: Paket
        /// annehmen — und bei einer **Nachforderung** die Verteilung erneuern.
        fn nimm(&self, rahmen: &ServerFrame) {
            if let ServerFrame::SchluesselAnfrage { raum, .. } = rahmen {
                self.verwaltung.verteilung_erneuern(*raum);
                return;
            }
            if let ServerFrame::Schluesselpaket {
                raum,
                epoche,
                von,
                daten,
                unterschrift,
            } = rahmen
            {
                let paket = Paket {
                    raum: *raum,
                    epoche: *epoche,
                    von: *von,
                    daten,
                    unterschrift,
                };
                if self.verlieren.get() > 0 {
                    // Netzverlust nachstellen: das Paket kommt nie an.
                    self.verlieren.set(self.verlieren.get() - 1);
                    self.verloren.set(self.verloren.get() + 1);
                    return;
                }
                if let Err(fehler) =
                    self.verwaltung
                        .nimm_paket(paket, &self.schluessel, &self.verbindung.mitglieder)
                {
                    eprintln!("[{}] Schlüsselpaket abgelehnt: {fehler}", self.name);
                }
            }
        }

        fn epoche(&self, raum: RoomId) -> Option<u32> {
            self.verwaltung.hole(raum).map(|s| s.epoche)
        }
    }

    /// Verteilt Schlüssel, bis alle Teilnehmer die erwartete Epoche haben.
    ///
    /// Gibt zurück, wie lange das gedauert hat — das ist das **Wechselfenster**,
    /// in dem Sprache nicht entschlüsselbar ist.
    async fn bis_schluessel_stehen(
        teilnehmer: &mut [&mut Teilnehmer],
        raum: RoomId,
        erwartet: u32,
    ) -> std::time::Duration {
        let beginn = std::time::Instant::now();
        for _ in 0..200 {
            let mut alle = true;
            for t in teilnehmer.iter_mut() {
                t.sorge(raum).await;
                if t.epoche(raum) != Some(erwartet) {
                    alle = false;
                }
            }
            if alle {
                return beginn.elapsed();
            }
            // Rahmen abholen und Schlüsselpakete annehmen.
            for t in teilnehmer.iter_mut() {
                if let Ok(Ok(Some(rahmen))) = tokio::time::timeout(
                    std::time::Duration::from_millis(20),
                    t.verbindung.empfang(),
                )
                .await
                {
                    t.nimm(&rahmen);
                }
            }
        }
        panic!("Schlüssel der Epoche {erwartet} kamen nicht zustande: {:?}", teilnehmer
            .iter()
            .map(|t| (t.name, t.epoche(raum)))
            .collect::<Vec<_>>());
    }

    /// Wartet, bis die Raumliste eines Teilnehmers eine Bedingung erfüllt.
    async fn bis_raumliste<F>(t: &mut Teilnehmer, passt: F)
    where
        F: Fn(&Raumansicht) -> bool,
    {
        for _ in 0..200 {
            let treffer = t
                .verbindung
                .raeume
                .iter()
                .find(|r| passt(r))
                .cloned();
            if treffer.is_some() {
                return;
            }
            match tokio::time::timeout(
                std::time::Duration::from_millis(50),
                t.verbindung.empfang(),
            )
            .await
            {
                Ok(Ok(Some(rahmen))) => t.nimm(&rahmen),
                Ok(Ok(None)) => panic!("Verbindung wurde geschlossen"),
                Ok(Err(fehler)) => panic!("Fehler: {fehler}"),
                Err(_) => continue,
            }
        }
        panic!(
            "[{}] Raumliste kam nicht wie erwartet: {:?}",
            t.name,
            t.verbindung
                .raeume
                .iter()
                .map(|r| (r.name.clone(), r.verschluesselt, r.anwesend, r.verschluesselt_seit))
                .collect::<Vec<_>>()
        );
    }

    /// Ein **verlorenes** Schlüsselpaket blockiert niemanden dauerhaft: der
    /// Wartende fragt nach, der Wart verteilt erneut — über den echten Server.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn verlorenes_schluesselpaket_wird_nachgefordert() {
        let server = server_starten("nachforderung");
        let adresse = server.adresse;

        let mut a = Teilnehmer::neu(adresse, "Ada").await;
        let mut b = Teilnehmer::neu(adresse, "Bosse").await;

        a.verbindung
            .sende(ClientFrame::RaumAnlegen {
                name: "Geheim".into(),
            })
            .await
            .expect("Raum anlegen");
        let raum = loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                a.verbindung.empfang(),
            )
            .await
            {
                Ok(Ok(Some(ServerFrame::Raeume { raeume }))) => {
                    if let Some(r) = raeume.iter().find(|r| r.name == "Geheim") {
                        break r.id;
                    }
                }
                Ok(Ok(Some(_))) => continue,
                Ok(Ok(None)) => panic!("Verbindung geschlossen"),
                Ok(Err(fehler)) => panic!("Fehler: {fehler}"),
                Err(_) => continue,
            }
        };
        for t in [&mut a, &mut b] {
            t.verbindung
                .sende(ClientFrame::Betreten { raum })
                .await
                .expect("betreten");
        }
        for t in [&mut a, &mut b] {
            bis_raumliste(t, |r| r.id == raum && r.anwesend == 2).await;
        }
        a.verbindung
            .sende(ClientFrame::RaumVerschluesseln { raum, an: true })
            .await
            .expect("verschlüsseln");
        for t in [&mut a, &mut b] {
            bis_raumliste(t, |r| r.id == raum && r.verschluesselt).await;
        }
        let epoche = super::epoche_aus(a.seit(raum));
        bis_schluessel_stehen(&mut [&mut a, &mut b], raum, epoche).await;

        // Wen trifft der Verlust? Wer **nicht** der Wart ist — der Wart
        // erzeugt den Schlüssel selbst und bräuchte kein Paket.
        let ist_wart = |t: &Teilnehmer| {
            Schluesselverwaltung::bin_wart(t.verbindung.ich, &t.verbindung.mitglieder, raum)
        };
        let (wart, verlierer) = if ist_wart(&a) { ("Ada", 1) } else { ("Bosse", 0) };

        // Zähler zurücksetzen: geprüft wird nur, was **nach** dem Verlust
        // passiert.
        a.anfragen.set(0);
        b.anfragen.set(0);

        // Dem Verlierer den Schlüssel wegnehmen (wie nach einem Neustart) und
        // dafür sorgen, dass das **erste** Paket verloren geht.
        if verlierer == 0 {
            a.verwaltung.vergiss(raum);
            a.verlieren.set(1);
        } else {
            b.verwaltung.vergiss(raum);
            b.verlieren.set(1);
        }

        // Ohne Nachforderung würde er für immer warten. Mit ihr: Schlüssel da.
        let beginn = std::time::Instant::now();
        bis_schluessel_stehen(&mut [&mut a, &mut b], raum, epoche).await;
        let _gedauert = beginn.elapsed();

        let (verloren, anfragen, epoche_danach) = if verlierer == 0 {
            (a.verloren.get(), a.anfragen.get(), a.epoche(raum))
        } else {
            (b.verloren.get(), b.anfragen.get(), b.epoche(raum))
        };
        assert_eq!(
            verloren, 1,
            "das Paket ging nicht wirklich verloren — der Test prüft nichts (Wart: {wart})"
        );
        assert!(
            anfragen >= 1,
            "es wurde gar nicht nachgefordert — der Verlierer hätte für immer gewartet"
        );
        // Der Abstand zwischen zwei Nachforderungen (und damit das Wiederholen,
        // wenn auch die Bitte verloren geht) steht in `ohne_wart_wird_gewartet`;
        // hier geht es um den Weg über den Server: Bitte raus, Schlüssel zurück.
        assert_eq!(
            epoche_danach,
            Some(epoche),
            "der Wartende hat den Schlüssel am Ende doch nicht"
        );
    }

    /// Auch die **Bitte** kann verloren gehen: dann muss sie sich nach dem
    /// Abstand wiederholen — sonst hängt der Wartende wieder für immer.
    ///
    /// Der Abstand (`ANFRAGE_ABSTAND`, 1 s) ist die Obergrenze, um die sich ein
    /// Verlust höchstens verzögert. Geprüft wird genau dieser eine Schritt:
    /// erste Bitte weg, Schlüsselpaket weg, zweite Bitte kommt an.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn verlorene_bitte_wiederholt_sich() {
        let server = server_starten("bitte");
        let adresse = server.adresse;

        let mut a = Teilnehmer::neu(adresse, "Ada").await;
        let mut b = Teilnehmer::neu(adresse, "Bosse").await;

        a.verbindung
            .sende(ClientFrame::RaumAnlegen {
                name: "Geheim".into(),
            })
            .await
            .expect("Raum anlegen");
        let raum = loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                a.verbindung.empfang(),
            )
            .await
            {
                Ok(Ok(Some(ServerFrame::Raeume { raeume }))) => {
                    if let Some(r) = raeume.iter().find(|r| r.name == "Geheim") {
                        break r.id;
                    }
                }
                Ok(Ok(Some(_))) => continue,
                Ok(Ok(None)) => panic!("Verbindung geschlossen"),
                Ok(Err(fehler)) => panic!("Fehler: {fehler}"),
                Err(_) => continue,
            }
        };
        for t in [&mut a, &mut b] {
            t.verbindung
                .sende(ClientFrame::Betreten { raum })
                .await
                .expect("betreten");
        }
        for t in [&mut a, &mut b] {
            bis_raumliste(t, |r| r.id == raum && r.anwesend == 2).await;
        }
        a.verbindung
            .sende(ClientFrame::RaumVerschluesseln { raum, an: true })
            .await
            .expect("verschlüsseln");
        for t in [&mut a, &mut b] {
            bis_raumliste(t, |r| r.id == raum && r.verschluesselt).await;
        }
        let epoche = super::epoche_aus(a.seit(raum));
        bis_schluessel_stehen(&mut [&mut a, &mut b], raum, epoche).await;

        // Der Verlierer ist wieder, wer nicht der Wart ist.
        let ist_wart = |t: &Teilnehmer| {
            Schluesselverwaltung::bin_wart(t.verbindung.ich, &t.verbindung.mitglieder, raum)
        };
        let verlierer_ist_a = !ist_wart(&a);
        if verlierer_ist_a {
            a.verwaltung.vergiss(raum);
            a.anfragen.set(0);
            a.verlieren.set(1); // Schlüsselpaket weg
            a.bitten_verlieren.set(1); // und die erste Bitte auch
        } else {
            b.verwaltung.vergiss(raum);
            b.anfragen.set(0);
            b.verlieren.set(1);
            b.bitten_verlieren.set(1);
        }

        let beginn = std::time::Instant::now();
        bis_schluessel_stehen(&mut [&mut a, &mut b], raum, epoche).await;
        let gedauert = beginn.elapsed();

        let (anfragen, bitten_verloren, pakete_verloren, epoche_danach) = if verlierer_ist_a {
            (
                a.anfragen.get(),
                a.bitten_verloren.get(),
                a.verloren.get(),
                a.epoche(raum),
            )
        } else {
            (
                b.anfragen.get(),
                b.bitten_verloren.get(),
                b.verloren.get(),
                b.epoche(raum),
            )
        };

        assert_eq!(pakete_verloren, 1, "das Schlüsselpaket ging nicht verloren");
        assert_eq!(bitten_verloren, 1, "die erste Bitte ging nicht verloren");
        assert!(
            anfragen >= 2,
            "es wurde nur {anfragen}× gefragt — nach dem Verlust der Bitte muss \
             erneut gefragt werden"
        );
        assert!(
            gedauert >= super::ANFRAGE_ABSTAND,
            "die Wiederholung kam nach {gedauert:?}, der Abstand ist {:?}",
            super::ANFRAGE_ABSTAND
        );
        assert_eq!(
            epoche_danach,
            Some(epoche),
            "der Wartende hat den Schlüssel am Ende doch nicht"
        );
    }

    /// Der ganze Weg: echter Server, drei echte Clients, Verschlüsselung an,
    /// einer geht — neuer Schlüssel —, er kommt zurück.
    ///
    /// Gemessen wird das **Wechselfenster**: die Zeit vom Verlassen bis zum
    /// neuen Schlüssel bei den Verbliebenen. In dieser Zeit ist Sprache nicht
    /// entschlüsselbar; bei 20 ms je Rahmen entspricht sie der Zahl der
    /// verlorenen Wörter (ADR 0006 §6).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn schluesselwechsel_im_laufenden_gespraech() {
        let server = server_starten("wechsel");
        let adresse = server.adresse;

        let mut a = Teilnehmer::neu(adresse, "Ada").await;
        let mut b = Teilnehmer::neu(adresse, "Bosse").await;
        let mut c = Teilnehmer::neu(adresse, "Clemens").await;

        // Ada ist die erste Verbindung und damit Administratorin.
        a.verbindung
            .sende(ClientFrame::RaumAnlegen {
                name: "Geheim".into(),
            })
            .await
            .expect("Raum anlegen");
        let raum = loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                a.verbindung.empfang(),
            )
            .await
            {
                Ok(Ok(Some(ServerFrame::Raeume { raeume }))) => {
                    if let Some(r) = raeume.iter().find(|r| r.name == "Geheim") {
                        break r.id;
                    }
                }
                Ok(Ok(Some(_))) => continue,
                Ok(Ok(None)) => panic!("Verbindung geschlossen"),
                Ok(Err(fehler)) => panic!("Fehler: {fehler}"),
                Err(_) => continue,
            }
        };

        for t in [&mut a, &mut b, &mut c] {
            t.verbindung
                .sende(ClientFrame::Betreten { raum })
                .await
                .expect("betreten");
        }
        for t in [&mut a, &mut b, &mut c] {
            bis_raumliste(t, |r| r.id == raum && r.anwesend == 3).await;
        }

        // Verschlüsselung an.
        a.verbindung
            .sende(ClientFrame::RaumVerschluesseln { raum, an: true })
            .await
            .expect("verschlüsseln");
        for t in [&mut a, &mut b, &mut c] {
            bis_raumliste(t, |r| r.id == raum && r.verschluesselt).await;
        }

        let seit = a.seit(raum);
        let epoche = super::epoche_aus(seit);
        bis_schluessel_stehen(&mut [&mut a, &mut b, &mut c], raum, epoche).await;

        // Ein verschlüsselter Rahmen kommt an — mit dem Schlüssel der Epoche.
        let raum_text = raum.text();
        let klar = b"hallo ende zu ende";
        let kopf = sprechwerk_protocol::Sprachkopf::neu(1);
        let datagramm = kopf.schreiben(klar);
        let geheim = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &datagramm,
            &a.verwaltung.hole(raum).expect("Schlüssel A"),
            &raum_text,
        )
        .expect("verschlüsseln");
        a.verbindung
            .sprache_senden(bytes::Bytes::from(geheim))
            .expect("senden");
        let empfangen = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            b.verbindung.sprache_empfangen(),
        )
        .await
        .expect("B bekommt den Rahmen")
        .expect("kein Fehler");
        let offen = sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(
            &empfangen,
            &b.verwaltung.hole(raum).expect("Schlüssel B"),
            &raum_text,
        )
        .expect("B entschlüsselt");
        let (_, nutzlast) = sprechwerk_protocol::Sprachkopf::lesen(&offen).expect("Kopf");
        assert_eq!(nutzlast, klar, "Klartext kam nicht heil an");

        // Den Schlüssel der alten Epoche behalten — damit lässt sich nach dem
        // Wechsel prüfen, ob das Übergangsfenster greift.
        let alter_schluessel = a.verwaltung.hole(raum).expect("Schlüssel der ersten Epoche");

        // C geht — die Epoche muss sich drehen. Seine Kennung behalten wir für
        // die Rückkehr.
        let c_schluessel = c.schluessel.clone();
        c.verbindung
            .sende(ClientFrame::Verlassen)
            .await
            .expect("verlassen");
        let beginn = std::time::Instant::now();
        bis_raumliste(&mut a, |r| r.id == raum && r.verschluesselt_seit > seit).await;
        let neue_seit = a.seit(raum);
        let neue_epoche = super::epoche_aus(neue_seit);
        assert_ne!(neue_epoche, epoche, "Epoche blieb gleich");

        // Neuen Schlüssel aushandeln (A und B sind noch da).
        let fenster =
            bis_schluessel_stehen(&mut [&mut a, &mut b], raum, neue_epoche).await;
        let gemessen = beginn.elapsed();

        // Der alte Schlüssel ist wertlos: er öffnet den neuen Rahmen nicht.
        let neuer_rahmen = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &sprechwerk_protocol::Sprachkopf::neu(2).schreiben(klar),
            &a.verwaltung.hole(raum).expect("neuer Schlüssel A"),
            &raum_text,
        )
        .expect("verschlüsseln");
        let alt = super::Raumschluessel::neu(epoche);
        assert!(
            sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(
                &neuer_rahmen,
                &alt,
                &raum_text
            )
            .is_err(),
            "ein fremder Schlüssel öffnet den Rahmen"
        );

        // **Übergangsfenster:** ein Rahmen, der noch mit dem alten Schlüssel
        // verschlüsselt wurde (er war beim Wechsel schon unterwegs), muss
        // ankommen — sonst gingen beim Wechsel Wörter verloren.
        let alter_rahmen = sprechwerk_adapters::verschluesselt::rahmen_verschluesseln(
            &sprechwerk_protocol::Sprachkopf::neu(3).schreiben(b"noch alt"),
            &alter_schluessel,
            &raum_text,
        )
        .expect("verschlüsseln");
        a.verbindung
            .sprache_senden(bytes::Bytes::from(alter_rahmen))
            .expect("senden");
        let empfangen = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            b.verbindung.sprache_empfangen(),
        )
        .await
        .expect("B bekommt auch den alten Rahmen")
        .expect("kein Fehler");
        let offen = b
            .verwaltung
            .entschluesseln(raum, &empfangen)
            .expect("B muss den Rahmen aus dem Wechselfenster noch öffnen können");
        assert_eq!(
            sprechwerk_protocol::Sprachkopf::lesen(&offen).expect("Kopf").1,
            b"noch alt",
            "der Rahmen aus dem Wechselfenster kam nicht heil an"
        );

        // Und mit dem neuen Schlüssel kommt Sprache wieder an.
        a.verbindung
            .sprache_senden(bytes::Bytes::from(neuer_rahmen))
            .expect("senden");
        let empfangen = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            b.verbindung.sprache_empfangen(),
        )
        .await
        .expect("B bekommt den neuen Rahmen")
        .expect("kein Fehler");
        let offen = sprechwerk_adapters::verschluesselt::rahmen_entschluesseln(
            &empfangen,
            &b.verwaltung.hole(raum).expect("neuer Schlüssel B"),
            &raum_text,
        )
        .expect("B entschlüsselt mit dem neuen Schlüssel");
        assert_eq!(
            sprechwerk_protocol::Sprachkopf::lesen(&offen).expect("Kopf").1,
            klar
        );

        // C kommt mit **derselben Kennung** zurück (nach einem Abbruch) und
        // bekommt den neuen Schlüssel.
        let mut c = Teilnehmer::mit(adresse, "Clemens", c_schluessel).await;
        c.verbindung
            .sende(ClientFrame::Betreten { raum })
            .await
            .expect("wieder betreten");
        bis_raumliste(&mut c, |r| r.id == raum && r.verschluesselt).await;
        bis_schluessel_stehen(&mut [&mut a, &mut b, &mut c], raum, neue_epoche).await;

        println!(
            "Schlüsselwechsel: Epoche {epoche} → {neue_epoche}, Wechselfenster {:.0} ms \
             (≈ {:.0} Rahmen à 20 ms), Verteilung {} ms",
            gemessen.as_secs_f64() * 1000.0,
            gemessen.as_secs_f64() * 1000.0 / 20.0,
            fenster.as_secs_f64() * 1000.0
        );
    }

}

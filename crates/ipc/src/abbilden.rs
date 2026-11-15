//! Rahmen des Servers in den Modellstand übersetzen.
//!
//! Diese Übersetzung ist **Logik**, nicht Darstellung — deshalb liegt sie hier
//! und nicht im Fenster (AGENTS.md §2.4). Sie ist rein: gleicher Rahmen plus
//! gleicher Stand ergibt denselben neuen Stand, ohne Netz, ohne Uhr, ohne
//! Seitenwirkung. Damit ist sie ohne Sockets prüfbar.

use crate::{Mitglied, Nachricht, Raum, Verbindung, Zustand};
use sprechwerk_domain::role::darf;
use sprechwerk_domain::{Capability, Message, Role, RoomId};
use sprechwerk_protocol::{Mitgliedsansicht, Raumansicht, ServerFrame};

/// Die Angaben der Begrüßung, aus denen der Anfangszustand entsteht.
///
/// Ein Wert statt neun Stellungen: die Felder der Begrüßung sind zum Teil
/// gleichartig (`String`, `Vec`, `Option`), und ein vertauschtes Argument wäre
/// hier erst im Fenster zu sehen.
#[derive(Debug, Clone)]
pub struct Begruessung {
    /// Adresse des Servers, wie sie im Fenster steht.
    pub server: String,
    /// Anzeigename des Servers.
    pub server_name: String,
    /// Eigene Kennung.
    pub ich: sprechwerk_domain::MemberId,
    /// Eigener Name.
    pub mein_name: String,
    /// Eigene Rolle.
    pub rolle: Role,
    /// Alle Räume des Servers.
    pub raeume: Vec<Raumansicht>,
    /// Alle Mitglieder des Servers.
    pub mitglieder: Vec<Mitgliedsansicht>,
    /// Raum, in dem man landet.
    pub raum: Option<RoomId>,
    /// Verlauf dieses Raums.
    pub verlauf: Vec<Message>,
}

/// Baut den Anfangszustand aus der Begrüßung.
pub fn aus_willkommen(b: Begruessung) -> Zustand {
    let Begruessung {
        server,
        server_name,
        ich,
        mein_name,
        rolle,
        raeume,
        mitglieder,
        raum,
        verlauf,
    } = b;
    let mut z = Zustand::neu();
    // Die eigene Kennung steht in der Mitgliederliste — der Server gibt sie
    // weiter, damit jeder die Schlüssel der anderen kennt.
    let eigene_kennung = mitglieder
        .iter()
        .find(|m| m.id == ich)
        .map(|m| m.kennung.clone())
        .unwrap_or_default();
    z.verbindung = Some(Verbindung::Verbunden {
        kennung: eigene_kennung,
        server,
        server_name: server_name.clone(),
        ich,
        name: mein_name,
        rolle,
    });
    z.raeume = raeume_abbilden(raeume, raum);
    z.mitglieder = mitglieder_abbilden(mitglieder, Some(ich));
    // Man ist immer in einem Raum: der Standardraum mit seinem Verlauf.
    z.aktueller_raum = raum;
    z.nachrichten = verlauf.iter().map(|n| nachricht_abbilden(n, &z)).collect();
    zaehle_anwesende(&mut z);
    z
}

/// Eine Protokollzeile fürs Fenster.
pub fn eintrag_abbilden(e: &sprechwerk_domain::AuditEntry) -> crate::Eintrag {
    crate::Eintrag {
        laufnummer: e.laufnummer,
        zeit: e.zeit,
        handelnder_name: e.handelnder_name.clone(),
        aktion: e.aktion.clone(),
        ziel: e.ziel.clone(),
        vorher: e.vorher.clone(),
        nachher: e.nachher.clone(),
        ergebnis: e.ergebnis.clone(),
        grund: e.grund.clone(),
    }
}

/// Rechnet einen Rahmen in den Stand ein.
pub fn verarbeite(zustand: &mut Zustand, rahmen: ServerFrame) {
    match rahmen {
        ServerFrame::Willkommen {
            server_name,
            ich,
            rolle,
            raeume,
            mitglieder,
            raum,
            verlauf,
            ..
        } => {
            let server = zustand
                .verbindung
                .as_ref()
                .and_then(|v| match v {
                    Verbindung::Verbindet { server: s } => Some(s.clone()),
                    Verbindung::Verbunden { server: s, .. } => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let name = zustand
                .mitglieder
                .iter()
                .find(|m| m.id == ich)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            *zustand = aus_willkommen(Begruessung {
                server,
                server_name,
                ich,
                mein_name: name,
                rolle,
                raeume,
                mitglieder,
                raum,
                verlauf,
            });
        }

        ServerFrame::Betreten {
            raum,
            verlauf,
            anwesend,
        } => {
            zustand.aktueller_raum = Some(raum);
            zustand.nachrichten = verlauf
                .iter()
                .map(|n| nachricht_abbilden(n, zustand))
                .collect();
            let mut anwesend = anwesend;
            for m in anwesend.iter_mut() {
                // Wer im Raum sitzt, hat den Raum — notfalls aus dem Rahmen.
                if m.raum.is_none() {
                    m.raum = Some(raum);
                }
            }
            zustand.mitglieder = mitglieder_abbilden(anwesend, zustand.ich());
            raeume_hier_setzen(zustand, Some(raum));
            zaehle_anwesende(zustand);
        }

        ServerFrame::Anwesenheit {
            raum,
            mut mitglied,
            anwesend,
        } => {
            // Der Raum der Meldung gilt. Fehlt er im Mitglied, wird er ergänzt —
            // sonst zählt die Anwesenheit im Modell nicht mit (live gesehen:
            // Raum zeigte „0", während jemand darin saß).
            if mitglied.raum.is_none() {
                mitglied.raum = Some(raum);
            }
            let ich = zustand.ich();
            if anwesend {
                let neu = mitglied_abbilden(mitglied, ich);
                match zustand.mitglieder.iter_mut().find(|m| m.id == neu.id) {
                    Some(vorhanden) => *vorhanden = neu,
                    None => zustand.mitglieder.push(neu),
                }
            } else {
                zustand.mitglieder.retain(|m| m.id != mitglied.id);
            }
            zaehle_anwesende(zustand);
        }

        ServerFrame::Text { nachricht } => {
            if Some(nachricht.raum) == zustand.aktueller_raum {
                let neu = nachricht_abbilden(&nachricht, zustand);
                zustand.nachrichten.push(neu);
            }
        }

        ServerFrame::NachrichtGeloescht { nachricht, hart } => {
            if hart {
                zustand.nachrichten.retain(|n| n.id != nachricht);
            } else if let Some(n) = zustand.nachrichten.iter_mut().find(|n| n.id == nachricht) {
                n.geloescht = true;
                n.text = String::new();
            }
        }

        // Schlüsselpakete gehören nicht in den Modellstand: sie sind ein
        // Auftrag an die Schlüsselverwaltung, nicht eine Anzeige.
        // Beide gehören der Schlüsselverwaltung, nicht dem Modell: der eine
        // bringt einen Schlüssel, der andere bittet darum.
        ServerFrame::Schluesselpaket { .. } | ServerFrame::SchluesselAnfrage { .. } => {}

        ServerFrame::Sperrliste { sperren } => {
            zustand.sperren = sperren
                .iter()
                .map(|b| crate::Sperre {
                    kennung: b.schluessel.clone(),
                    adresse: b.adresse.clone(),
                    grund: b.grund.clone(),
                    zeit: b.zeit,
                    handelnder_name: b.handelnder_name.clone(),
                })
                .collect();
        }

        ServerFrame::Zaehler {
            sprachrahmen,
            verteilt,
            verloren,
            abgelehnt,
            verbundene,
        } => {
            zustand.guete = Some(crate::Guete {
                sprachrahmen,
                verteilt,
                verloren,
                abgelehnt,
                verbundene,
            });
        }

        ServerFrame::Protokoll { eintraege } => {
            zustand.protokoll = eintraege.iter().map(eintrag_abbilden).collect();
        }

        ServerFrame::Raeume { raeume } => {
            let hier = zustand.aktueller_raum;
            zustand.raeume = raeume_abbilden(raeume, hier);
            zaehle_anwesende(zustand);
        }

        ServerFrame::MitgliedGeaendert { mitglied } => {
            let ich = zustand.ich();
            let neu = mitglied_abbilden(mitglied, ich);
            match zustand.mitglieder.iter_mut().find(|m| m.id == neu.id) {
                Some(vorhanden) => *vorhanden = neu,
                None => zustand.mitglieder.push(neu),
            }
        }

        ServerFrame::Fehler { text, .. } => {
            zustand.meldung = Some(text);
        }

        ServerFrame::Ok | ServerFrame::Pong => {
            zustand.meldung = None;
        }

        ServerFrame::Anforderung { .. } => {}
    }
}

/// Meldet, dass die Verbindung beendet wurde.
pub fn getrennt(zustand: &mut Zustand, grund: Option<String>) {
    zustand.verbindung = Some(match grund {
        Some(text) => Verbindung::Fehler { text },
        None => Verbindung::Getrennt,
    });
    zustand.aktueller_raum = None;
    zustand.mitglieder.clear();
    zustand.raeume.clear();
    // Der Fingerabdruck gehört zur Verbindung: ohne Verbindung kein Wert.
    zustand.zertifikat = None;
}

/// Setzt die Anwesenheitszahlen aus der Mitgliederliste (nicht aus der Meldung —
/// sonst zeigt ein Raum „0", während daneben ein Mitglied darin sitzt).
fn zaehle_anwesende(zustand: &mut Zustand) {
    let mut zahlen: std::collections::HashMap<RoomId, usize> = std::collections::HashMap::new();
    for m in &zustand.mitglieder {
        if let Some(raum) = m.raum {
            *zahlen.entry(raum).or_default() += 1;
        }
    }
    for r in zustand.raeume.iter_mut() {
        r.anwesend = zahlen.get(&r.id).copied().unwrap_or(0);
    }
}

fn raeume_abbilden(raeume: Vec<Raumansicht>, hier: Option<RoomId>) -> Vec<Raum> {
    raeume
        .into_iter()
        .map(|r| Raum {
            id: r.id,
            name: r.name,
            anwesend: r.anwesend,
            hier: hier == Some(r.id),
            verschluesselt: r.verschluesselt,
        })
        .collect()
}

fn raeume_hier_setzen(zustand: &mut Zustand, hier: Option<RoomId>) {
    for r in zustand.raeume.iter_mut() {
        r.hier = Some(r.id) == hier;
    }
}

fn mitglieder_abbilden(
    mitglieder: Vec<Mitgliedsansicht>,
    ich: Option<sprechwerk_domain::MemberId>,
) -> Vec<Mitglied> {
    let mut aus: Vec<Mitglied> = mitglieder
        .into_iter()
        .map(|m| mitglied_abbilden(m, ich))
        .collect();
    aus.sort_by_key(|m| m.name.to_lowercase());
    aus
}

fn mitglied_abbilden(m: Mitgliedsansicht, ich: Option<sprechwerk_domain::MemberId>) -> Mitglied {
    Mitglied {
        id: m.id,
        name: m.name,
        rolle: m.rolle,
        raum: m.raum,
        spricht: m.spricht,
        stumm: m.stumm,
        taub: m.taub,
        ich: Some(m.id) == ich,
    }
}

fn nachricht_abbilden(n: &Message, zustand: &Zustand) -> Nachricht {
    let eigene_rolle = zustand.meine_rolle().unwrap_or(Role::Gast);
    let ich = zustand.ich();
    let loeschbar = !n.geloescht
        && (Some(n.verfasser) == ich || darf(eigene_rolle, Capability::PersonEingreifen));
    Nachricht {
        id: n.id,
        verfasser: n.verfasser,
        verfasser_name: n.verfasser_name.clone(),
        text: n.text.clone(),
        zeit: n.zeit,
        geloescht: n.geloescht,
        loeschbar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_domain::{MemberId, MessageId};

    fn verbunden(rolle: Role) -> Zustand {
        let mut z = Zustand::neu();
        z.verbindung = Some(Verbindung::Verbunden {
            server: "127.0.0.1:7443".into(),
            server_name: "Test".into(),
            ich: MemberId::aus_roh(1),
            name: "Ada".into(),
            rolle,
            kennung: "ab".repeat(32),
        });
        z
    }

    fn raum(id: u128) -> Raumansicht {
        Raumansicht {
            id: RoomId::aus_roh(id),
            name: format!("Raum {id}"),
            reihenfolge: 0,
            anwesend: 0,
            verschluesselt: false,
            verschluesselt_seit: 0,
        }
    }

    fn mitglied(id: u128, name: &str) -> Mitgliedsansicht {
        Mitgliedsansicht {
            id: MemberId::aus_roh(id),
            name: name.to_string(),
            kennung: format!("k{id}"),
            rolle: Role::Mitglied,
            raum: None,
            spricht: false,
            stumm: false,
            taub: false,
            raum_seit: 0,
        }
    }

    fn nachricht(id: u128, verfasser: u128, text: &str, raum: RoomId) -> Message {
        Message {
            id: MessageId::aus_roh(id),
            raum,
            verfasser: MemberId::aus_roh(verfasser),
            verfasser_name: "Ada".into(),
            text: text.into(),
            zeit: 1_000,
            geloescht: false,
        }
    }

    #[test]
    fn betreten_setzt_raum_und_verlauf() {
        let mut z = verbunden(Role::Mitglied);
        let r = RoomId::aus_roh(7);
        z.raeume = raeume_abbilden(vec![raum(7)], None);
        verarbeite(
            &mut z,
            ServerFrame::Betreten {
                raum: r,
                verlauf: vec![nachricht(1, 1, "alt", r)],
                anwesend: vec![mitglied(1, "Ada")],
            },
        );
        assert_eq!(z.aktueller_raum, Some(r));
        assert_eq!(z.nachrichten.len(), 1);
        assert!(z.raeume[0].hier);
        assert!(z.mitglieder[0].ich);
    }

    #[test]
    fn text_nur_im_eigenen_raum() {
        let mut z = verbunden(Role::Mitglied);
        let hier = RoomId::aus_roh(7);
        z.aktueller_raum = Some(hier);
        verarbeite(
            &mut z,
            ServerFrame::Text {
                nachricht: nachricht(1, 2, "fremd", RoomId::aus_roh(8)),
            },
        );
        assert!(
            z.nachrichten.is_empty(),
            "anderer Raum darf nicht auftauchen"
        );
        verarbeite(
            &mut z,
            ServerFrame::Text {
                nachricht: nachricht(2, 2, "passt", hier),
            },
        );
        assert_eq!(z.nachrichten.len(), 1);
        assert_eq!(z.nachrichten[0].text, "passt");
    }

    #[test]
    fn eigene_nachricht_ist_loeschbar_moderator_darf_fremde() {
        let mut z = verbunden(Role::Mitglied);
        let hier = RoomId::aus_roh(7);
        z.aktueller_raum = Some(hier);
        verarbeite(
            &mut z,
            ServerFrame::Text {
                nachricht: nachricht(1, 1, "eigene", hier),
            },
        );
        verarbeite(
            &mut z,
            ServerFrame::Text {
                nachricht: nachricht(2, 2, "fremde", hier),
            },
        );
        assert!(z.nachrichten[0].loeschbar);
        assert!(
            !z.nachrichten[1].loeschbar,
            "Mitglied darf fremde nicht löschen"
        );

        let mut m = verbunden(Role::Moderator);
        m.aktueller_raum = Some(hier);
        verarbeite(
            &mut m,
            ServerFrame::Text {
                nachricht: nachricht(2, 2, "fremde", hier),
            },
        );
        assert!(m.nachrichten[0].loeschbar, "Moderator darf fremde löschen");
    }

    #[test]
    fn anwesenheit_kommt_und_geht() {
        let mut z = verbunden(Role::Mitglied);
        verarbeite(
            &mut z,
            ServerFrame::Anwesenheit {
                raum: RoomId::aus_roh(7),
                mitglied: mitglied(2, "Bosse"),
                anwesend: true,
            },
        );
        assert_eq!(z.mitglieder.len(), 1);
        verarbeite(
            &mut z,
            ServerFrame::Anwesenheit {
                raum: RoomId::aus_roh(7),
                mitglied: mitglied(2, "Bosse"),
                anwesend: false,
            },
        );
        assert!(z.mitglieder.is_empty());
    }

    #[test]
    fn anwesenheitszahl_kommt_aus_dem_modell() {
        let mut z = verbunden(Role::Mitglied);
        let r = RoomId::aus_roh(7);
        z.raeume = raeume_abbilden(vec![raum(7)], None);
        assert_eq!(z.raeume[0].anwesend, 0);
        verarbeite(
            &mut z,
            ServerFrame::Betreten {
                raum: r,
                verlauf: vec![],
                anwesend: vec![mitglied(1, "Ada")],
            },
        );
        assert_eq!(z.raeume[0].anwesend, 1, "Raum zeigt die Anwesenden");
        verarbeite(
            &mut z,
            ServerFrame::Anwesenheit {
                raum: r,
                mitglied: mitglied(2, "Bosse"),
                anwesend: true,
            },
        );
        assert_eq!(z.raeume[0].anwesend, 2);
    }

    #[test]
    fn fehler_landet_als_meldung() {
        let mut z = verbunden(Role::Gast);
        verarbeite(
            &mut z,
            ServerFrame::Fehler {
                code: sprechwerk_protocol::Fehlercode::KeinRecht,
                text: "dafür fehlt das Recht".into(),
            },
        );
        assert_eq!(z.meldung.as_deref(), Some("dafür fehlt das Recht"));
        verarbeite(&mut z, ServerFrame::Ok);
        assert!(z.meldung.is_none());
    }

    #[test]
    fn trennen_raeumt_auf() {
        let mut z = verbunden(Role::Mitglied);
        z.nachrichten = vec![];
        getrennt(&mut z, Some("Server weg".into()));
        assert_eq!(
            z.verbindung,
            Some(Verbindung::Fehler {
                text: "Server weg".into()
            })
        );
        assert!(z.mitglieder.is_empty() && z.raeume.is_empty());
    }
}

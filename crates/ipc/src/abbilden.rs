//! Rahmen des Servers in den Modellstand übersetzen.
//!
//! Diese Übersetzung ist **Logik**, nicht Darstellung — deshalb liegt sie hier
//! und nicht im Fenster (AGENTS.md §2.4). Sie ist rein: gleicher Rahmen plus
//! gleicher Stand ergibt denselben neuen Stand, ohne Netz, ohne Uhr, ohne
//! Seitenwirkung. Damit ist sie ohne Sockets prüfbar.

use crate::{Mitglied, Nachricht, Protokolleintrag, Raum, Verbindung, Zustand};
use sprechwerk_domain::role::darf;
use sprechwerk_domain::{Capability, Message, Role, RoomId};
use sprechwerk_protocol::{Mitgliedsansicht, Raumansicht, ServerFrame};

/// Baut den Anfangszustand aus der Begrüßung.
pub fn aus_willkommen(
    server: &str,
    server_name: String,
    ich: sprechwerk_domain::MemberId,
    mein_name: String,
    rolle: Role,
    raeume: Vec<Raumansicht>,
    mitglieder: Vec<Mitgliedsansicht>,
    verlauf: Vec<Message>,
) -> Zustand {
    let mut z = Zustand::neu();
    z.verbindung = Some(Verbindung::Verbunden {
        server: server.to_string(),
        server_name: server_name.clone(),
        ich,
        name: mein_name,
        rolle,
    });
    z.raeume = raeume_abbilden(raeume, None);
    z.mitglieder = mitglieder_abbilden(mitglieder, Some(ich));
    // Der zuletzt besuchte Raum: den Verlauf zeigen wir erst, wenn der Client
    // ihn betritt — bis dahin nur die Räume.
    let _ = verlauf;
    z
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
            *zustand = aus_willkommen(
                &server,
                server_name,
                ich,
                name,
                rolle,
                raeume,
                mitglieder,
                verlauf,
            );
        }

        ServerFrame::Betreten {
            raum,
            verlauf,
            anwesend,
        } => {
            zustand.aktueller_raum = Some(raum);
            zustand.nachrichten = verlauf.iter().map(|n| nachricht_abbilden(n, zustand)).collect();
            zustand.mitglieder = mitglieder_abbilden(anwesend, zustand.ich());
            raeume_hier_setzen(zustand, Some(raum));
        }

        ServerFrame::Anwesenheit {
            mitglied,
            anwesend,
            ..
        } => {
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

        ServerFrame::Raeume { raeume } => {
            let hier = zustand.aktueller_raum;
            zustand.raeume = raeume_abbilden(raeume, hier);
        }

        ServerFrame::MitgliedGeaendert { mitglied } => {
            let ich = zustand.ich();
            let neu = mitglied_abbilden(mitglied, ich);
            match zustand.mitglieder.iter_mut().find(|m| m.id == neu.id) {
                Some(vorhanden) => *vorhanden = neu,
                None => zustand.mitglieder.push(neu),
            }
        }

        ServerFrame::Protokoll { eintraege } => {
            zustand.protokoll = eintraege
                .into_iter()
                .map(|e| Protokolleintrag {
                    laufnummer: e.laufnummer,
                    zeit: e.zeit,
                    handelnder_name: e.handelnder_name,
                    aktion: e.aktion,
                    ziel: e.ziel,
                    ergebnis: e.ergebnis,
                })
                .collect();
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
}

fn raeume_abbilden(raeume: Vec<Raumansicht>, hier: Option<RoomId>) -> Vec<Raum> {
    // Tiefe aus der Elternkette, damit die Oberfläche einrücken kann, ohne den
    // Baum selbst nachzubauen.
    let eltern: std::collections::HashMap<RoomId, Option<RoomId>> =
        raeume.iter().map(|r| (r.id, r.eltern)).collect();
    raeume
        .into_iter()
        .map(|r| {
            let mut tiefe = 0usize;
            let mut lauf = r.eltern;
            while let Some(e) = lauf {
                tiefe += 1;
                lauf = *eltern.get(&e).unwrap_or(&None);
                if tiefe > 64 {
                    break; // Schutz gegen kaputte Daten
                }
            }
            Raum {
                id: r.id,
                name: r.name,
                eltern: r.eltern,
                tiefe,
                anwesend: r.anwesend,
                hier: hier == Some(r.id),
            }
        })
        .collect()
}

fn raeume_hier_setzen(zustand: &mut Zustand, hier: Option<RoomId>) {
    for r in zustand.raeume.iter_mut() {
        r.hier = Some(r.id) == hier;
    }
}

fn mitglieder_abbilden(mitglieder: Vec<Mitgliedsansicht>, ich: Option<sprechwerk_domain::MemberId>) -> Vec<Mitglied> {
    let mut aus: Vec<Mitglied> = mitglieder
        .into_iter()
        .map(|m| mitglied_abbilden(m, ich))
        .collect();
    aus.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    aus
}

fn mitglied_abbilden(m: Mitgliedsansicht, ich: Option<sprechwerk_domain::MemberId>) -> Mitglied {
    Mitglied {
        id: m.id,
        name: m.name,
        rolle: m.rolle,
        raum: m.raum,
        spricht: m.spricht,
        ich: Some(m.id) == ich,
    }
}

fn nachricht_abbilden(n: &Message, zustand: &Zustand) -> Nachricht {
    let eigene_rolle = zustand.meine_rolle().unwrap_or(Role::Gast);
    let ich = zustand.ich();
    let loeschbar = n.geloescht == false
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
        });
        z
    }

    fn raum(id: u128, eltern: Option<u128>) -> Raumansicht {
        Raumansicht {
            id: RoomId::aus_roh(id),
            name: format!("Raum {id}"),
            eltern: eltern.map(RoomId::aus_roh),
            reihenfolge: 0,
            anwesend: 0,
        }
    }

    fn mitglied(id: u128, name: &str) -> Mitgliedsansicht {
        Mitgliedsansicht {
            id: MemberId::aus_roh(id),
            name: name.to_string(),
            rolle: Role::Mitglied,
            raum: None,
            spricht: false,
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
    fn raumtiefe_wird_gerechnet() {
        let raeume = vec![raum(1, None), raum(2, Some(1)), raum(3, Some(2))];
        let abgebildet = raeume_abbilden(raeume, Some(RoomId::aus_roh(2)));
        assert_eq!(abgebildet[0].tiefe, 0);
        assert_eq!(abgebildet[1].tiefe, 1);
        assert_eq!(abgebildet[2].tiefe, 2);
        assert!(abgebildet[1].hier);
        assert!(!abgebildet[0].hier);
    }

    #[test]
    fn betreten_setzt_raum_und_verlauf() {
        let mut z = verbunden(Role::Mitglied);
        let r = RoomId::aus_roh(7);
        z.raeume = raeume_abbilden(vec![raum(7, None)], None);
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
        assert!(z.nachrichten.is_empty(), "anderer Raum darf nicht auftauchen");
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
        assert!(!z.nachrichten[1].loeschbar, "Mitglied darf fremde nicht löschen");

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
        assert_eq!(z.verbindung, Some(Verbindung::Fehler { text: "Server weg".into() }));
        assert!(z.mitglieder.is_empty() && z.raeume.is_empty());
    }
}

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Oberfläche ohne Server und ohne Soundkarte (AGENTS.md §4): ein
//! Beispielmodell hinein, über AccessKit bedienen (Rollen und Namen müssen
//! stimmen, sonst findet der Test nichts), die entstandenen Aktionen prüfen.
//!
//! Mit `SPRECHWERK_BILDER=<verzeichnis>` schreibt der Lauf außerdem die
//! Bildschirmfotos für README und Doku — erzeugt, nicht von Hand abgelegt.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sprechwerk_client::einstellungen::{Einstellungen, Favorit};
use sprechwerk_client::sitzung::{Aktion, Meldung};
use sprechwerk_client::AudioStand;
use sprechwerk_domain::{MemberId, MessageId, Role, RoomId};
use sprechwerk_fenster::app::{Fenster, Kern};
use sprechwerk_fenster::modell::{Ansicht, Oberflaeche};
use sprechwerk_fenster::thema::Thema;
use sprechwerk_ipc::{
    Befehl, Eintrag, Guete, Mitglied, Nachricht, NachrichtInhalt, Raum, Reaktionen, Sperre,
    Textschluessel, Verbindung, Zustand,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

#[derive(Clone, Default)]
struct Falsch(Rc<RefCell<Vec<Aktion>>>);

impl Kern for Falsch {
    fn ausfuehren(&self, aktion: Aktion) {
        self.0.borrow_mut().push(aktion);
    }
    fn audio_stand(&self) -> AudioStand {
        AudioStand {
            laeuft: true,
            pegel: 42,
            stumm: false,
            taub: false,
        }
    }
}

const ICH: u128 = 1;
const ZWEI: u128 = 2;

fn mitglied(id: u128, name: &str, rolle: Role, raum: u128, spricht: bool) -> Mitglied {
    Mitglied {
        id: MemberId::aus_roh(id),
        name: name.into(),
        rolle,
        raum: Some(RoomId::aus_roh(raum)),
        spricht,
        stumm: false,
        taub: false,
        ich: id == ICH,
    }
}

fn nachricht(id: u128, von: u128, name: &str, text: &str, minute: i64) -> Nachricht {
    Nachricht {
        id: MessageId::aus_roh(id),
        verfasser: MemberId::aus_roh(von),
        verfasser_name: name.into(),
        inhalt: NachrichtInhalt::Text {
            text: text.into(),
            effekt: None,
            vorschau: None,
        },
        roh: None,
        zeit: 1_763_460_000_000 + minute * 60_000,
        geloescht: false,
        loeschbar: von == ICH,
        reaktionen: Vec::new(),
    }
}

/// Ein Vereinsabend: drei Räume, vier Leute, ein paar Zeilen Chat.
fn beispiel(rolle: Role) -> Zustand {
    let mut z = Zustand::neu();
    z.verbindung = Some(Verbindung::Verbunden {
        server: "verein.example:7443".into(),
        server_name: "Schachclub Nord".into(),
        ich: MemberId::aus_roh(ICH),
        name: "Ada".into(),
        rolle,
        kennung: "3fa1c2e94b7d0a5e".repeat(4),
    });
    z.aktueller_raum = Some(RoomId::aus_roh(1));
    z.raeume = vec![
        Raum {
            id: RoomId::aus_roh(1),
            name: "Eingang".into(),
            anwesend: 3,
            hier: true,
            verschluesselt: true,
        },
        Raum {
            id: RoomId::aus_roh(2),
            name: "Training".into(),
            anwesend: 1,
            hier: false,
            verschluesselt: true,
        },
        Raum {
            id: RoomId::aus_roh(3),
            name: "Vorstand".into(),
            anwesend: 0,
            hier: false,
            verschluesselt: true,
        },
    ];
    z.mitglieder = vec![
        mitglied(ICH, "Ada", rolle, 1, false),
        mitglied(ZWEI, "Bosse", Role::Moderator, 1, true),
        mitglied(3, "Carla", Role::Mitglied, 1, false),
        mitglied(4, "Dima", Role::Gast, 2, false),
    ];
    z.schluessel = vec![sprechwerk_ipc::Schluesselstand {
        raum: RoomId::aus_roh(1),
        epoche: 3,
    }];
    z.textschluessel = Textschluessel::Bereit;
    let mut erste = nachricht(
        10,
        ZWEI,
        "Bosse",
        "Guten Abend! Heute Blitzschach ab 20 Uhr.",
        0,
    );
    erste.reaktionen = vec![Reaktionen {
        zeichen: "👍".into(),
        anzahl: 2,
        ich: true,
        namen: vec!["Ada".into(), "Carla".into()],
    }];
    z.nachrichten = vec![
        erste,
        nachricht(
            11,
            3,
            "Carla",
            "Die Paarungen stehen hier: https://verein.example/turnier",
            2,
        ),
        nachricht(12, ICH, "Ada", "Bin in fünf Minuten da.", 3),
    ];
    z.latenz_ms = Some(8);
    z.guete = Some(Guete {
        sprachrahmen: 1200,
        verteilt: 3600,
        verloren: 0,
        abgelehnt: 0,
        verbundene: 4,
    });
    z.zertifikat = Some("51a2155620e33bd9".repeat(4));
    z.protokoll = vec![
        Eintrag {
            laufnummer: 2,
            zeit: 1_763_460_100_000,
            handelnder_name: "Ada".into(),
            aktion: "raum_angelegt".into(),
            ziel: "Vorstand".into(),
            vorher: None,
            nachher: None,
            ergebnis: "erlaubt".into(),
            grund: None,
        },
        Eintrag {
            laufnummer: 3,
            zeit: 1_763_460_200_000,
            handelnder_name: "Dima".into(),
            aktion: "raum_geloescht".into(),
            ziel: "Training".into(),
            vorher: None,
            nachher: None,
            ergebnis: "abgelehnt: kein Recht".into(),
            grund: None,
        },
    ];
    z.sperren = vec![Sperre {
        kennung: Some("9c0e".repeat(16)),
        adresse: None,
        grund: "Werbung im Chat".into(),
        zeit: 1_763_460_000_000,
        handelnder_name: "Ada".into(),
    }];
    z
}

fn einstellungen() -> Einstellungen {
    Einstellungen {
        name: "Ada".into(),
        favoriten: vec![
            Favorit {
                name: "Schachclub Nord".into(),
                server: "verein.example:7443".into(),
                zuletzt: 0,
            },
            Favorit {
                name: "Probe".into(),
                server: "127.0.0.1:7443".into(),
                zuletzt: 0,
            },
        ],
        ..Default::default()
    }
}

type Stand = (Fenster<Falsch>, mpsc::Sender<Meldung>);

fn fenster(o: Oberflaeche, groesse: (f32, f32)) -> (Harness<'static, Stand>, Falsch) {
    let kern = Falsch::default();
    let (tx, rx) = mpsc::channel();
    let f = Fenster::neu(kern.clone(), rx, o);
    let h = Harness::builder()
        .with_size(groesse)
        .with_context_setup(sprechwerk_fenster::thema::schriften)
        .build_ui_state(|ui, (f, _): &mut Stand| f.bild(ui), (f, tx));
    (h, kern)
}

fn verbunden(rolle: Role, ansicht: Ansicht) -> Oberflaeche {
    let mut o = Oberflaeche::neu(einstellungen(), beispiel(rolle));
    o.ansicht = ansicht;
    o
}

#[test]
fn serverliste_verbindet_mit_einem_klick() {
    let o = Oberflaeche::neu(einstellungen(), Zustand::neu());
    let (mut h, kern) = fenster(o, (1000.0, 700.0));
    h.run();
    h.get_all_by_label("Verbinden")
        .next()
        .expect("Knopf Verbinden")
        .click();
    h.run();
    let aktionen = kern.0.borrow();
    assert!(
        aktionen
            .iter()
            .any(|a| matches!(a, Aktion::Verbinden { server, name, .. }
            if server == "verein.example:7443" && name == "Ada")),
        "{aktionen:?}"
    );
}

#[test]
fn chat_sendet_mit_enter() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    let feld = h.get_by_role(egui::accesskit::Role::MultilineTextInput);
    feld.focus();
    feld.type_text("Hallo zusammen");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let aktionen = kern.0.borrow();
    assert!(
        aktionen.iter().any(
            |a| matches!(a, Aktion::Befehl(Befehl::Text { text, .. }) if text == "Hallo zusammen")
        ),
        "{aktionen:?}"
    );
}

#[test]
fn raum_wechseln_per_klick() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label_contains("Training").click();
    h.run();
    assert!(kern.0.borrow().iter().any(
        |a| matches!(a, Aktion::Befehl(Befehl::Betreten { raum }) if *raum == RoomId::aus_roh(2))
    ));
}

#[test]
fn reiter_folgen_der_rolle() {
    let (mut h, _) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    assert!(
        h.query_by_label("Protokoll").is_none(),
        "Mitglied sieht kein Protokoll"
    );
    assert!(h.query_by_label("Sperren").is_none());

    let (mut h, kern) = fenster(
        verbunden(Role::Administrator, Ansicht::Raeume),
        (1100.0, 720.0),
    );
    h.run();
    h.get_by_label("Protokoll").click();
    h.run();
    assert!(kern
        .0
        .borrow()
        .iter()
        .any(|a| matches!(a, Aktion::Befehl(Befehl::Protokoll { .. }))));
    assert_eq!(h.state().0.o.ansicht, Ansicht::Protokoll);
}

#[test]
fn trennen_stoppt_auch_den_ton() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label("Verbindung trennen").click();
    h.run();
    let a = kern.0.borrow();
    assert!(a.contains(&Aktion::Befehl(Befehl::Trennen)));
    assert!(a.contains(&Aktion::AudioStoppen));
}

#[test]
fn zustand_aus_dem_client_kommt_an() {
    let o = Oberflaeche::neu(einstellungen(), Zustand::neu());
    let (mut h, _) = fenster(o, (1000.0, 700.0));
    h.run();
    assert!(h.query_by_label("Räume").is_none());
    let neu = beispiel(Role::Mitglied);
    h.state().1.send(Meldung::Zustand(Box::new(neu))).unwrap();
    h.run();
    assert!(
        h.query_by_label("Räume").is_some(),
        "nach dem Verbinden gibt es den Reiter"
    );
}

#[test]
fn alle_ansichten_in_allen_themes() {
    let ziel = std::env::var_os("SPRECHWERK_BILDER").map(std::path::PathBuf::from);
    for thema in Thema::ALLE {
        for (name, ansicht) in [
            ("server", Ansicht::Server),
            ("raeume", Ansicht::Raeume),
            ("einstellungen", Ansicht::Einstellungen),
            ("protokoll", Ansicht::Protokoll),
            ("sperren", Ansicht::Sperren),
        ] {
            let mut o = verbunden(Role::Administrator, ansicht);
            o.einstellungen.thema = thema.name().into();
            let (mut h, _) = fenster(o, (1200.0, 760.0));
            h.run();
            // Jede Ansicht hat einen Inhalt mit Namen (Screenreader).
            assert!(
                h.query_by_label("sprechwerk").is_some(),
                "{name}/{}",
                thema.name()
            );
            if let Some(ziel) = &ziel {
                let bild = h.render().expect("zeichnen (wgpu)");
                std::fs::create_dir_all(ziel).unwrap();
                bild.save(ziel.join(format!("{name}-{}.png", thema.name())))
                    .unwrap();
            }
        }
    }
}

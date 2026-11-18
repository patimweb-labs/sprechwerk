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
    Anhangentwurf, Befehl, Betrachter, Dateiansicht, Dateiart, Eintrag, Entwurfsstand, Guete,
    Ladestand, Mitglied, Nachricht, NachrichtInhalt, Raum, Reaktionen, Sperre, Textschluessel,
    Verbindung, Zustand,
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
    fn datei(&self, id: &str) -> Option<Vec<u8>> {
        (id == BILD).then(testbild)
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
const BILD: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// Ein Bild mit Verlauf in den Akzentfarben — als PNG, wie es aus dem
/// Zwischenspeicher käme.
fn testbild() -> Vec<u8> {
    let bild = image::RgbaImage::from_fn(480, 300, |x, y| {
        image::Rgba([(x * 255 / 480) as u8, 120, (y * 255 / 300) as u8, 255])
    });
    let mut aus = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(bild)
        .write_to(&mut aus, image::ImageFormat::Png)
        .unwrap();
    aus.into_inner()
}

fn dateiansicht(id: &str, name: &str, art: Dateiart, groesse: u64) -> Dateiansicht {
    Dateiansicht {
        id: id.into(),
        name: name.into(),
        art,
        typ: String::new(),
        groesse,
        vorschau: None,
        breite: (art == Dateiart::Bild).then_some(480),
        hoehe: (art == Dateiart::Bild).then_some(300),
        stand: if art == Dateiart::Bild {
            Ladestand::Bereit
        } else {
            Ladestand::Keiner
        },
    }
}
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
        Nachricht {
            inhalt: NachrichtInhalt::Anhang {
                text: "Fotos und Video vom letzten Turnier".into(),
                dateien: vec![
                    dateiansicht(BILD, "brett.png", Dateiart::Bild, 412_000),
                    dateiansicht(
                        "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
                        "partie.mp4",
                        Dateiart::Video,
                        48_200_000,
                    ),
                    dateiansicht(
                        "ffeeddccbbaa99887766554433221100",
                        "paarungen.pdf",
                        Dateiart::Datei,
                        182_000,
                    ),
                ],
            },
            ..nachricht(13, 3, "Carla", "", 4)
        },
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
    // Kurzer Bildtakt: zwei Klicks hintereinander gelten als Doppelklick.
    let h = Harness::builder()
        .with_size(groesse)
        .with_step_dt(1.0 / 30.0)
        .with_context_setup(sprechwerk_oberflaeche::einrichten)
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
fn raum_wechseln_wie_bei_teamspeak() {
    let wechselt = |kern: &Falsch| {
        kern.0.borrow().iter().any(|a| {
            matches!(a, Aktion::Befehl(Befehl::Betreten { raum }) if *raum == RoomId::aus_roh(2))
        })
    };
    // Ein einfacher Mausklick zieht niemanden um.
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label_contains("Training").click();
    h.run();
    assert!(!wechselt(&kern), "ein Klick wechselt nicht");
    // Doppelklick wechselt.
    h.get_by_label_contains("Training").click();
    h.step();
    h.get_by_label_contains("Training").click();
    h.run();
    assert!(wechselt(&kern), "Doppelklick wechselt");
    // Ohne Maus (Tastatur, Screenreader) genügt das Auslösen.
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label_contains("Training").click_accesskit();
    h.run();
    assert!(wechselt(&kern), "Auslösen ohne Maus wechselt");
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
    assert!(h.query_by_label("Verbindung trennen").is_none());
    let neu = beispiel(Role::Mitglied);
    h.state().1.send(Meldung::Zustand(Box::new(neu))).unwrap();
    h.run();
    assert!(
        h.query_by_label("Verbindung trennen").is_some(),
        "nach dem Verbinden gibt es den Reiter"
    );
}

#[test]
fn chat_einklappen_und_wieder_aufklappen() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label("Chat einklappen").click();
    h.run();
    assert!(h.state().0.o.einstellungen.chat_eingeklappt);
    assert!(
        kern.0.borrow().iter().any(|a| matches!(a,
            Aktion::EinstellungenSpeichern(e) if e.chat_eingeklappt)),
        "der Zustand wird gemerkt"
    );
    // Eingeklappt: kein Eingabefeld, aber die Schiene mit dem Knopf zurück.
    assert!(h
        .query_by_role(egui::accesskit::Role::MultilineTextInput)
        .is_none());
    h.get_by_label("Chat ausklappen").click();
    h.run();
    assert!(!h.state().0.o.einstellungen.chat_eingeklappt);
    assert!(h
        .query_by_role(egui::accesskit::Role::MultilineTextInput)
        .is_some());
}

/// Bei jeder Breite bis hinunter zur Mindestgröße stehen die Reiter links von
/// den Knöpfen rechts — nichts schiebt sich übereinander.
#[test]
fn kopfleiste_ueberlappt_bei_keiner_breite() {
    for breite in [720.0, 820.0, 960.0, 1400.0] {
        let (mut h, _) = fenster(
            verbunden(Role::Administrator, Ansicht::Raeume),
            (breite, 700.0),
        );
        h.run();
        if let Some(ziel) = std::env::var_os("SPRECHWERK_KOPFBILDER") {
            let bild = h.render().expect("zeichnen (wgpu)");
            bild.save(std::path::Path::new(&ziel).join(format!("kopf-{breite}.png")))
                .unwrap();
        }
        // Der Einklappknopf des Chats bleibt im Fenster.
        let einklappen = h.get_by_label("Chat einklappen").rect();
        assert!(
            einklappen.right() <= breite,
            "Breite {breite}: Einklappknopf außerhalb"
        );
        // Der Servername steht zweimal da (Werkzeugleiste, Wurzel des Baums);
        // gemeint ist der obere.
        let links = h
            .get_all_by_label("Schachclub Nord")
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap();
        let rechts = h.get_by_label("Mikrofon stummschalten").rect();
        assert!(
            links.right() <= rechts.left(),
            "Breite {breite}: Reiter endet bei {} , Knöpfe beginnen bei {}",
            links.right(),
            rechts.left()
        );
    }
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
            ("raeume-eingeklappt", Ansicht::Raeume),
        ] {
            let mut o = verbunden(Role::Administrator, ansicht);
            o.einstellungen.chat_eingeklappt = name == "raeume-eingeklappt";
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

/// Alle Bedienelemente liegen waagerecht im Fenster — beim Verkleinern
/// verschwindet nichts über den Rand. (Senkrecht darf gescrollt werden.)
fn alles_im_fenster(h: &Harness<'static, Stand>, breite: f32, wo: &str) {
    use egui::accesskit::Role;
    for rolle in [
        Role::Button,
        Role::TextInput,
        Role::MultilineTextInput,
        Role::PasswordInput,
        Role::ComboBox,
        Role::CheckBox,
        Role::Slider,
    ] {
        for knoten in h.query_all(egui_kittest::kittest::By::new().role(rolle)) {
            let r = knoten.rect();
            assert!(
                r.left() >= -0.5 && r.right() <= breite + 0.5,
                "{wo} bei {breite} px: {rolle:?} „{}“ reicht von {} bis {}",
                egui_kittest::kittest::NodeT::accesskit_node(&knoten)
                    .label()
                    .unwrap_or_default(),
                r.left(),
                r.right()
            );
        }
    }
}

#[test]
fn verkleinern_laesst_nichts_verschwinden() {
    for breite in [360.0, 560.0, 800.0] {
        for (name, ansicht, eingeklappt) in [
            ("server", Ansicht::Server, false),
            ("raeume", Ansicht::Raeume, true),
            ("einstellungen", Ansicht::Einstellungen, false),
            ("protokoll", Ansicht::Protokoll, false),
            ("sperren", Ansicht::Sperren, false),
        ] {
            let mut o = verbunden(Role::Administrator, ansicht);
            o.einstellungen.chat_eingeklappt = eingeklappt;
            let (mut h, _) = fenster(o, (breite, 700.0));
            h.run();
            if let Some(ziel) = std::env::var_os("SPRECHWERK_SCHMALBILDER") {
                let bild = h.render().expect("zeichnen (wgpu)");
                bild.save(std::path::Path::new(&ziel).join(format!("{name}-{breite}.png")))
                    .unwrap();
            }
            alles_im_fenster(&h, breite, name);
        }
    }
}

#[test]
fn anhaengen_waehlt_dateien() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label("Datei anhängen").click();
    h.run();
    assert!(kern.0.borrow().contains(&Aktion::AnhaengeWaehlen));
}

#[test]
fn dateien_im_verlauf() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 900.0));
    h.run();
    h.get_by_label("brett.png groß zeigen").click();
    h.run();
    h.get_by_label("Abspielen: partie.mp4").click();
    h.run();
    h.get_by_label("paarungen.pdf speichern").click();
    h.run();
    let a = kern.0.borrow();
    assert!(
        a.iter().any(
            |x| matches!(x, Aktion::Befehl(Befehl::DateiOeffnen { datei, .. }) if datei == BILD)
        ),
        "{a:?}"
    );
    assert!(
        a.iter()
            .any(|x| matches!(x, Aktion::DateiExtern { name, .. } if name == "partie.mp4")),
        "Video im Player"
    );
    assert!(
        a.iter().any(
            |x| matches!(x, Aktion::DateiSpeichernUnter { name, .. } if name == "paarungen.pdf")
        ),
        "PDF speichern"
    );
}

#[test]
fn betrachter_zeigt_und_schliesst() {
    let mut o = verbunden(Role::Mitglied, Ansicht::Raeume);
    o.zustand.betrachter = Some(Betrachter {
        nachricht: MessageId::aus_roh(13),
        datei: dateiansicht(BILD, "brett.png", Dateiart::Bild, 412_000),
    });
    let (mut h, kern) = fenster(o, (1100.0, 720.0));
    h.run();
    h.get_by_label("Schließen").click();
    h.run();
    assert!(kern
        .0
        .borrow()
        .contains(&Aktion::Befehl(Befehl::BetrachterSchliessen)));
}

#[test]
fn entwurf_mit_dateien_senden_und_entfernen() {
    let mut o = verbunden(Role::Mitglied, Ansicht::Raeume);
    o.zustand.anhaenge = vec![Anhangentwurf {
        nr: 7,
        name: "brett.png".into(),
        art: Dateiart::Bild,
        groesse: 412_000,
        vorschau: None,
        stand: Entwurfsstand::Bereit,
    }];
    let (mut h, kern) = fenster(o, (1100.0, 720.0));
    h.run();
    // Nur Dateien, ohne Text: Enter schickt trotzdem.
    h.get_by_role(egui::accesskit::Role::MultilineTextInput)
        .focus();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.get_by_label("brett.png entfernen").click();
    h.run();
    let a = kern.0.borrow();
    assert!(
        a.iter()
            .any(|x| matches!(x, Aktion::Befehl(Befehl::Text { text }) if text.is_empty())),
        "{a:?}"
    );
    assert!(a.contains(&Aktion::Befehl(Befehl::AnhangEntfernen { nr: 7 })));
}

#[test]
fn schwelle_direkt_im_pegelbalken_setzen() {
    let mut o = verbunden(Role::Mitglied, Ansicht::Einstellungen);
    o.einstellungen.schwelle = 30;
    let (mut h, _) = fenster(o, (1100.0, 2400.0));
    h.run();
    if let Some(ziel) = std::env::var_os("SPRECHWERK_SCHMALBILDER") {
        let bild = h.render().expect("zeichnen (wgpu)");
        bild.save(std::path::Path::new(&ziel).join("einstellungen-lang.png"))
            .unwrap();
    }
    let balken = h.get_by_role_and_label(
        egui::accesskit::Role::Slider,
        "Sprechschwelle (bei offenem Mikrofon)",
    );
    // Klick ins rechte Viertel: die Schwelle springt dorthin.
    let r = balken.rect();
    let ziel = egui::pos2(r.left() + r.width() * 0.75, r.center().y);
    h.input_mut().events.push(egui::Event::PointerMoved(ziel));
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: ziel,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: ziel,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.run();
    let schwelle = h.state().0.o.einstellungen.schwelle;
    assert!((73..=77).contains(&schwelle), "Schwelle {schwelle}");
}

#[test]
fn sprechtest_ohne_verbindung_oeffnet_das_mikrofon() {
    let mut o = Oberflaeche::neu(einstellungen(), Zustand::neu());
    o.ansicht = Ansicht::Einstellungen;
    let (mut h, kern) = fenster(o, (1100.0, 2400.0));
    h.run();
    h.get_by_label("Sprechtest starten").click();
    h.run();
    assert!(kern.0.borrow().contains(&Aktion::AudioStarten));
    assert!(h.query_by_label("Sprechtest beenden").is_some());
}

#[test]
fn leere_sperrliste_steht_im_seitenrahmen() {
    let mut o = verbunden(Role::Administrator, Ansicht::Sperren);
    o.zustand.sperren.clear();
    let (mut h, _) = fenster(o, (1000.0, 500.0));
    h.run();
    if let Some(ziel) = std::env::var_os("SPRECHWERK_SCHMALBILDER") {
        let bild = h.render().expect("zeichnen (wgpu)");
        bild.save(std::path::Path::new(&ziel).join("sperren-leer.png"))
            .unwrap();
    }
    // Überschrift und leere Ansicht beginnen an derselben Kante.
    let titel = h.get_by_label("Verhängte Sperren").rect();
    let leer = h.get_by_label("Niemand gesperrt").rect();
    assert!(
        (titel.left() - leer.left()).abs() < 24.0,
        "{titel:?} {leer:?}"
    );
    assert!(titel.left() >= 20.0, "Rand zur Fensterkante");
}

#[test]
fn emoji_ins_eingabefeld() {
    let (mut h, _) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_label("Emoji einfügen").click();
    h.run();
    h.get_by_label("😎").click();
    h.run();
    assert_eq!(h.state().0.o.entwurf, "😎");
}

#[test]
fn auf_fremde_nachricht_reagieren() {
    let (mut h, kern) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 900.0));
    h.run();
    // Über die Nachricht fahren: der Knopf zum Reagieren erscheint.
    h.get_by_label("Guten Abend! Heute Blitzschach ab 20 Uhr.")
        .hover();
    h.run_steps(3);
    h.get_all_by_label("Reagieren")
        .next()
        .expect("Knopf")
        .click();
    h.run_steps(3);
    if let Some(ziel) = std::env::var_os("SPRECHWERK_SCHMALBILDER") {
        let bild = h.render().expect("zeichnen (wgpu)");
        bild.save(std::path::Path::new(&ziel).join("reagieren.png"))
            .unwrap();
    }
    h.get_by_label("🚀").click();
    h.run_steps(3);
    assert!(
        kern.0.borrow().iter().any(|a| matches!(a,
            Aktion::Befehl(Befehl::Reagieren { zeichen, .. }) if zeichen == "🚀")),
        "{:?}",
        kern.0.borrow()
    );
}

#[test]
fn rechtsklick_ins_eingabefeld_bietet_einfuegen() {
    let (mut h, _) = fenster(verbunden(Role::Mitglied, Ansicht::Raeume), (1100.0, 720.0));
    h.run();
    h.get_by_role(egui::accesskit::Role::MultilineTextInput)
        .click_secondary();
    h.run();
    for name in ["Ausschneiden", "Kopieren", "Einfügen", "Alles markieren"] {
        assert!(h.query_by_label(name).is_some(), "{name} fehlt");
    }
}

/// Jedes Emoji der Auswahl kann die eingebaute Schrift zeichnen — sonst
/// stünde ein Kästchen in der Auswahl.
#[test]
fn jedes_emoji_der_auswahl_ist_darstellbar() {
    let ctx = egui::Context::default();
    sprechwerk_oberflaeche::einrichten(&ctx);
    let mut aus = ctx.run_ui(Default::default(), |_| {});
    aus.textures_delta.clear();
    let fehlt: Vec<&str> = sprechwerk_fenster::logik::EMOJI
        .iter()
        .flat_map(|(_, e)| e.iter().copied())
        .filter(|z| {
            let z = sprechwerk_fenster::logik::darstellbar(z);
            !ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(20.0), &z))
        })
        .collect();
    assert!(fehlt.is_empty(), "ohne Glyphe: {fehlt:?}");
}

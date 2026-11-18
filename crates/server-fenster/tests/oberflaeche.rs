// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Das Server-Fenster ohne laufenden Dienst: Knöpfe, Seiten, Rollen und Namen
//! (AccessKit), Bildschirmfotos mit `SPRECHWERK_BILDER`.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sprechwerk_domain::{MemberId, Role};
use sprechwerk_oberflaeche::thema::Thema;
use sprechwerk_server::mitschrift::Zeile;
use sprechwerk_server::verwaltung::{Mitgliedszeile, Sicherungsstand, Stand};
use sprechwerk_server_fenster::{zeichnen, Aktion, Modell, Seite};

type Stand2 = (Modell, Vec<Aktion>);

fn fenster(m: Modell) -> Harness<'static, Stand2> {
    Harness::builder()
        .with_size((1100.0, 700.0))
        .with_context_setup(|ctx| {
            sprechwerk_oberflaeche::thema::schriften(ctx);
        })
        .build_ui_state(
            |ui, (m, aktionen): &mut Stand2| {
                sprechwerk_oberflaeche::thema::anwenden(ui.ctx(), m.thema);
                aktionen.extend(zeichnen(ui, m));
            },
            (m, Vec::new()),
        )
}

fn beispielstand() -> Stand {
    Stand {
        name: "Schachclub Nord".into(),
        version: "1.4.0".into(),
        adresse: "0.0.0.0:7443".into(),
        port: 7443,
        fingerabdruck: "51a2155620e33bd9".repeat(4),
        gestartet: 1_763_460_000_000,
        jetzt: 1_763_463_600_000,
        raeume: 3,
        verbundene: 2,
        erster_wird_administrator: false,
        sicherung: Sicherungsstand {
            aktiv: true,
            verzeichnis: "/data/sicherungen".into(),
            letzte: Some(1_763_460_000_000),
        },
        mitglieder: vec![
            Mitgliedszeile {
                id: MemberId::aus_roh(1),
                name: "Ada".into(),
                kennung: "3fa1c2e94b7d0a5e".repeat(4),
                rolle: Role::Administrator,
                verbunden: true,
                raum: Some("Eingang".into()),
                zuletzt_gesehen: 0,
            },
            Mitgliedszeile {
                id: MemberId::aus_roh(2),
                name: "Bosse".into(),
                kennung: "9c0e7a11b2d4f600".repeat(4),
                rolle: Role::Mitglied,
                verbunden: false,
                raum: None,
                zuletzt_gesehen: 0,
            },
        ],
    }
}

#[test]
fn gestoppt_zeigt_startknopf() {
    let mut h = fenster(Modell::default());
    h.run();
    h.get_by_label("Server starten").click();
    h.run();
    assert_eq!(h.state().1, vec![Aktion::Starten]);
}

#[test]
fn seiten_wechseln() {
    let mut h = fenster(Modell::default());
    h.run();
    h.get_by_label("Einstellungen").click();
    h.run();
    assert_eq!(h.state().0.seite, Seite::Einstellungen);
    assert!(h.query_by_label("Datenverzeichnis").is_some());
}

#[test]
fn alle_seiten_in_allen_themes() {
    let ziel = std::env::var_os("SPRECHWERK_BILDER").map(std::path::PathBuf::from);
    for thema in Thema::ALLE {
        for (name, seite) in [
            ("uebersicht", Seite::Uebersicht),
            ("mitglieder", Seite::Mitglieder),
            ("protokoll", Seite::Protokoll),
            ("einstellungen", Seite::Einstellungen),
        ] {
            let m = Modell {
                seite,
                thema,
                stand: Some(beispielstand()),
                zeilen: vec![
                    Zeile {
                        nr: 1,
                        text: "12:00:01  INFO sprechwerk startet name=Schachclub Nord port=7443"
                            .into(),
                    },
                    Zeile {
                        nr: 2,
                        text: "12:00:01  INFO lauscht adresse=0.0.0.0:7443".into(),
                    },
                ],
                ..Default::default()
            };
            let mut h = fenster(m);
            h.run();
            assert!(h.query_by_label("sprechwerk").is_some());
            if let Some(ziel) = &ziel {
                let bild = h.render().expect("zeichnen (wgpu)");
                std::fs::create_dir_all(ziel).unwrap();
                bild.save(ziel.join(format!("server-fenster-{name}-{}.png", thema.name())))
                    .unwrap();
            }
        }
    }
}

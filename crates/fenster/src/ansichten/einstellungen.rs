// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Einstellungen: Ich, Mikrofon, Wiedergabe, Sprechen, Chat, Darstellung.
//! Änderungen sind ein Entwurf im Modell; „Speichern" schreibt sie und wendet
//! sie auf den laufenden Sprechpfad an.

use super::{hauptknopf, karte, knopf, seitenkopf, text, Icon};
use crate::logik::{self, Sprechtaste};
use crate::modell::{Laden, Oberflaeche};
use crate::texte::{rolle, t, tf, Sprache};
use crate::thema::{self, Thema, Ton};
use eframe::egui::{self, PointerButton, RichText};
use sprechwerk_client::audio::Geraet;
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Verbindung;
use sprechwerk_oberflaeche::bausteine::{
    auswahl, fakten, feld, feldbreite, kopierbar, pegelregler, seite, zeile,
};

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    seite(ui, |ui| {
        seitenkopf(ui, o, &t(s, "set.title"), &t(s, "set.sub"));

        karte(ui, &f, &t(s, "set.me"), Some(&t(s, "set.me.sub")), |ui| {
            feld(
                ui,
                &f,
                &t(s, "set.name"),
                Some(&t(s, "set.name.hint")),
                |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut o.einstellungen.name)
                            .hint_text(t(s, "set.name.placeholder"))
                            .desired_width(feldbreite(ui)),
                    )
                },
            );
            if let Some(Verbindung::Verbunden {
                server,
                server_name,
                name,
                rolle: r,
                kennung,
                ..
            }) = o.zustand.verbindung.clone()
            {
                ui.add_space(thema::SPACE_2);
                fakten(
                    ui,
                    &f,
                    &[
                        (t(s, "set.fact.server"), server, true),
                        (t(s, "set.fact.name"), server_name, false),
                        (t(s, "set.fact.me"), name, false),
                        (t(s, "set.fact.role"), rolle(s, r), false),
                    ],
                );
                feld(
                    ui,
                    &f,
                    &t(s, "set.fact.key"),
                    Some(&t(s, "set.key_hint")),
                    |ui| {
                        ui.horizontal_top(|ui| {
                            kopierbar(
                                ui,
                                &f,
                                &logik::kennung_gruppiert(&kennung, 64),
                                &t(s, "srv.copy"),
                            )
                        })
                        .response
                    },
                );
            }
        });
        ui.add_space(thema::SPACE_4);

        let geraete = o.geraete.clone();
        karte(
            ui,
            &f,
            &t(s, "set.mic_talk"),
            Some(&t(s, "set.mic_talk.sub")),
            |ui| {
                let liste = match &geraete {
                    Laden::Fertig(l) => l.mikrofon.clone(),
                    _ => Vec::new(),
                };
                geraetewahl(
                    ui,
                    &f,
                    s,
                    "mikrofon",
                    &mut o.einstellungen.mikrofon,
                    &liste,
                    &geraete,
                );

                // Sprechtest: ohne Verbindung öffnet er das Mikrofon nur für den
                // Pegel; mit Verbindung läuft es ohnehin.
                zeile(ui, &f, &t(s, "set.mic_test"), None, |ui| {
                    if o.verbunden() {
                        ui.label(text(o, t(s, "set.mic_test.live"), Ton::Leise))
                    } else {
                        let (name, an) = if o.sprechtest {
                            (t(s, "set.mic_test.stop"), false)
                        } else {
                            (t(s, "set.mic_test.start"), true)
                        };
                        let a = knopf(ui, &f, Icon::Mikrofon, &name);
                        if a.clicked() {
                            aktionen.extend(o.sprechtest(an));
                        }
                        a
                    }
                });

                // Pegel und Schwelle in einem Balken; daneben das Sprechlicht.
                let ptt = o.einstellungen.push_to_talk;
                let pegel = if o.audio.laeuft && !o.audio.stumm {
                    o.audio.pegel
                } else {
                    0
                };
                let spricht =
                    logik::ich_spreche(&o.audio, o.einstellungen.schwelle, ptt, o.spricht);
                zeile(
                    ui,
                    &f,
                    &t(s, "set.level"),
                    Some(&t(s, "set.level.hint")),
                    |ui| {
                        ui.horizontal(|ui| {
                            // Rechts daneben stehen Sprechlicht und Schwellenwert:
                            // deren Platz geht ab, sonst wird die Karte breiter als
                            // das Fenster.
                            let breite = balkenbreite(ui);
                            let schwelle = (!ptt).then_some(&mut o.einstellungen.schwelle);
                            let a = pegelregler(
                                ui,
                                &f,
                                pegel,
                                schwelle,
                                breite,
                                &t(s, "set.threshold"),
                            );
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(thema::FONT_SIZE_MD, thema::FONT_SIZE_MD),
                                egui::Sense::hover(),
                            );
                            let quadrat = egui::Rect::from_center_size(
                                rect.center(),
                                egui::vec2(thema::SPACE_3, thema::SPACE_3),
                            );
                            if spricht {
                                ui.painter().rect_filled(quadrat, 0.0, f.status_ok);
                            } else {
                                ui.painter().rect_stroke(
                                    quadrat,
                                    0.0,
                                    egui::Stroke::new(thema::SIZE_BORDER, f.border_strong),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if !ptt {
                                ui.label(
                                    text(
                                        o,
                                        tf(s, "set.threshold.value", &[&o.einstellungen.schwelle]),
                                        Ton::Blass,
                                    )
                                    .small(),
                                );
                            }
                            a
                        })
                        .inner
                    },
                );
                ui.add_space(thema::SPACE_2);
                schalter(
                    ui,
                    &f,
                    &mut o.einstellungen.push_to_talk,
                    &t(s, "set.ptt"),
                    "",
                );
                if o.einstellungen.push_to_talk {
                    let warnung = (o.einstellungen.ptt_taste == "Leertaste")
                        .then(|| t(s, "set.key.space_warn"));
                    feld(
                        ui,
                        &f,
                        &t(s, "set.key"),
                        Some(&t(s, "set.key.hint")),
                        |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if o.taste_aufnehmen {
                                    ui.label(text(o, t(s, "set.key.press"), Ton::Akzent));
                                    if let Some(neu) = gedrueckte_taste(ui) {
                                        o.einstellungen.ptt_taste = neu.name();
                                        o.taste_aufnehmen = false;
                                    }
                                    if ui.button(t(s, "set.key.cancel")).clicked() {
                                        o.taste_aufnehmen = false;
                                    }
                                } else {
                                    ui.label(RichText::new(&o.einstellungen.ptt_taste).monospace());
                                    if ui.button(t(s, "set.key.change")).clicked() {
                                        o.taste_aufnehmen = true;
                                    }
                                }
                            })
                            .response
                        },
                    );
                    if let Some(w) = warnung {
                        ui.add(egui::Label::new(text(o, w, Ton::Warnung).small()).wrap());
                    }
                }
                feld(
                    ui,
                    &f,
                    &t(s, "set.lead.short"),
                    Some(&t(s, "set.lead.hint")),
                    |ui| {
                        // Gleich breit wie der Pegelbalken darüber.
                        ui.spacing_mut().slider_width = balkenbreite(ui);
                        ui.add(
                            egui::Slider::new(&mut o.einstellungen.vorlauf_ms, 0..=500)
                                .suffix(" ms"),
                        )
                    },
                );

                ui.add_space(thema::SPACE_2);
                schalter(
                    ui,
                    &f,
                    &mut o.einstellungen.rauschunterdrueckung,
                    &t(s, "set.denoise"),
                    &t(s, "set.denoise.hint"),
                );
                schalter(
                    ui,
                    &f,
                    &mut o.einstellungen.noisegate,
                    &t(s, "set.noisegate"),
                    &t(s, "set.noisegate.hint"),
                );
                schalter(
                    ui,
                    &f,
                    &mut o.einstellungen.echounterdrueckung,
                    &t(s, "set.aec"),
                    &t(s, "set.aec.hint"),
                );
            },
        );
        ui.add_space(thema::SPACE_4);

        karte(
            ui,
            &f,
            &t(s, "set.speaker"),
            Some(&t(s, "set.speaker.sub")),
            |ui| {
                let liste = match &geraete {
                    Laden::Fertig(l) => l.lautsprecher.clone(),
                    _ => Vec::new(),
                };
                geraetewahl(
                    ui,
                    &f,
                    s,
                    "lautsprecher",
                    &mut o.einstellungen.lautsprecher,
                    &liste,
                    &geraete,
                );
                if knopf(ui, &f, Icon::Lautsprecher, &t(s, "set.speaker_test")).clicked() {
                    aktionen.push(Aktion::Testton {
                        geraet: o.einstellungen.lautsprecher.clone(),
                    });
                }
            },
        );
        ui.add_space(thema::SPACE_4);
        karte(ui, &f, &t(s, "set.chat"), None, |ui| {
            schalter(
                ui,
                &f,
                &mut o.einstellungen.link_vorschau,
                &t(s, "set.link_preview"),
                &t(s, "set.link_preview.hint"),
            );
        });
        ui.add_space(thema::SPACE_4);

        karte(ui, &f, &t(s, "set.appearance"), None, |ui| {
            let mut thema_wahl = o.thema();
            feld(ui, &f, &t(s, "set.scheme"), None, |ui| {
                let moeglich: Vec<(Thema, String)> = Thema::ALLE
                    .iter()
                    .map(|th| (*th, t(s, th.text_schluessel())))
                    .collect();
                ui.horizontal_wrapped(|ui| auswahl(ui, &mut thema_wahl, &moeglich))
                    .response
            });
            o.einstellungen.thema = thema_wahl.name().into();
            let mut sprache = s;
            feld(ui, &f, &t(s, "set.language"), None, |ui| {
                let moeglich: Vec<(Sprache, String)> = Sprache::ALLE
                    .iter()
                    .map(|sp| (*sp, sp.anzeige().to_string()))
                    .collect();
                ui.horizontal_wrapped(|ui| auswahl(ui, &mut sprache, &moeglich))
                    .response
            });
            o.einstellungen.sprache = sprache.code().into();
        });
        ui.add_space(thema::SPACE_4);
        if hauptknopf(ui, &f, &t(s, "set.save")).clicked() {
            aktionen.push(o.speichern());
        }
    });
}

/// Breite von Pegelbalken und Vorlauf-Regler: gleich breit, mit Platz
/// rechts für Licht und Wert bzw. das Zahlenfeld.
fn balkenbreite(ui: &egui::Ui) -> f32 {
    (feldbreite(ui) - thema::SPACE_8 * 2.0).max(thema::SPACE_8)
}

/// Häkchen mit Erklärung darunter (umbrechend).
fn schalter(ui: &mut egui::Ui, f: &thema::Farben, wert: &mut bool, titel: &str, hinweis: &str) {
    ui.checkbox(wert, titel);
    if !hinweis.is_empty() {
        ui.horizontal_top(|ui| {
            ui.add_space(thema::SPACE_5);
            ui.add(
                egui::Label::new(
                    sprechwerk_oberflaeche::bausteine::text(f, hinweis, Ton::Blass).small(),
                )
                .wrap(),
            );
        });
    }
    ui.add_space(thema::SPACE_1);
}

fn geraetewahl(
    ui: &mut egui::Ui,
    f: &thema::Farben,
    s: Sprache,
    id: &str,
    wahl: &mut String,
    liste: &[Geraet],
    laden: &Laden<sprechwerk_client::audio::Geraeteliste>,
) {
    let anzeige = |g: &Geraet| {
        if g.vorgabe {
            tf(s, "set.default_device_marked", &[&g.name])
        } else {
            g.name.clone()
        }
    };
    let gewaehlt = if wahl.is_empty() {
        t(s, "set.default_device")
    } else {
        liste
            .iter()
            .find(|g| g.id == *wahl)
            .map(anzeige)
            .unwrap_or_else(|| tf(s, "set.device_missing", &[wahl]))
    };
    let fehler = match laden {
        Laden::Fehler(x) => Some(x.clone()),
        _ => None,
    };
    feld(ui, f, &t(s, "set.device"), fehler.as_deref(), |ui| {
        ui.horizontal(|ui| {
            let breite = feldbreite(ui);
            let a = egui::ComboBox::from_id_salt(id)
                .selected_text(gewaehlt)
                .truncate()
                .width(breite)
                .show_ui(ui, |ui| {
                    ui.selectable_value(wahl, String::new(), t(s, "set.default_device"));
                    for g in liste {
                        ui.selectable_value(wahl, g.id.clone(), anzeige(g));
                    }
                })
                .response;
            if matches!(laden, Laden::Laedt) {
                ui.spinner();
            }
            a
        })
        .inner
    });
}

/// Die nächste gedrückte Taste (oder Maus 4/5) für die Sprechtaste.
fn gedrueckte_taste(ui: &egui::Ui) -> Option<Sprechtaste> {
    ui.input(|i| {
        if i.pointer.button_pressed(PointerButton::Extra1) {
            return Some(Sprechtaste::Maus4);
        }
        if i.pointer.button_pressed(PointerButton::Extra2) {
            return Some(Sprechtaste::Maus5);
        }
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key, pressed: true, ..
            } if *key != egui::Key::Escape => Some(Sprechtaste::Taste(*key)),
            _ => None,
        })
    })
}

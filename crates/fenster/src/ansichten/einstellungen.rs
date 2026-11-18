// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Einstellungen: Ich, Mikrofon, Wiedergabe, Sprechen, Chat, Darstellung.
//! Änderungen sind ein Entwurf im Modell; „Speichern" schreibt sie und wendet
//! sie auf den laufenden Sprechpfad an.

use super::{hauptknopf, karte, knopf, pegelbalken, seitenkopf, text, Icon};
use crate::logik::{self, Sprechtaste};
use crate::modell::{Laden, Oberflaeche};
use crate::texte::{rolle, t, tf, Sprache};
use crate::thema::{self, Thema, Ton};
use eframe::egui::{self, PointerButton, RichText};
use sprechwerk_client::audio::Geraet;
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Verbindung;

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_max_width(thema::SPACE_8 * 12.0);
        ui.add_space(thema::SPACE_2);
        seitenkopf(ui, o, &t(s, "set.title"), &t(s, "set.sub"));

        karte(ui, &f, &t(s, "set.me"), Some(&t(s, "set.me.sub")), |ui| {
            ui.label(t(s, "set.name"));
            ui.add(
                egui::TextEdit::singleline(&mut o.einstellungen.name)
                    .hint_text(t(s, "set.name.placeholder")),
            );
            ui.label(text(o, t(s, "set.name.hint"), Ton::Blass).small());
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
                egui::Grid::new("fakten").num_columns(2).show(ui, |ui| {
                    for (k, v) in [
                        ("set.fact.server", server),
                        ("set.fact.name", server_name),
                        ("set.fact.me", name),
                        ("set.fact.role", rolle(s, r)),
                    ] {
                        ui.label(text(o, t(s, k), Ton::Leise));
                        ui.label(v);
                        ui.end_row();
                    }
                    ui.label(text(o, t(s, "set.fact.key"), Ton::Leise));
                    ui.label(RichText::new(logik::kennung_gruppiert(&kennung, 64)).monospace())
                        .on_hover_text(t(s, "set.key_hint"));
                    ui.end_row();
                });
            }
        });
        ui.add_space(thema::SPACE_4);

        let geraete = o.geraete.clone();
        karte(ui, &f, &t(s, "set.mic"), Some(&t(s, "set.mic.sub")), |ui| {
            let liste = match &geraete {
                Laden::Fertig(l) => l.mikrofon.clone(),
                _ => Vec::new(),
            };
            geraetewahl(
                ui,
                s,
                "mikrofon",
                &mut o.einstellungen.mikrofon,
                &liste,
                &geraete,
            );
            ui.horizontal(|ui| {
                if knopf(ui, &f, Icon::Mikrofon, &t(s, "set.mic_test")).clicked() {
                    o.pegeltest = Laden::Laedt;
                    aktionen.push(Aktion::Pegel {
                        geraet: o.einstellungen.mikrofon.clone(),
                        millisekunden: 1500,
                    });
                }
                let pegel = match o.pegeltest {
                    Laden::Fertig(p) => p,
                    _ if o.audio.laeuft => o.audio.pegel,
                    _ => 0,
                };
                ui.label(text(o, t(s, "set.level"), Ton::Leise));
                pegelbalken(
                    ui,
                    &f,
                    pegel,
                    Some(o.einstellungen.schwelle),
                    thema::SPACE_8 * 3.0,
                );
            });
            ui.label(text(o, t(s, "set.mic.hint"), Ton::Blass).small());
            umschalter(ui, o, "set.denoise", "set.denoise.hint", |e| {
                &mut e.rauschunterdrueckung
            });
            umschalter(ui, o, "set.noisegate", "set.noisegate.hint", |e| {
                &mut e.noisegate
            });
            umschalter(ui, o, "set.aec", "set.aec.hint", |e| {
                &mut e.echounterdrueckung
            });
        });
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

        karte(
            ui,
            &f,
            &t(s, "set.talk"),
            Some(&t(s, "set.talk.sub")),
            |ui| {
                ui.checkbox(&mut o.einstellungen.push_to_talk, t(s, "set.ptt"));
                if o.einstellungen.push_to_talk {
                    ui.horizontal(|ui| {
                        ui.label(t(s, "set.key"));
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
                    });
                    ui.label(text(o, t(s, "set.key.hint"), Ton::Blass).small());
                    if o.einstellungen.ptt_taste == "Leertaste" {
                        ui.label(text(o, t(s, "set.key.space_warn"), Ton::Warnung).small());
                    }
                } else {
                    ui.add(
                        egui::Slider::new(&mut o.einstellungen.schwelle, 0..=100)
                            .text(t(s, "set.threshold")),
                    );
                }
                ui.add(
                    egui::Slider::new(&mut o.einstellungen.vorlauf_ms, 0..=500)
                        .suffix(" ms")
                        .text(t(s, "set.lead")),
                );
                ui.label(text(o, t(s, "set.lead.hint"), Ton::Blass).small());
            },
        );
        ui.add_space(thema::SPACE_4);

        karte(ui, &f, &t(s, "set.chat"), None, |ui| {
            umschalter(ui, o, "set.link_preview", "set.link_preview.hint", |e| {
                &mut e.link_vorschau
            });
        });
        ui.add_space(thema::SPACE_4);

        karte(ui, &f, &t(s, "set.appearance"), None, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(s, "set.scheme"));
                for th in Thema::ALLE {
                    if ui
                        .selectable_label(o.thema() == th, t(s, th.text_schluessel()))
                        .clicked()
                    {
                        o.einstellungen.thema = th.name().into();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(t(s, "set.language"));
                for sp in Sprache::ALLE {
                    if ui.selectable_label(s == sp, sp.anzeige()).clicked() {
                        o.einstellungen.sprache = sp.code().into();
                    }
                }
            });
        });
        ui.add_space(thema::SPACE_4);
        if hauptknopf(ui, &f, &t(s, "set.save")).clicked() {
            aktionen.push(o.speichern());
        }
        ui.add_space(thema::SPACE_6);
    });
}

fn umschalter(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    titel: &str,
    hinweis: &str,
    feld: impl FnOnce(&mut sprechwerk_client::einstellungen::Einstellungen) -> &mut bool,
) {
    let s = o.sprache();
    let h = text(o, t(s, hinweis), Ton::Blass).small();
    ui.checkbox(feld(&mut o.einstellungen), t(s, titel));
    ui.label(h);
}

fn geraetewahl(
    ui: &mut egui::Ui,
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
    ui.horizontal(|ui| {
        let beschriftung = ui.label(t(s, "set.device"));
        egui::ComboBox::from_id_salt(id)
            .selected_text(gewaehlt)
            .width(thema::SPACE_8 * 5.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(wahl, String::new(), t(s, "set.default_device"));
                for g in liste {
                    ui.selectable_value(wahl, g.id.clone(), anzeige(g));
                }
            })
            .response
            .labelled_by(beschriftung.id);
        match laden {
            Laden::Laedt => {
                ui.spinner();
            }
            Laden::Fehler(x) => {
                ui.label(RichText::new(x).small());
            }
            _ => {}
        }
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

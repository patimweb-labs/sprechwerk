// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Serverliste: gemerkte Server, ein Klick verbindet. Dazu der eigene Name
//! und — wenn verbunden — der Fingerabdruck des Zertifikats (TOFU).

use super::{hauptknopf_mit, karte, knopf, leer, seitenkopf, text, Icon};
use crate::logik;
use crate::modell::{Dialog, Oberflaeche};
use crate::texte::t;
use crate::thema::{self, Ton};
use eframe::egui::{self, RichText};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Verbindung;

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_max_width(thema::SPACE_8 * 12.0);
        ui.add_space(thema::SPACE_2);
        seitenkopf(ui, o, &t(s, "server.title"), &t(s, "server.sub"));

        let verbindet = matches!(o.zustand.verbindung, Some(Verbindung::Verbindet { .. }));
        let aktuell = match &o.zustand.verbindung {
            Some(Verbindung::Verbunden { server, .. }) => Some(server.clone()),
            _ => None,
        };

        karte(
            ui,
            &f,
            &t(s, "set.me"),
            Some(&t(s, "set.name.hint")),
            |ui| {
                egui::Grid::new("ich")
                    .num_columns(2)
                    .spacing([thema::SPACE_4, thema::SPACE_2])
                    .show(ui, |ui| {
                        let beschriftung = ui.label(t(s, "set.name"));
                        let name = ui
                            .add(
                                egui::TextEdit::singleline(&mut o.einstellungen.name)
                                    .hint_text(t(s, "set.name.placeholder"))
                                    .desired_width(thema::SPACE_8 * 4.0),
                            )
                            .labelled_by(beschriftung.id);
                        if name.lost_focus() {
                            aktionen.push(o.speichern());
                        }
                        ui.end_row();
                        let beschriftung = ui.label(t(s, "server.password"));
                        ui.add(
                            egui::TextEdit::singleline(&mut o.einstellungen.passwort)
                                .password(true)
                                .desired_width(thema::SPACE_8 * 4.0),
                        )
                        .labelled_by(beschriftung.id);
                        ui.end_row();
                    });
            },
        );
        ui.add_space(thema::SPACE_4);

        let name_fehlt = o.einstellungen.name.trim().is_empty();
        let favoriten = o.einstellungen.favoriten.clone();
        karte(ui, &f, &t(s, "app.menu_servers"), None, |ui| {
            if favoriten.is_empty() {
                leer(ui, o, &t(s, "server.add"), &t(s, "server.empty"));
            }
            egui::Grid::new("favoriten")
                .num_columns(4)
                .striped(true)
                .spacing([thema::SPACE_4, thema::SPACE_2])
                .show(ui, |ui| {
                    for fav in &favoriten {
                        ui.label(
                            RichText::new(&fav.name)
                                .font(egui::FontId::new(thema::FONT_SIZE_MD, thema::fett())),
                        );
                        ui.label(RichText::new(&fav.server).monospace());
                        let jetzt = jetzt_ms();
                        ui.label(text(
                            o,
                            logik::zuletzt_text(s, fav.zuletzt, jetzt).unwrap_or_default(),
                            Ton::Blass,
                        ));
                        ui.horizontal(|ui| {
                            let hier = aktuell.as_deref() == Some(fav.server.as_str());
                            let knopf = if verbindet {
                                t(s, "server.connecting")
                            } else {
                                t(s, "server.connect")
                            };
                            let frei = !verbindet && !hier && !name_fehlt;
                            if ui
                                .add_enabled_ui(frei, |ui| {
                                    hauptknopf_mit(ui, &f, Icon::Stecker, &knopf)
                                })
                                .inner
                                .clicked()
                            {
                                aktionen.push(o.verbinden(&fav.server));
                            }
                            ui.menu_image_button(
                                Icon::Mehr.bild(thema::FONT_SIZE_LG, f.text_muted),
                                |ui| {
                                    if ui.button(t(s, "server.remove")).clicked() {
                                        o.dialog = Dialog::ServerEntfernen {
                                            server: fav.server.clone(),
                                            name: fav.name.clone(),
                                        };
                                        ui.close();
                                    }
                                    if o.einstellungen.vertrauen_fuer(&fav.server).is_some()
                                        && ui.button(t(s, "server.certificate_forget")).clicked()
                                    {
                                        aktionen.push(Aktion::ZertifikatVergessen {
                                            server: fav.server.clone(),
                                        });
                                        o.einstellungen.vergiss_vertrauen(&fav.server);
                                        ui.close();
                                    }
                                },
                            )
                            .response
                            .on_hover_text(crate::texte::tf(s, "server.menu_for", &[&fav.name]));
                        });
                        ui.end_row();
                    }
                });
            ui.add_space(thema::SPACE_2);
            if knopf(ui, &f, Icon::Plus, &t(s, "server.add")).clicked() {
                o.dialog = Dialog::ServerHinzufuegen {
                    name: String::new(),
                    adresse: String::new(),
                };
            }
        });

        if let Some(fp) = o.zustand.zertifikat.clone().filter(|_| aktuell.is_some()) {
            ui.add_space(thema::SPACE_4);
            karte(
                ui,
                &f,
                &t(s, "server.certificate"),
                Some(&t(s, "server.certificate_hint")),
                |ui| {
                    ui.label(RichText::new(logik::fingerabdruck_gruppiert(&fp)).monospace());
                },
            );
        }
        ui.add_space(thema::SPACE_4);
        ui.label(text(o, t(s, "server.footnote"), Ton::Blass));
    });
}

fn jetzt_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

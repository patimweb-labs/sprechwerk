// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Serverliste: gemerkte Server, ein Klick verbindet. Dazu der eigene Name
//! und — wenn verbunden — der Fingerabdruck des Zertifikats (TOFU).

use super::{hauptknopf_mit, karte, knopf, leer, seitenkopf, text, Icon};
use crate::logik;
use crate::modell::{Dialog, Oberflaeche};
use crate::texte::{t, tf};
use crate::thema::{self, Ton};
use eframe::egui::{self, RichText};
use sprechwerk_client::einstellungen::Favorit;
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Verbindung;
use sprechwerk_oberflaeche::bausteine::{feld, feldbreite, kopierbar, seite, status};

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    seite(ui, |ui| {
        seitenkopf(ui, o, &t(s, "server.title"), &t(s, "server.sub"));

        let aktuell = match &o.zustand.verbindung {
            Some(Verbindung::Verbunden { server, .. }) => Some(server.clone()),
            _ => None,
        };

        karte(ui, &f, &t(s, "set.me"), None, |ui| {
            let name = feld(
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
            if name.lost_focus() {
                aktionen.push(o.speichern());
            }
            feld(ui, &f, &t(s, "server.password"), None, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut o.einstellungen.passwort)
                        .password(true)
                        .desired_width(feldbreite(ui)),
                )
            });
        });
        ui.add_space(thema::SPACE_4);

        let favoriten = o.einstellungen.favoriten.clone();
        karte(ui, &f, &t(s, "app.menu_servers"), None, |ui| {
            if favoriten.is_empty() {
                leer(ui, o, &t(s, "server.add"), &t(s, "server.empty"));
            }
            for (i, fav) in favoriten.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                eintrag(ui, o, fav, aktuell.as_deref(), aktionen);
            }
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
                    kopierbar(
                        ui,
                        &f,
                        &logik::fingerabdruck_gruppiert(&fp),
                        &t(s, "srv.copy"),
                    )
                },
            );
        }
        ui.add_space(thema::SPACE_4);
        ui.add(egui::Label::new(text(o, t(s, "server.footnote"), Ton::Blass)).wrap());
    });
}

/// Ein gemerkter Server: links Name, Adresse, zuletzt (gekürzt, wenn der
/// Platz fehlt), rechts Verbinden bzw. der Stand und das Menü.
fn eintrag(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    fav: &Favorit,
    aktuell: Option<&str>,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let verbindet = matches!(o.zustand.verbindung, Some(Verbindung::Verbindet { .. }));
    let hier = aktuell == Some(fav.server.as_str());
    let frei = !verbindet && !o.einstellungen.name.trim().is_empty();
    let zuletzt = logik::zuletzt_text(s, fav.zuletzt, jetzt_ms()).unwrap_or_default();
    let mut verbinden = false;
    let mut entfernen = false;
    let mut vergessen = false;
    let angeheftet = o.einstellungen.vertrauen_fuer(&fav.server).is_some();
    egui::Sides::new()
        .height(thema::SIZE_CONTROL_LG)
        .shrink_left()
        .truncate()
        .show(
            ui,
            |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(
                        RichText::new(&fav.name)
                            .font(egui::FontId::new(thema::FONT_SIZE_MD, thema::fett())),
                    );
                    let unter = if zuletzt.is_empty() {
                        fav.server.clone()
                    } else {
                        format!("{} · {zuletzt}", fav.server)
                    };
                    ui.label(text(o, unter, Ton::Blass).small());
                });
            },
            |ui| {
                ui.menu_image_button(Icon::Mehr.bild(thema::FONT_SIZE_LG, f.text_muted), |ui| {
                    if ui.button(t(s, "server.remove")).clicked() {
                        entfernen = true;
                        ui.close();
                    }
                    if angeheftet && ui.button(t(s, "server.certificate_forget")).clicked() {
                        vergessen = true;
                        ui.close();
                    }
                })
                .response
                .on_hover_text(tf(s, "server.menu_for", &[&fav.name]));
                if hier {
                    // Schon da: kein ausgegrauter Knopf, sondern der Stand.
                    status(ui, &f, Ton::Gut, &t(s, "server.connected"));
                } else {
                    let beschriftung = if verbindet {
                        t(s, "server.connecting")
                    } else {
                        t(s, "server.connect")
                    };
                    verbinden = ui
                        .add_enabled_ui(frei, |ui| {
                            hauptknopf_mit(ui, &f, Icon::Stecker, &beschriftung)
                        })
                        .inner
                        .on_disabled_hover_text(t(s, "set.name.hint"))
                        .clicked();
                }
            },
        );
    if verbinden {
        aktionen.push(o.verbinden(&fav.server));
    }
    if entfernen {
        o.dialog = Dialog::ServerEntfernen {
            server: fav.server.clone(),
            name: fav.name.clone(),
        };
    }
    if vergessen {
        aktionen.push(Aktion::ZertifikatVergessen {
            server: fav.server.clone(),
        });
        o.einstellungen.vergiss_vertrauen(&fav.server);
    }
}

fn jetzt_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

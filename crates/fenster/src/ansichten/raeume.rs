// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Räume und Chat: links die Räume mit ihren Leuten, rechts der Verlauf des
//! eigenen Raums. Eingeklappt wird der Chat zur schmalen Schiene am rechten
//! Rand, und die Raumliste bekommt die Fläche (gemerkt in den Einstellungen). Eingreifen (stumm, verschieben, sperren) über das Menü am
//! Namen — es erscheint nur, wenn die Rolle es erlaubt (AGENTS.md §2.3).

use super::{icon, icon_knopf, kuerzel, leer, text, Icon};
use crate::app::Zwischenspeicher;
use crate::logik::{self, Stueck, REAKTIONEN};
use crate::modell::{Dialog, Oberflaeche};
use crate::texte::{rolle, t, tf, Sprache};
use crate::thema::{self, fett, Ton};
use base64::Engine;
use eframe::egui::{self, RichText, Stroke};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::{
    Befehl, Linkentwurf, Linkvorschau, Mitglied, Nachricht, NachrichtInhalt, Textschluessel,
};

pub fn zeigen(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let f = *o.thema().farben();
    let s = o.sprache();
    let leiste = egui::Frame::new()
        .fill(f.bg_panel)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_3 as i8));

    if o.einstellungen.chat_eingeklappt {
        // Schmale Schiene rechts: ein Knopf holt den Chat zurück.
        egui::Panel::right("chatschiene")
            .resizable(false)
            .exact_size(thema::SIZE_CONTROL_LG + thema::SPACE_2 * 2.0)
            .frame(
                egui::Frame::new()
                    .fill(f.bg_panel)
                    .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                    .inner_margin(egui::Margin::same(thema::SPACE_2 as i8)),
            )
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if icon_knopf(ui, &f, Icon::Sprechblase, &t(s, "chat.expand"), false).clicked()
                    {
                        o.einstellungen.chat_eingeklappt = false;
                        aktionen.push(o.speichern());
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(leiste)
            .show(ui, |ui| raumliste(ui, o, aktionen));
        return;
    }

    egui::Panel::left("raeume")
        .resizable(true)
        .default_size(thema::SPACE_8 * 4.5)
        .min_size(thema::SPACE_8 * 3.0)
        .frame(leiste)
        .show(ui, |ui| raumliste(ui, o, aktionen));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(f.bg_canvas))
        .show(ui, |ui| chat(ui, o, speicher, aktionen));
}

fn raumliste(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t(s, "app.rooms")).font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
        );
        if o.zustand.darf_raeume_verwalten() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_knopf(ui, &f, Icon::Plus, &t(s, "room.add"), false).clicked() {
                    o.dialog = Dialog::RaumAnlegen {
                        name: String::new(),
                    };
                }
            });
        }
    });
    ui.separator();
    let raeume = o.zustand.raeume.clone();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for raum in &raeume {
            let zahl = logik::anwesende(&o.zustand, raum.id).count();
            let zahl = if zahl > 0 {
                zahl.to_string()
            } else {
                String::new()
            };
            let schloss = Icon::Schloss.bild(
                thema::FONT_SIZE_SM,
                if raum.hier {
                    f.accent_default
                } else {
                    f.text_faint
                },
            );
            let knopf = if raum.verschluesselt {
                egui::Button::image_and_text(schloss, raum.name.as_str())
            } else {
                egui::Button::new(raum.name.as_str())
            };
            let antwort = ui.add(
                knopf
                    .selected(raum.hier)
                    .frame_when_inactive(raum.hier)
                    .right_text(zahl)
                    .min_size(egui::vec2(ui.available_width(), thema::SIZE_CONTROL_SM)),
            );
            let antwort = if raum.verschluesselt {
                antwort.on_hover_text(t(s, "room.encrypted"))
            } else {
                antwort
            };
            if antwort.clicked() && !raum.hier {
                aktionen.push(Aktion::Befehl(Befehl::Betreten { raum: raum.id }));
            }
            if o.zustand.darf_raeume_verwalten() {
                antwort.context_menu(|ui| {
                    if ui.button(t(s, "room.enter")).clicked() {
                        aktionen.push(Aktion::Befehl(Befehl::Betreten { raum: raum.id }));
                        ui.close();
                    }
                    if ui.button(t(s, "room.rename")).clicked() {
                        o.dialog = Dialog::RaumUmbenennen {
                            raum: raum.id,
                            name: raum.name.clone(),
                        };
                        ui.close();
                    }
                    if ui.button(t(s, "room.delete")).clicked() {
                        o.dialog = Dialog::RaumLoeschen {
                            raum: raum.id,
                            name: raum.name.clone(),
                        };
                        ui.close();
                    }
                });
            }
            if raum.hier && logik::schluessel_vorhanden(&o.zustand, raum.id) == Some(false) {
                ui.horizontal_wrapped(|ui| {
                    icon(ui, &f, Icon::Warnung, Ton::Warnung);
                    ui.label(text(o, t(s, "room.key_missing"), Ton::Warnung));
                });
                if ui.small_button(t(s, "room.key_request")).clicked() {
                    aktionen.push(Aktion::Befehl(Befehl::SchluesselAnfordern {
                        raum: raum.id,
                    }));
                }
            }
            let leute: Vec<Mitglied> = logik::anwesende(&o.zustand, raum.id).cloned().collect();
            for m in &leute {
                ui.horizontal(|ui| {
                    ui.add_space(thema::SPACE_3);
                    kuerzel(ui, &f, &m.name, m.ich, m.spricht);
                    let mut zeile = m.name.clone();
                    if m.ich {
                        zeile.push_str(&format!(" ({})", t(s, "chat.you")));
                    }
                    let mut unter = rolle(s, m.rolle);
                    if m.stumm {
                        unter.push_str(&format!(" · {}", t(s, "state.muted")));
                    }
                    if m.taub {
                        unter.push_str(&format!(" · {}", t(s, "state.deaf")));
                    }
                    let a = ui
                        .vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(zeile);
                            ui.label(text(o, unter, Ton::Blass).small());
                        })
                        .response
                        .interact(egui::Sense::click());
                    if !m.ich && logik::darf_eingreifen(&o.zustand) {
                        a.context_menu(|ui| mitgliedmenue(ui, o, m, aktionen));
                    }
                });
            }
            ui.add_space(thema::SPACE_1);
        }
    });
}

fn mitgliedmenue(ui: &mut egui::Ui, o: &mut Oberflaeche, m: &Mitglied, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let befehl = |b: Befehl| Aktion::Befehl(b);
    let mik = if m.stumm {
        "member.unmute"
    } else {
        "member.mute"
    };
    if ui.button(t(s, mik)).clicked() {
        aktionen.push(befehl(Befehl::Stummschalten {
            mitglied: m.id,
            taub: false,
            an: !m.stumm,
        }));
        ui.close();
    }
    let ton = if m.taub {
        "member.undeaf"
    } else {
        "member.deaf"
    };
    if ui.button(t(s, ton)).clicked() {
        aktionen.push(befehl(Befehl::Stummschalten {
            mitglied: m.id,
            taub: true,
            an: !m.taub,
        }));
        ui.close();
    }
    ui.separator();
    if ui.button(t(s, "member.move")).clicked() {
        o.dialog = Dialog::Verschieben {
            mitglied: m.id,
            name: m.name.clone(),
            ziel: None,
        };
        ui.close();
    }
    if ui.button(t(s, "member.kick_room")).clicked() {
        aktionen.push(befehl(Befehl::AusRaumWerfen { mitglied: m.id }));
        ui.close();
    }
    if o.zustand.darf_rollen_vergeben() && ui.button(t(s, "member.role")).clicked() {
        o.dialog = Dialog::Rolle {
            mitglied: m.id,
            name: m.name.clone(),
            rolle: m.rolle,
        };
        ui.close();
    }
    ui.separator();
    if ui.button(t(s, "member.kick")).clicked() {
        o.dialog = Dialog::Trennen {
            mitglied: m.id,
            name: m.name.clone(),
            grund: String::new(),
        };
        ui.close();
    }
    if logik::darf_sperren(&o.zustand) && ui.button(t(s, "member.ban")).clicked() {
        o.dialog = Dialog::Sperren {
            mitglied: m.id,
            name: m.name.clone(),
            grund: String::new(),
        };
        ui.close();
    }
}

fn chat(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let Some(raum) = o.zustand.raeume.iter().find(|r| r.hier).cloned() else {
        leer(ui, o, &t(s, "room.none"), &t(s, "room.choose"));
        return;
    };

    // Kopf
    egui::Panel::top("chatkopf")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::symmetric(
                    thema::SPACE_4 as i8,
                    thema::SPACE_2 as i8,
                )),
        )
        .show(ui, |ui| {
            // Der Hinweis links wird gekürzt, der Knopf rechts bleibt immer da.
            let mut einklappen = false;
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_SM)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.label(
                            RichText::new(&raum.name)
                                .font(egui::FontId::new(thema::FONT_SIZE_LG, fett())),
                        );
                        icon(ui, &f, Icon::Schloss, Ton::Blass).on_hover_text(t(s, "chat.e2e"));
                        ui.label(text(o, t(s, "chat.e2e"), Ton::Blass).small());
                    },
                    |ui| {
                        einklappen =
                            icon_knopf(ui, &f, Icon::PanelZu, &t(s, "chat.collapse"), false)
                                .clicked();
                    },
                );
            if einklappen {
                o.einstellungen.chat_eingeklappt = true;
                aktionen.push(o.speichern());
            }
            if o.zustand.textschluessel == Textschluessel::Wartet {
                ui.label(text(o, t(s, "chat.key_wait"), Ton::Warnung));
            }
        });

    // Eingabe
    egui::Panel::bottom("eingabe")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::same(thema::SPACE_3 as i8)),
        )
        .show(ui, |ui| eingabe(ui, o, speicher, aktionen));

    // Verlauf
    egui::CentralPanel::default()
        .frame(egui::Frame::new().inner_margin(egui::Margin::same(thema::SPACE_4 as i8)))
        .show(ui, |ui| {
            let nachrichten: Vec<Nachricht> =
                logik::sichtbar(&o.zustand.nachrichten).cloned().collect();
            if nachrichten.is_empty() {
                leer(ui, o, &t(s, "room.empty.title"), &t(s, "room.empty.desc"));
                return;
            }
            let versatz = ortszeit_versatz();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for n in &nachrichten {
                        beitrag(ui, o, s, n, versatz, speicher, aktionen);
                        ui.add_space(thema::SPACE_2);
                    }
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn beitrag(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    s: Sprache,
    n: &Nachricht,
    versatz: i64,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let f = *o.thema().farben();
    let ich = Some(n.verfasser) == o.zustand.ich();
    let antwort = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = thema::SPACE_1;
            ui.horizontal(|ui| {
                let name = if ich {
                    format!("{} ({})", n.verfasser_name, t(s, "chat.you"))
                } else {
                    n.verfasser_name.clone()
                };
                let farbe = if ich {
                    f.accent_default
                } else {
                    f.text_default
                };
                ui.label(
                    RichText::new(name)
                        .font(egui::FontId::new(thema::FONT_SIZE_MD, fett()))
                        .color(farbe),
                );
                ui.label(text(o, logik::uhrzeit(n.zeit, versatz), Ton::Blass).small());
            });
            match &n.inhalt {
                NachrichtInhalt::Text {
                    text: inhalt,
                    vorschau,
                } => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for stueck in logik::teile(inhalt) {
                            match stueck {
                                Stueck::Text(x) => {
                                    ui.label(x);
                                }
                                Stueck::Link(url) => {
                                    if ui.link(&url).clicked() {
                                        oeffne_link(ui.ctx(), &url);
                                    }
                                }
                            }
                        }
                    });
                    if let Some(v) = vorschau {
                        vorschaukarte(ui, o, v, speicher, None);
                    }
                }
                NachrichtInhalt::Wartet => {
                    ui.label(text(o, t(s, "chat.waiting"), Ton::Blass).italics());
                }
                NachrichtInhalt::Unlesbar => {
                    ui.label(text(o, t(s, "chat.unreadable"), Ton::Warnung));
                }
                NachrichtInhalt::Neuer { art } => {
                    ui.label(text(o, tf(s, "chat.newer", &[art]), Ton::Blass));
                }
                NachrichtInhalt::Geloescht => {
                    ui.label(text(o, t(s, "chat.deleted"), Ton::Blass).italics());
                }
                NachrichtInhalt::Reaktion { .. } => {}
            }
            if !n.reaktionen.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for r in &n.reaktionen {
                        let beschriftung = format!("{} {}", r.zeichen, r.anzahl);
                        let hinweis = tf(
                            s,
                            "react.count",
                            &[
                                &t(s, &format!("react.{}", r.zeichen)),
                                &r.namen.join(", "),
                                &r.anzahl,
                            ],
                        );
                        if ui
                            .selectable_label(r.ich, beschriftung)
                            .on_hover_text(hinweis)
                            .clicked()
                        {
                            aktionen.push(Aktion::Befehl(Befehl::Reagieren {
                                nachricht: n.id,
                                zeichen: r.zeichen.clone(),
                            }));
                        }
                    }
                });
            }
        })
        .response
        .interact(egui::Sense::click());

    let lesbar = matches!(n.inhalt, NachrichtInhalt::Text { .. });
    if lesbar || n.loeschbar {
        antwort.context_menu(|ui| {
            if lesbar {
                ui.horizontal(|ui| {
                    for z in REAKTIONEN {
                        if ui
                            .button(z)
                            .on_hover_text(t(s, &format!("react.{z}")))
                            .clicked()
                        {
                            aktionen.push(Aktion::Befehl(Befehl::Reagieren {
                                nachricht: n.id,
                                zeichen: z.to_string(),
                            }));
                            ui.close();
                        }
                    }
                });
                if let NachrichtInhalt::Text { text, .. } = &n.inhalt {
                    if ui
                        .add(egui::Button::image_and_text(
                            Icon::Kopieren.bild(thema::FONT_SIZE_MD, f.text_muted),
                            t(s, "chat.copy"),
                        ))
                        .clicked()
                    {
                        ui.ctx().copy_text(text.clone());
                        ui.close();
                    }
                }
            }
            if n.loeschbar
                && !n.geloescht
                && ui
                    .add(egui::Button::image_and_text(
                        Icon::Papierkorb.bild(thema::FONT_SIZE_MD, f.status_error),
                        t(s, "chat.delete"),
                    ))
                    .clicked()
            {
                aktionen.push(Aktion::Befehl(Befehl::NachrichtLoeschen {
                    nachricht: n.id,
                }));
                ui.close();
            }
        });
    }
}

fn eingabe(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    match o.zustand.entwurf.clone() {
        Linkentwurf::Laedt { .. } => {
            ui.label(text(o, t(s, "preview.loading"), Ton::Blass).small());
        }
        Linkentwurf::Fertig { vorschau } => {
            let mut weg = false;
            vorschaukarte(ui, o, &vorschau, speicher, Some(&mut weg));
            if weg {
                aktionen.push(Aktion::Befehl(Befehl::VorschauVerwerfen));
            }
        }
        Linkentwurf::Keiner | Linkentwurf::Ohne { .. } => {}
    }
    let darf_schreiben =
        o.zustand.textschluessel != Textschluessel::Wartet && o.zustand.meine_rolle().is_some();
    ui.horizontal(|ui| {
        let senden_breite = thema::SIZE_CONTROL_LG + thema::SPACE_2;
        let feld = ui.add_enabled(
            darf_schreiben,
            egui::TextEdit::multiline(&mut o.entwurf)
                .desired_rows(1)
                .desired_width(ui.available_width() - senden_breite)
                .hint_text(t(s, "chat.placeholder"))
                .return_key(egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                )),
        );
        if feld.changed() {
            aktionen.extend(o.entwurf_geaendert());
        }
        let enter =
            feld.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        let f = *o.thema().farben();
        let knopf = ui
            .add_enabled_ui(darf_schreiben, |ui| {
                icon_knopf(ui, &f, Icon::Senden, &t(s, "chat.send"), false)
            })
            .inner;
        if enter || knopf.clicked() {
            aktionen.extend(o.senden());
            feld.request_focus();
        }
    });
}

fn vorschaukarte(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    v: &Linkvorschau,
    speicher: &mut Zwischenspeicher,
    verwerfen: Option<&mut bool>,
) {
    let f = *o.thema().farben();
    egui::Frame::new()
        .fill(f.bg_raised)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_2 as i8))
        .show(ui, |ui| {
            ui.set_max_width(thema::SPACE_8 * 7.0);
            ui.horizontal(|ui| {
                if let Some(textur) = bild(ui.ctx(), speicher, v) {
                    ui.add(
                        egui::Image::new(&textur)
                            .max_height(thema::SPACE_8)
                            .max_width(thema::SPACE_8 * 1.5),
                    );
                }
                ui.vertical(|ui| {
                    if ui
                        .link(
                            RichText::new(&v.titel)
                                .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                        )
                        .clicked()
                    {
                        oeffne_link(ui.ctx(), &v.url);
                    }
                    if let Some(b) = &v.beschreibung {
                        ui.label(text(o, b, Ton::Leise).small());
                    }
                    ui.label(text(o, &v.url, Ton::Blass).small());
                    if let Some(weg) = verwerfen {
                        if ui.small_button(t(o.sprache(), "preview.remove")).clicked() {
                            *weg = true;
                        }
                    }
                });
            });
        });
}

/// Vorschaubild (JPEG, Base64) einmal dekodieren und als Textur merken.
fn bild(
    ctx: &egui::Context,
    speicher: &mut Zwischenspeicher,
    v: &Linkvorschau,
) -> Option<egui::TextureHandle> {
    let roh = v.bild.as_ref()?;
    speicher
        .bilder
        .entry(v.url.clone())
        .or_insert_with(|| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(roh).ok()?;
            let bild = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let groesse = [bild.width() as usize, bild.height() as usize];
            let farbbild = egui::ColorImage::from_rgba_unmultiplied(groesse, bild.as_raw());
            Some(ctx.load_texture(format!("vorschau:{}", v.url), farbbild, Default::default()))
        })
        .clone()
}

/// Links aus dem Chat kommen von anderen: nur `http(s)`, im Browser des
/// Systems — nie im Fenster, nie ein anderes Programm.
fn oeffne_link(ctx: &egui::Context, url: &str) {
    if sprechwerk_ipc::abbilden::ist_weblink(url) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

/// Versatz der Ortszeit zu UTC in Sekunden.
fn ortszeit_versatz() -> i64 {
    use chrono::Offset;
    chrono::Local::now().offset().fix().local_minus_utc() as i64
}

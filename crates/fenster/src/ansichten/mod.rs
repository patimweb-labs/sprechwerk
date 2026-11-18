// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Ansichten. Jede ist eine Funktion `(ui, modell, aktionen)`: sie liest
//! das Modell, zeichnet und legt Aktionen ab — rechnen tut sie nicht.

pub mod dialog;
pub mod effekt;
pub mod einstellungen;
pub mod protokoll;
pub mod raeume;
pub mod server;
pub mod sperren;

use crate::logik;
use crate::modell::{Ansicht, Hinweis, Oberflaeche};
use crate::texte::{t, tf};
use crate::thema::{self, fett, Farben, Ton};
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::{Befehl, Verbindung};

/// Text in einer Bedeutungsfarbe.
pub fn text(o: &Oberflaeche, inhalt: impl Into<String>, art: Ton) -> RichText {
    RichText::new(inhalt).color(thema::ton(o.thema().farben(), art))
}

/// Überschrift einer Seite mit Unterzeile.
pub fn seitenkopf(ui: &mut egui::Ui, o: &Oberflaeche, titel: &str, unter: &str) {
    ui.add_space(thema::SPACE_4);
    ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_2XL, fett())));
    ui.label(text(o, unter, Ton::Leise));
    ui.add_space(thema::SPACE_4);
}

/// Eine Karte: Rahmen, Titel, Inhalt.
pub fn karte<R>(
    ui: &mut egui::Ui,
    f: &Farben,
    titel: &str,
    unter: Option<&str>,
    inhalt: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(f.bg_panel)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_LG, fett())));
            if let Some(u) = unter {
                ui.label(RichText::new(u).color(f.text_muted));
            }
            ui.add_space(thema::SPACE_2);
            inhalt(ui)
        })
        .inner
}

/// Schmaler Pegelbalken 0–100 mit optionaler Marke (Sprechschwelle).
pub fn pegelbalken(ui: &mut egui::Ui, f: &Farben, pegel: u8, marke: Option<u8>, breite: f32) {
    let (rect, antwort) =
        ui.allocate_exact_size(egui::vec2(breite, thema::SPACE_2), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, f.bg_input);
    let mut voll = rect;
    voll.set_width(rect.width() * pegel.min(100) as f32 / 100.0);
    let farbe = match marke {
        Some(m) if pegel < m => f.text_faint,
        _ => f.accent_default,
    };
    p.rect_filled(voll, 0.0, farbe);
    if let Some(m) = marke {
        let x = rect.left() + rect.width() * m.min(100) as f32 / 100.0;
        p.line_segment(
            [
                egui::pos2(x, rect.top() - thema::SIZE_FOCUS),
                egui::pos2(x, rect.bottom() + thema::SIZE_FOCUS),
            ],
            Stroke::new(thema::SIZE_BORDER, f.text_default),
        );
    }
    antwort.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, format!("{pegel} %"))
    });
}

/// Hauptschaltfläche (Akzent).
pub fn hauptknopf(ui: &mut egui::Ui, f: &Farben, beschriftung: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(beschriftung).color(f.text_on_accent))
            .fill(f.accent_default)
            .stroke(Stroke::new(thema::SIZE_BORDER, f.accent_default)),
    )
}

/// Kopfleiste: Marke, Ansichten, eigenes Mikrofon und Ton, Trennen.
pub fn kopf(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::Panel::top("kopf")
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
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("sprechwerk")
                        .font(egui::FontId::new(thema::FONT_SIZE_LG, fett()))
                        .color(f.accent_default),
                );
                ui.add_space(thema::SPACE_4);
                let mut reiter = vec![(Ansicht::Server, t(s, "server.title"))];
                if o.verbunden() {
                    reiter.push((Ansicht::Raeume, t(s, "app.rooms")));
                    if o.zustand.darf_protokoll_lesen() {
                        reiter.push((Ansicht::Protokoll, t(s, "app.audit")));
                    }
                    if logik::darf_sperren(&o.zustand) {
                        reiter.push((Ansicht::Sperren, t(s, "app.bans")));
                    }
                }
                reiter.push((Ansicht::Einstellungen, t(s, "app.settings")));
                for (ansicht, name) in reiter {
                    if ui.selectable_label(o.ansicht == ansicht, name).clicked() {
                        aktionen.extend(o.oeffnen(ansicht));
                    }
                }

                if o.verbunden() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t(s, "app.disconnect")).clicked() {
                            aktionen.push(Aktion::Befehl(Befehl::Trennen));
                            aktionen.push(Aktion::AudioStoppen);
                        }
                        let ton = if o.audio.taub {
                            "app.sound_unmute"
                        } else {
                            "app.sound_mute"
                        };
                        if ui.selectable_label(o.audio.taub, t(s, ton)).clicked() {
                            aktionen.push(Aktion::HoerenAus(!o.audio.taub));
                        }
                        let mik = if o.audio.stumm {
                            "app.mic_unmute"
                        } else {
                            "app.mic_mute"
                        };
                        if ui.selectable_label(o.audio.stumm, t(s, mik)).clicked() {
                            aktionen.push(Aktion::MikrofonStumm(!o.audio.stumm));
                        }
                        let pegel = if o.audio.stumm { 0 } else { o.audio.pegel };
                        pegelbalken(ui, &f, pegel, None, thema::SPACE_8);
                    });
                }
            });
        });
}

/// Fußleiste: Verbindung, Umlaufzeit, Güte, Hinweis.
pub fn fuss(ui: &mut egui::Ui, o: &Oberflaeche) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::Panel::bottom("fuss")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::symmetric(
                    thema::SPACE_4 as i8,
                    thema::SPACE_1 as i8,
                )),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                let (stand, farbe) = match &o.zustand.verbindung {
                    Some(Verbindung::Verbunden {
                        server_name,
                        server,
                        ..
                    }) => (format!("{server_name} · {server}"), f.status_ok),
                    Some(Verbindung::Verbindet { server }) => {
                        (tf(s, "app.connecting", &[server]), f.status_warn)
                    }
                    Some(Verbindung::Fehler { text }) => (text.clone(), f.status_error),
                    _ => (t(s, "app.not_connected"), f.text_faint),
                };
                ui.label(RichText::new(stand).color(farbe));
                if let Some(l) = logik::latenz_text(o.zustand.latenz_ms) {
                    ui.label(RichText::new(format!("· {l}")).color(f.text_muted))
                        .on_hover_text(t(s, "app.latency"));
                }
                if let Some((q, gut)) = logik::qualitaet(s, &o.zustand) {
                    let c = if gut { f.status_ok } else { f.status_warn };
                    ui.label(RichText::new(format!("· {q}")).color(c));
                }
                let meldung = match (&o.hinweis, &o.zustand.meldung) {
                    (Some(Hinweis::Fehler(x)), _) => Some((x.clone(), f.status_error)),
                    (Some(Hinweis::Info(x)), _) => Some((x.clone(), f.text_muted)),
                    (None, Some(x)) => Some((x.clone(), f.status_warn)),
                    (None, None) => None,
                };
                if let Some((m, c)) = meldung {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(m).color(c));
                    });
                }
            });
        });
}

/// Eine leere Fläche mit Titel und Erklärung.
pub fn leer(ui: &mut egui::Ui, o: &Oberflaeche, titel: &str, erklaerung: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(thema::SPACE_7);
        ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_LG, fett())));
        ui.label(text(o, erklaerung, Ton::Leise));
    });
}

/// Farbe für ein Kürzel-Quadrat (Avatar): Akzent für mich, sonst neutral.
pub fn kuerzel(ui: &mut egui::Ui, f: &Farben, name: &str, ich: bool, spricht: bool) {
    let groesse = thema::SIZE_CONTROL_SM;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(groesse, groesse), Sense::hover());
    let p = ui.painter();
    let (flaeche, schrift): (Color32, Color32) = if ich {
        (f.accent_default, f.text_on_accent)
    } else {
        (f.bg_selected, f.text_default)
    };
    p.rect_filled(rect, 0.0, flaeche);
    if spricht {
        p.rect_stroke(
            rect.expand(thema::SIZE_FOCUS),
            0.0,
            Stroke::new(thema::SIZE_FOCUS, f.status_ok),
            egui::StrokeKind::Outside,
        );
    }
    let buchstabe: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        buchstabe,
        egui::FontId::new(thema::FONT_SIZE_MD, fett()),
        schrift,
    );
}

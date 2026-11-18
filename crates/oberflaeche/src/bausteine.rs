// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Bausteine beider Fenster: Karte, Seitenkopf, Pegelbalken, Hauptknopf,
//! leere Fläche, Kürzel. Sie bekommen Farben aus dem Theme, nie Werte.

use crate::thema::{self, fett, Farben, Ton};
use egui::{Color32, RichText, Sense, Stroke};

/// Text in einer Bedeutungsfarbe.
pub fn text(f: &Farben, inhalt: impl Into<String>, art: Ton) -> RichText {
    RichText::new(inhalt).color(thema::ton(f, art))
}

/// Überschrift einer Seite mit Unterzeile.
pub fn seitenkopf(ui: &mut egui::Ui, f: &Farben, titel: &str, unter: &str) {
    ui.add_space(thema::SPACE_4);
    ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_2XL, fett())));
    ui.label(text(f, unter, Ton::Leise));
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

/// Eine leere Fläche mit Titel und Erklärung.
pub fn leer(ui: &mut egui::Ui, f: &Farben, titel: &str, erklaerung: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(thema::SPACE_7);
        ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_LG, fett())));
        ui.label(text(f, erklaerung, Ton::Leise));
    });
}

/// Kürzel-Quadrat (Avatar): Akzent für mich, sonst neutral; grüner Rand, wenn
/// die Person spricht.
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

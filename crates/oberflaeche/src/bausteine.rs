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

/// Die Icons der Fenster — dieselbe kuratierte Lucide-Auswahl wie
/// `@weppelmann/icons` (erzeugt mit `scripts/icons-holen.py`). Ein Summentyp:
/// eine Ansicht kann kein Icon verlangen, das es nicht gibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Mehr,
    Schloss,
    PanelRechts,
    PanelZu,
    Schliessen,
    Mikrofon,
    MikrofonAus,
    Kopfhoerer,
    KopfhoererAus,
    Stecker,
    Trennen,
    Einstellungen,
    Neuladen,
    Lautsprecher,
    Chat,
    Sprechblase,
    Papierkorb,
    Kopieren,
    Reagieren,
    Server,
    Leute,
    Protokoll,
    Sperre,
    Start,
    Stopp,
    Senden,
    Warnung,
}

impl Icon {
    pub fn quelle(self) -> egui::ImageSource<'static> {
        match self {
            Icon::Plus => egui::include_image!("../icons/plus.svg"),
            Icon::Mehr => egui::include_image!("../icons/more.svg"),
            Icon::Schloss => egui::include_image!("../icons/lock.svg"),
            Icon::PanelRechts => egui::include_image!("../icons/panel_right.svg"),
            Icon::PanelZu => egui::include_image!("../icons/panel_close.svg"),
            Icon::Schliessen => egui::include_image!("../icons/close.svg"),
            Icon::Mikrofon => egui::include_image!("../icons/mic.svg"),
            Icon::MikrofonAus => egui::include_image!("../icons/mic_off.svg"),
            Icon::Kopfhoerer => egui::include_image!("../icons/headphones.svg"),
            Icon::KopfhoererAus => egui::include_image!("../icons/headphone_off.svg"),
            Icon::Stecker => egui::include_image!("../icons/plug.svg"),
            Icon::Trennen => egui::include_image!("../icons/unplug.svg"),
            Icon::Einstellungen => egui::include_image!("../icons/settings.svg"),
            Icon::Neuladen => egui::include_image!("../icons/refresh.svg"),
            Icon::Lautsprecher => egui::include_image!("../icons/speaker.svg"),
            Icon::Chat => egui::include_image!("../icons/chat.svg"),
            Icon::Sprechblase => egui::include_image!("../icons/sprechblase.svg"),
            Icon::Papierkorb => egui::include_image!("../icons/trash.svg"),
            Icon::Kopieren => egui::include_image!("../icons/copy.svg"),
            Icon::Reagieren => egui::include_image!("../icons/react.svg"),
            Icon::Server => egui::include_image!("../icons/server.svg"),
            Icon::Leute => egui::include_image!("../icons/users.svg"),
            Icon::Protokoll => egui::include_image!("../icons/log.svg"),
            Icon::Sperre => egui::include_image!("../icons/ban.svg"),
            Icon::Start => egui::include_image!("../icons/play.svg"),
            Icon::Stopp => egui::include_image!("../icons/stop.svg"),
            Icon::Senden => egui::include_image!("../icons/send.svg"),
            Icon::Warnung => egui::include_image!("../icons/warning.svg"),
        }
    }

    /// Das Icon als Bild in Größe und Farbe.
    pub fn bild(self, groesse: f32, farbe: Color32) -> egui::Image<'static> {
        egui::Image::new(self.quelle())
            .tint(farbe)
            .fit_to_exact_size(egui::vec2(groesse, groesse))
    }
}

/// Ein Icon ohne Knopf (Schloss neben dem Raumnamen u. ä.).
pub fn icon(ui: &mut egui::Ui, f: &Farben, icon: Icon, ton: Ton) -> egui::Response {
    ui.add(icon.bild(thema::FONT_SIZE_MD, thema::ton(f, ton)))
}

/// Knopf nur mit Icon. `name` ist Hinweis **und** zugänglicher Name — ein
/// Screenreader liest ihn vor, die Maus zeigt ihn beim Darüberfahren.
/// `an` hebt den Knopf als eingeschaltet hervor (Mikrofon stumm …).
pub fn icon_knopf(
    ui: &mut egui::Ui,
    f: &Farben,
    icon: Icon,
    name: &str,
    an: bool,
) -> egui::Response {
    let farbe = if an { f.accent_default } else { f.text_muted };
    let antwort = ui
        .add(
            egui::Button::image(icon.bild(thema::FONT_SIZE_LG, farbe))
                .selected(an)
                .frame_when_inactive(an)
                .min_size(egui::vec2(thema::SIZE_CONTROL_SM, thema::SIZE_CONTROL_SM)),
        )
        .on_hover_text(name);
    let name = name.to_string();
    antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    antwort
}

/// Knopf mit Icon und Text (Reiter, Hauptaktionen).
pub fn icon_text_knopf(
    ui: &mut egui::Ui,
    f: &Farben,
    icon: Icon,
    text: &str,
    gewaehlt: bool,
) -> egui::Response {
    let (farbe, schrift) = if gewaehlt {
        (f.accent_default, f.text_default)
    } else {
        (f.text_muted, f.text_muted)
    };
    ui.add(
        egui::Button::image_and_text(
            icon.bild(thema::FONT_SIZE_MD, farbe),
            RichText::new(text).color(schrift),
        )
        .selected(gewaehlt)
        .frame_when_inactive(gewaehlt),
    )
}

/// Gewöhnlicher Knopf mit Icon vor dem Text (Neu laden, Sprechtest …).
pub fn knopf(ui: &mut egui::Ui, f: &Farben, icon: Icon, text: &str) -> egui::Response {
    ui.add(egui::Button::image_and_text(
        icon.bild(thema::FONT_SIZE_MD, f.text_muted),
        text,
    ))
}

/// Hauptschaltfläche (Akzent) mit Icon vor dem Text.
pub fn hauptknopf_mit(ui: &mut egui::Ui, f: &Farben, icon: Icon, text: &str) -> egui::Response {
    ui.add(
        egui::Button::image_and_text(
            icon.bild(thema::FONT_SIZE_MD, f.text_on_accent),
            RichText::new(text).color(f.text_on_accent),
        )
        .fill(f.accent_default)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.accent_default)),
    )
}

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Drei Themes und nichts sonst (AGENTS.md §2.7). Die Werte kommen aus den
//! Token-Dateien (`build.rs`); hier wird nur zugeordnet, welcher Token welche
//! Fläche von egui färbt.

use egui::{self, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

/// Das gewählte Farbschema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Thema {
    #[default]
    Dunkel,
    Hell,
    Mono,
}

impl Thema {
    pub const ALLE: [Thema; 3] = [Thema::Dunkel, Thema::Hell, Thema::Mono];

    /// Aus dem gespeicherten Namen; Unbekanntes wird `dark`.
    pub fn aus_name(name: &str) -> Self {
        match name {
            "light" => Thema::Hell,
            "mono" => Thema::Mono,
            _ => Thema::Dunkel,
        }
    }

    /// Name in der Einstellungsdatei.
    pub fn name(self) -> &'static str {
        match self {
            Thema::Dunkel => "dark",
            Thema::Hell => "light",
            Thema::Mono => "mono",
        }
    }

    /// Schlüssel für den Anzeigenamen in `texte.json`.
    pub fn text_schluessel(self) -> &'static str {
        match self {
            Thema::Dunkel => "admin.theme.dark",
            Thema::Hell => "admin.theme.light",
            Thema::Mono => "admin.theme.mono",
        }
    }

    pub fn farben(self) -> &'static Farben {
        match self {
            Thema::Dunkel => &DUNKEL,
            Thema::Hell => &HELL,
            Thema::Mono => &MONO,
        }
    }
}

/// Schriftfamilie für Hervorhebungen (Plex Sans SemiBold).
pub fn fett() -> FontFamily {
    FontFamily::Name("fett".into())
}

/// IBM Plex Sans und Mono; die eingebauten Schriften von egui bleiben als
/// Rückfall dahinter (Emoji, Zeichen, die Plex nicht hat).
pub fn schriften(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut dazu = |name: &str, bytes: &'static [u8]| {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    dazu(
        "plex-sans",
        include_bytes!("../schriften/PlexSans-Regular.ttf"),
    );
    dazu(
        "plex-sans-fett",
        include_bytes!("../schriften/PlexSans-SemiBold.ttf"),
    );
    dazu(
        "plex-mono",
        include_bytes!("../schriften/PlexMono-Regular.ttf"),
    );
    let rueckfall = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    defs.families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex-sans".into());
    defs.families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".into());
    let mut fett_liste = vec!["plex-sans-fett".to_string()];
    fett_liste.extend(rueckfall);
    defs.families.insert(fett(), fett_liste);
    ctx.set_fonts(defs);
}

/// Wendet ein Theme auf egui an.
pub fn anwenden(ctx: &egui::Context, thema: Thema) {
    let f = thema.farben();
    let ecke = CornerRadius::same(RADIUS_MD as u8);
    let rand = Stroke::new(SIZE_BORDER, f.border_subtle);
    let mut style = (*ctx.global_style()).clone();

    let mut v = if thema == Thema::Hell {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.override_text_color = Some(f.text_default);
    v.panel_fill = f.bg_panel;
    v.window_fill = f.bg_raised;
    v.window_stroke = Stroke::new(SIZE_BORDER, f.border_strong);
    v.window_corner_radius = ecke;
    v.menu_corner_radius = ecke;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.extreme_bg_color = f.bg_input;
    v.faint_bg_color = f.bg_raised;
    v.code_bg_color = f.bg_input;
    v.hyperlink_color = f.accent_default;
    v.warn_fg_color = f.status_warn;
    v.error_fg_color = f.status_error;
    // Gewählt heißt: ruhige Fläche, Akzent in der Schrift — nicht ein
    // leuchtender Block (der gehört dem Hauptknopf).
    v.selection.bg_fill = f.bg_selected;
    v.selection.stroke = Stroke::new(SIZE_BORDER, f.accent_default);
    v.text_cursor.stroke = Stroke::new(SIZE_FOCUS, f.focus_ring);
    // Hand über allem, was man anklicken kann — Knöpfe, Häkchen, Reiter,
    // Links (wie in der alten Oberfläche; egui zeigt sonst den Pfeil).
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let w = &mut v.widgets;
    for (zustand, flaeche, linie, schrift) in [
        (
            &mut w.noninteractive,
            f.bg_panel,
            f.border_subtle,
            f.text_default,
        ),
        (
            &mut w.inactive,
            f.bg_raised,
            f.border_subtle,
            f.text_default,
        ),
        (&mut w.hovered, f.bg_hover, f.border_strong, f.text_default),
        (&mut w.active, f.bg_selected, f.focus_ring, f.text_default),
        (&mut w.open, f.bg_selected, f.border_strong, f.text_default),
    ] {
        zustand.bg_fill = flaeche;
        zustand.weak_bg_fill = flaeche;
        zustand.bg_stroke = Stroke::new(SIZE_BORDER, linie);
        zustand.fg_stroke = Stroke::new(SIZE_BORDER, schrift);
        zustand.corner_radius = ecke;
        zustand.expansion = 0.0;
    }
    w.noninteractive.bg_stroke = rand;
    style.visuals = v;

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(SPACE_2, SPACE_2);
    s.button_padding = egui::vec2(SPACE_3, SPACE_1);
    s.interact_size.y = SIZE_CONTROL_SM;
    s.window_margin = egui::Margin::same(SPACE_4 as i8);
    s.menu_margin = egui::Margin::same(SPACE_1 as i8);

    style.text_styles = [
        (TextStyle::Small, FontId::proportional(FONT_SIZE_SM)),
        (TextStyle::Body, FontId::proportional(FONT_SIZE_MD)),
        (TextStyle::Button, FontId::proportional(FONT_SIZE_MD)),
        (TextStyle::Monospace, FontId::monospace(FONT_SIZE_SM)),
        (TextStyle::Heading, FontId::new(FONT_SIZE_XL, fett())),
    ]
    .into();
    ctx.set_global_style(style);
}

/// Text in einer Statusfarbe.
pub fn ton(f: &Farben, art: Ton) -> Color32 {
    match art {
        Ton::Normal => f.text_default,
        Ton::Leise => f.text_muted,
        Ton::Blass => f.text_faint,
        Ton::Gut => f.status_ok,
        Ton::Warnung => f.status_warn,
        Ton::Fehler => f.status_error,
        Ton::Akzent => f.accent_default,
    }
}

/// Bedeutung einer Farbe — die Ansichten wählen Bedeutungen, keine Farben.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ton {
    Normal,
    Leise,
    Blass,
    Gut,
    Warnung,
    Fehler,
    Akzent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_hin_und_zurueck() {
        for t in Thema::ALLE {
            assert_eq!(Thema::aus_name(t.name()), t);
        }
        assert_eq!(Thema::aus_name("rosa"), Thema::Dunkel);
    }

    #[test]
    fn themes_unterscheiden_sich() {
        assert_ne!(DUNKEL.bg_canvas, HELL.bg_canvas);
        assert_ne!(DUNKEL.accent_default, MONO.accent_default);
        // Lesbarkeit: Text hebt sich vom Hintergrund ab.
        for f in [&DUNKEL, &HELL, &MONO] {
            assert_ne!(f.text_default, f.bg_panel);
        }
    }

    #[test]
    fn eckig_wie_die_marke() {
        assert_eq!(RADIUS_MD, 0.0);
        const { assert!(SPACE_2 > 0.0 && SPACE_4 > SPACE_2) };
    }
}

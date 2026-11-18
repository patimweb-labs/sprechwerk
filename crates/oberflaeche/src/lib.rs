// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der Bausatz der sprechwerk-Fenster (ADR 0013): Themes aus den Token-Dateien
//! (`build.rs`), Texte in vier Sprachen, eingebettete Schriften, Bausteine.

pub mod bausteine;
pub mod texte;
pub mod thema;

pub use egui;

/// Einmal beim Start: Schriften und die Bildlader (für die SVG-Icons).
pub fn einrichten(ctx: &egui::Context) {
    thema::schriften(ctx);
    egui_extras::install_image_loaders(ctx);
}

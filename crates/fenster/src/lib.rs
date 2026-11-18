// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! sprechwerk als Fensteranwendung in reinem Rust (egui, ADR 0013).
//!
//! Aufbau: [`modell`] hält, was die Oberfläche zeigt; [`logik`] rechnet;
//! [`ansichten`] zeichnen; [`app`] verbindet das mit dem Client-Kern
//! (`crates/client`). Kein Web, kein CSS, kein TypeScript.

pub mod ansichten;
pub mod app;
pub mod logik;
pub mod modell;
pub mod texte;
pub mod thema;

use std::path::PathBuf;

/// Wo Schlüssel und Einstellungen liegen.
///
/// Derselbe Ort wie bei der früheren Tauri-Anwendung
/// (`<Konfiguration>/de.weppelmann.Sprechwerk`), damit Kennung und gemerkte
/// Server beim Umstieg bleiben. `SPRECHWERK_PROFIL` setzt einen anderen Ort —
/// so laufen zwei Fenster mit zwei Kennungen auf einem Rechner (Rundlauf).
pub fn profil() -> PathBuf {
    if let Some(eigen) = std::env::var_os("SPRECHWERK_PROFIL") {
        return PathBuf::from(eigen);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("de.weppelmann.Sprechwerk")
}

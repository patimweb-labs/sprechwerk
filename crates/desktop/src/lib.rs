// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! sprechwerk als Fensteranwendung.
//!
//! Das Fenster ist eine Web-Anwendung (Vue, in `ui/`). Diese Bibliothek startet
//! es und stellt die Befehle bereit; gerechnet wird in `crates/domain`,
//! `crates/protocol`, `crates/adapters` und `crates/ipc`.

pub mod anwendung;
pub mod commands;
pub mod einstellungen;

use anwendung::Anwendung;
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let verzeichnis = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let _ = std::fs::create_dir_all(&verzeichnis);
            let schluessel_pfad = verzeichnis.join("schluessel");

            let fenster = app.handle().clone();
            // Nach jeder Änderung geht der **ganze** Stand ans Fenster
            // (AGENTS.md §2.1: die Oberfläche ist `render(modell)`).
            let anwendung = Anwendung::starten(schluessel_pfad, move |zustand| {
                let _ = fenster.emit("zustand", &zustand);
            });
            app.manage(Arc::new(anwendung));

            let einstellungen_pfad = verzeichnis.join("einstellungen.json");
            app.manage(einstellungen::EinstellungenPfad(einstellungen_pfad));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::zustand,
            commands::verbinden,
            commands::trennen,
            commands::betreten,
            commands::verlassen,
            commands::text_senden,
            commands::nachricht_loeschen,
            commands::raum_anlegen,
            commands::raum_umbenennen,
            commands::raum_loeschen,
            commands::rolle_setzen,
            commands::protokoll,
            commands::einstellungen_lesen,
            commands::einstellungen_speichern,
        ])
        .run(tauri::generate_context!())
        .expect("Sprechwerk konnte das Fenster nicht starten");
}

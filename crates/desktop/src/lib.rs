// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! sprechwerk als Fensteranwendung.
//!
//! Das Fenster ist eine Web-Anwendung (Vue, in `ui/`). Diese Bibliothek startet
//! es und stellt die Befehle bereit; gerechnet wird in `crates/domain`,
//! `crates/protocol`, `crates/adapters` und `crates/ipc`.

pub mod anwendung;
pub mod audio;
pub mod commands;
pub mod einstellungen;
pub mod sprech;
pub mod verschluesselung;

use anwendung::Anwendung;
use std::sync::Arc;
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Fenster- und Taskleistensymbol: kommt aus der Anwendung selbst,
            // nicht aus dem Paket — so stimmt es auch beim Start aus dem
            // Quellbaum (`cargo run`), wo es kein Paket gibt.
            if let Some(fenster) = app.get_webview_window("main") {
                match tauri::image::Image::from_bytes(include_bytes!("../icons/icon-256.png")) {
                    Ok(bild) => {
                        if let Err(fehler) = fenster.set_icon(bild) {
                            eprintln!("Fenstersymbol: {fehler}");
                        }
                    }
                    Err(fehler) => eprintln!("Fenstersymbol nicht lesbar: {fehler}"),
                }
            }

            let verzeichnis = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let _ = std::fs::create_dir_all(&verzeichnis);
            let schluessel_pfad = verzeichnis.join("schluessel");

            let fenster = app.handle().clone();
            // Nach jeder Änderung geht der **ganze** Stand ans Fenster
            // (AGENTS.md §2.1: die Oberfläche ist `render(modell)`).
            let sprech = Arc::new(sprech::Sprechbetrieb::neu());
            let verschluesselung = Arc::new(verschluesselung::Schluesselverwaltung::neu());
            let anwendung = Anwendung::starten(
                schluessel_pfad,
                sprech.clone(),
                verschluesselung.clone(),
                move |zustand| {
                    let _ = fenster.emit("zustand", &zustand);
                },
            );
            app.manage(Arc::new(anwendung));
            app.manage(sprech.clone());
            app.manage(verschluesselung);

            // Pegel für die Anzeige: eigener Zeitgeber, 10 × je Sekunde.
            // (Der Stand kommt sonst nur bei Änderungen — ein Pegel ändert sich
            // ständig und gehört deshalb nicht in `zustand`.)
            let pegel_fenster = app.handle().clone();
            let pegel_sprech = sprech.clone();
            std::thread::Builder::new()
                .name("sprechwerk-pegel".into())
                .spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if !pegel_sprech.laeuft() {
                        continue;
                    }
                    let _ = pegel_fenster.emit("pegel", pegel_sprech.pegel());
                })
                .ok();

            let einstellungen_pfad = verzeichnis.join("einstellungen.json");
            app.manage(einstellungen::EinstellungenPfad(einstellungen_pfad));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::zustand,
            commands::verbinden,
            commands::trennen,
            commands::betreten,
            commands::text_senden,
            commands::nachricht_loeschen,
            commands::raum_anlegen,
            commands::raum_umbenennen,
            commands::raum_loeschen,
            commands::rolle_setzen,
            commands::stummschalten,
            commands::verschieben,
            commands::aus_raum_werfen,
            commands::trennen_mitglied,
            commands::sperren,
            commands::entsperren,
            commands::protokoll,
            commands::zaehler,
            commands::sperrliste,
            commands::raum_verschluesseln,
            commands::audio_starten,
            commands::audio_stoppen,
            commands::audio_stand,
            commands::audio_einstellen,
            commands::sprechtaste,
            commands::mikrofon_stumm,
            commands::hoeren_aus,
            commands::audio_geraete,
            commands::audio_pegel,
            commands::audio_testton,
            commands::einstellungen_lesen,
            commands::einstellungen_speichern,
        ])
        .run(tauri::generate_context!())
        .expect("Sprechwerk konnte das Fenster nicht starten");
}

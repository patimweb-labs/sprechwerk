// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

// Unter Windows kein zusätzliches Konsolenfenster im Release-Bau.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use sprechwerk_client::Client;
use sprechwerk_fenster::app::Fenster;
use sprechwerk_fenster::modell::Oberflaeche;
use sprechwerk_fenster::profil;
use std::sync::{mpsc, Arc, Mutex};

fn main() -> eframe::Result {
    let symbol =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/icon-256.png"))
            .unwrap_or_default();
    let optionen = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sprechwerk")
            .with_app_id("de.weppelmann.Sprechwerk")
            .with_icon(Arc::new(symbol))
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sprechwerk",
        optionen,
        Box::new(|cc| {
            sprechwerk_oberflaeche::einrichten(&cc.egui_ctx);
            let (tx, rx) = mpsc::channel();
            let tx = Mutex::new(tx);
            let ctx = cc.egui_ctx.clone();
            // Jede Meldung des Clients weckt das Fenster: neues Bild, neues Modell.
            let client = Client::starten(&profil(), move |m| {
                if let Ok(tx) = tx.lock() {
                    let _ = tx.send(m);
                }
                ctx.request_repaint();
            });
            let o = Oberflaeche::neu(client.einstellungen(), client.zustand());
            Ok(Box::new(Fenster::neu(client, rx, o)))
        }),
    )
}

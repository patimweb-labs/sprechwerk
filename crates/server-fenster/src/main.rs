// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

// Unter Windows kein zusätzliches Konsolenfenster im Release-Bau.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use sprechwerk_oberflaeche::thema;
use sprechwerk_server::dienst;
use sprechwerk_server::mitschrift::Mitschrift;
use sprechwerk_server::verwaltung::Verwaltung;
use sprechwerk_server_fenster::{zeichnen, Aktion, Lauf, Modell};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Was der Dienstfaden meldet.
enum Bericht {
    Bereit(Verwaltung),
    Ende(Result<(), String>),
}

struct App {
    m: Modell,
    mitschrift: Mitschrift,
    berichte: mpsc::Receiver<Bericht>,
    sender: mpsc::Sender<Bericht>,
    anhalten: Option<tokio::sync::oneshot::Sender<()>>,
    angewandt: Option<thema::Thema>,
    zuletzt: Option<Instant>,
}

impl App {
    fn starten(&mut self, ctx: &egui::Context) {
        let konfig = match self.m.entwurf.konfig() {
            Ok(k) => k,
            Err(fehler) => {
                self.m.lauf = Lauf::Fehler(fehler);
                return;
            }
        };
        let _ = std::fs::create_dir_all(&konfig.server.daten);
        let (stopp, gestoppt) = tokio::sync::oneshot::channel();
        self.anhalten = Some(stopp);
        self.m.lauf = Lauf::Startet;
        self.m.zeilen.clear();
        self.m.weiter_ab = 0;
        let sender = self.sender.clone();
        let mitschrift = self.mitschrift.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("sprechwerk-dienst".into())
            .spawn(move || {
                let laufzeit = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(l) => l,
                    Err(f) => {
                        let _ = sender.send(Bericht::Ende(Err(f.to_string())));
                        return;
                    }
                };
                let bereit_sender = sender.clone();
                let bereit_ctx = ctx.clone();
                let (merk_tx, merk_rx) = std::sync::mpsc::channel::<Verwaltung>();
                let ergebnis = laufzeit.block_on(async move {
                    tokio::select! {
                        r = dienst::laufen(konfig, mitschrift, move |v| {
                            let _ = merk_tx.send(v.clone());
                            let _ = bereit_sender.send(Bericht::Bereit(v));
                            bereit_ctx.request_repaint();
                        }) => r.map_err(|f| format!("{f:#}")),
                        _ = gestoppt => Ok(()),
                    }
                });
                // Beim Anhalten (und nach einem Fehler) den Zustand sichern.
                if let Ok(v) = merk_rx.try_recv() {
                    dienst::speichern(&v);
                }
                laufzeit.shutdown_timeout(Duration::from_secs(2));
                let _ = sender.send(Bericht::Ende(ergebnis));
                ctx.request_repaint();
            })
            .expect("Dienstfaden");
    }

    fn anhalten(&mut self) {
        if let Some(stopp) = self.anhalten.take() {
            let _ = stopp.send(());
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Ok(b) = self.berichte.try_recv() {
            match b {
                Bericht::Bereit(v) => self.m.lauf = Lauf::Laeuft(v),
                Bericht::Ende(Ok(())) => {
                    self.m.lauf = Lauf::Gestoppt;
                    self.m.stand = None;
                }
                Bericht::Ende(Err(f)) => {
                    self.m.lauf = Lauf::Fehler(f);
                    self.m.stand = None;
                }
            }
        }
        if self.angewandt != Some(self.m.thema) {
            thema::anwenden(&ctx, self.m.thema);
            self.angewandt = Some(self.m.thema);
        }
        if self.m.lauf.verwaltung().is_some() {
            if self
                .zuletzt
                .is_none_or(|z| z.elapsed() >= Duration::from_secs(1))
            {
                self.m.auffrischen();
                self.zuletzt = Some(Instant::now());
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        for aktion in zeichnen(ui, &mut self.m) {
            match aktion {
                Aktion::Starten => self.starten(&ctx),
                Aktion::Anhalten => self.anhalten(),
                Aktion::Rolle { mitglied, rolle } => {
                    if let Some(v) = self.m.lauf.verwaltung().cloned() {
                        let name = self
                            .m
                            .stand
                            .as_ref()
                            .and_then(|s| s.mitglieder.iter().find(|z| z.id.text() == mitglied))
                            .map(|z| z.name.clone())
                            .unwrap_or_default();
                        let ergebnis = v.rolle_setzen(&mitglied, rolle);
                        self.m.rolle_gesetzt(&name, ergebnis);
                    }
                }
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(v) = self.m.lauf.verwaltung() {
            dienst::speichern(v);
        }
        self.anhalten();
    }
}

fn main() -> eframe::Result {
    let mitschrift = dienst::protokoll_einrichten();
    let symbol =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/icon-256.png"))
            .unwrap_or_default();
    let optionen = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sprechwerk Server")
            .with_app_id("de.weppelmann.Sprechwerk.Server")
            .with_icon(std::sync::Arc::new(symbol))
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([thema::SPACE_8 * 5.0, thema::SPACE_8 * 6.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sprechwerk Server",
        optionen,
        Box::new(move |cc| {
            sprechwerk_oberflaeche::einrichten(&cc.egui_ctx);
            let (sender, berichte) = mpsc::channel();
            Ok(Box::new(App {
                m: Modell::default(),
                mitschrift,
                berichte,
                sender,
                anhalten: None,
                angewandt: None,
                zuletzt: None,
            }))
        }),
    )
}

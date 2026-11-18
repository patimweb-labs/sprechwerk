// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Anwendung: Meldungen einsammeln, Modell fortschreiben, zeichnen,
//! Aktionen abgeben. Jedes Bild ist `render(modell)` (AGENTS.md §2.1).

use crate::ansichten;
use crate::logik::{self, Sprechtaste};
use crate::modell::{Ansicht, Oberflaeche};
use crate::thema::{self, Thema};
use eframe::egui::{self, PointerButton};
use sprechwerk_client::sitzung::{Aktion, Meldung};
use sprechwerk_client::{AudioStand, Client};
use sprechwerk_ipc::Befehl;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Was das Fenster vom Client braucht. Ein Trait, damit die Oberfläche ohne
/// Server und ohne Soundkarte geprüft werden kann (AGENTS.md §4).
pub trait Kern {
    fn ausfuehren(&self, aktion: Aktion);
    fn audio_stand(&self) -> AudioStand;
}

impl Kern for Client {
    fn ausfuehren(&self, aktion: Aktion) {
        Client::ausfuehren(self, aktion)
    }
    fn audio_stand(&self) -> AudioStand {
        Client::audio_stand(self)
    }
}

/// Zeichenhilfen, die aus dem Modell abgeleitet und nur zwischengespeichert
/// sind (Texturen der Vorschaubilder).
#[derive(Default)]
pub struct Zwischenspeicher {
    pub bilder: HashMap<String, Option<egui::TextureHandle>>,
}

pub struct Fenster<K: Kern> {
    kern: K,
    meldungen: Receiver<Meldung>,
    pub o: Oberflaeche,
    angewandt: Option<Thema>,
    takt: Option<Instant>,
    speicher: Zwischenspeicher,
    /// Stand des Chats beim letzten Bild — ein Wechsel ändert die Fenstergröße.
    eingeklappt_zuletzt: Option<bool>,
    /// Breite vor dem Einklappen (kommt beim Ausklappen zurück).
    breite_vorher: Option<f32>,
}

/// Abstand zwischen zwei Abfragen von Betriebszahlen und Umlaufzeit.
const TAKT: Duration = Duration::from_secs(10);

impl<K: Kern> Fenster<K> {
    pub fn neu(kern: K, meldungen: Receiver<Meldung>, o: Oberflaeche) -> Self {
        Self {
            kern,
            meldungen,
            o,
            angewandt: None,
            takt: None,
            speicher: Zwischenspeicher::default(),
            eingeklappt_zuletzt: None,
            breite_vorher: None,
        }
    }

    /// Ein Bild: einsammeln, zeichnen, abgeben.
    pub fn bild(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        while let Ok(m) = self.meldungen.try_recv() {
            self.o.melden(m);
        }
        if self.angewandt != Some(self.o.thema()) {
            thema::anwenden(ctx, self.o.thema());
            self.angewandt = Some(self.o.thema());
        }

        self.fenstergroesse(ctx);

        let mut aktionen = Vec::new();
        if self.o.verbunden() {
            self.o.audio = self.kern.audio_stand();
            // Pegel und „spricht" leben: zehn Bilder je Sekunde genügen.
            ctx.request_repaint_after(Duration::from_millis(100));
            if self.takt.is_none_or(|t| t.elapsed() >= TAKT) {
                self.takt = Some(Instant::now());
                aktionen.push(Aktion::Befehl(Befehl::Zaehler));
                aktionen.push(Aktion::Befehl(Befehl::Ping));
            }
        } else {
            self.takt = None;
        }
        self.sprechtaste(ctx, &mut aktionen);

        ansichten::kopf(ui, &mut self.o, &mut aktionen);
        ansichten::fuss(ui, &self.o);
        egui::CentralPanel::default()
            // Ohne Rand: den setzen die Seiten innen (`bausteine::seite`), damit
            // Scrollbalken am Fensterrand sitzen.
            .frame(egui::Frame::new().fill(self.o.thema().farben().bg_canvas))
            .show(ui, |ui| match self.o.ansicht {
                Ansicht::Server => ansichten::server::zeigen(ui, &mut self.o, &mut aktionen),
                Ansicht::Raeume => {
                    ansichten::raeume::zeigen(ui, &mut self.o, &mut self.speicher, &mut aktionen)
                }
                Ansicht::Protokoll => ansichten::protokoll::zeigen(ui, &mut self.o, &mut aktionen),
                Ansicht::Sperren => ansichten::sperren::zeigen(ui, &mut self.o, &mut aktionen),
                Ansicht::Einstellungen => {
                    ansichten::einstellungen::zeigen(ui, &mut self.o, &mut aktionen)
                }
            });
        ansichten::dialog::zeigen(ctx, &mut self.o, &mut aktionen);

        for aktion in aktionen {
            self.kern.ausfuehren(aktion);
        }
    }

    /// Chat eingeklappt: das Fenster darf kompakt werden und wird es beim
    /// Einklappen auch; ausgeklappt kommt die alte Breite zurück.
    fn fenstergroesse(&mut self, ctx: &egui::Context) {
        let eingeklappt = self.o.einstellungen.chat_eingeklappt;
        if self.eingeklappt_zuletzt == Some(eingeklappt) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            logik::mindestbreite(eingeklappt),
            logik::HOEHE_MIN,
        )));
        // Beim ersten Bild nur die Grenze setzen — die Größe hat der Nutzer.
        if self.eingeklappt_zuletzt.is_some() {
            let jetzt = ctx.input(|i| i.viewport().inner_rect);
            if eingeklappt {
                self.breite_vorher = jetzt.map(|r| r.width());
            }
            let breite = logik::breite_nach_wechsel(eingeklappt, self.breite_vorher);
            let hoehe = jetzt.map_or(logik::HOEHE_MIN, |r| r.height());
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(breite, hoehe)));
        }
        self.eingeklappt_zuletzt = Some(eingeklappt);
    }

    /// Push-to-Talk: Taste oder Maus 4/5, nur solange das Fenster den Fokus
    /// hat. In einem Eingabefeld zählt eine schreibende Taste nicht.
    fn sprechtaste(&mut self, ctx: &egui::Context, aktionen: &mut Vec<Aktion>) {
        if !self.o.einstellungen.push_to_talk || self.o.taste_aufnehmen {
            return;
        }
        let Some(taste) = Sprechtaste::aus_name(&self.o.einstellungen.ptt_taste) else {
            return;
        };
        let tippt = ctx.egui_wants_keyboard_input();
        let gedrueckt = ctx.input(|i| {
            i.focused
                && match taste {
                    Sprechtaste::Maus4 => i.pointer.button_down(PointerButton::Extra1),
                    Sprechtaste::Maus5 => i.pointer.button_down(PointerButton::Extra2),
                    Sprechtaste::Taste(k) => i.key_down(k) && !(tippt && taste.schreibt()),
                }
        });
        aktionen.extend(self.o.sprechen(gedrueckt));
    }
}

impl<K: Kern> eframe::App for Fenster<K> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.bild(ui);
    }
}

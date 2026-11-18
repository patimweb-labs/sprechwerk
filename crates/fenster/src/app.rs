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
    /// Eine geladene Datei entschlüsselt (Bild für Vorschau und Betrachter).
    fn datei(&self, id: &str) -> Option<Vec<u8>>;
}

impl Kern for Client {
    fn ausfuehren(&self, aktion: Aktion) {
        Client::ausfuehren(self, aktion)
    }
    fn audio_stand(&self) -> AudioStand {
        Client::audio_stand(self)
    }
    fn datei(&self, id: &str) -> Option<Vec<u8>> {
        Client::datei(self, id)
    }
}

/// Zeichenhilfen, die aus dem Modell abgeleitet und nur zwischengespeichert
/// sind (Texturen der Vorschaubilder).
#[derive(Default)]
pub struct Zwischenspeicher {
    pub bilder: HashMap<String, Option<egui::TextureHandle>>,
    /// Für welche Nachricht die Reaktionsauswahl offen ist (sie bleibt
    /// stehen, auch wenn die Maus die Nachricht verlässt).
    pub reaktion: Option<sprechwerk_domain::MessageId>,
}

/// Wie die Ansichten an den Inhalt einer Datei kommen (über den `Kern`).
pub type Dateiquelle<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

pub struct Fenster<K: Kern> {
    kern: K,
    meldungen: Receiver<Meldung>,
    pub o: Oberflaeche,
    angewandt: Option<Thema>,
    takt: Option<Instant>,
    speicher: Zwischenspeicher,
    /// Stand des Chats beim letzten Bild — ein Wechsel ändert die Fenstergröße.
    eingeklappt_zuletzt: Option<bool>,
    /// Zuletzt gesehene Breite mit Chat und kompakt — jede kommt zurück,
    /// wenn man wieder in diesen Zustand wechselt.
    breite_mit_chat: Option<f32>,
    breite_kompakt: Option<f32>,
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
            breite_mit_chat: None,
            breite_kompakt: None,
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
        aktionen.extend(self.o.sprechtest_aufraeumen());
        if self.o.sprechtest || self.o.ansicht == Ansicht::Einstellungen {
            // Der Pegel im Sprechtest lebt: 20 Bilder je Sekunde.
            self.o.audio = self.kern.audio_stand();
            if self.o.audio.laeuft {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
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
        self.hineinziehen(ctx, &mut aktionen);

        ansichten::kopf(ui, &mut self.o, &mut aktionen);
        ansichten::fuss(ui, &self.o);
        egui::CentralPanel::default()
            // Ohne Rand: den setzen die Seiten innen (`bausteine::seite`), damit
            // Scrollbalken am Fensterrand sitzen.
            .frame(egui::Frame::new().fill(self.o.thema().farben().bg_canvas))
            .show(ui, |ui| match self.o.ansicht {
                Ansicht::Server => ansichten::server::zeigen(ui, &mut self.o, &mut aktionen),
                Ansicht::Raeume => ansichten::raeume::zeigen(
                    ui,
                    &mut self.o,
                    &mut self.speicher,
                    &|id: &str| self.kern.datei(id),
                    &mut aktionen,
                ),
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
        // Die Fläche, die egui zeichnet, ist die Innengröße des Fensters —
        // anders als `viewport().inner_rect` kennt sie jedes System (unter
        // Wayland fehlte jene, und die Höhe fiel auf die Mindesthöhe).
        let jetzt = ctx.content_rect().size();
        if self.eingeklappt_zuletzt == Some(eingeklappt) {
            // Wer die Breite von Hand ändert, dessen Breite gilt beim
            // nächsten Wechsel zurück in diesen Zustand.
            if eingeklappt {
                self.breite_kompakt = Some(jetzt.x);
            } else {
                self.breite_mit_chat = Some(jetzt.x);
            }
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            logik::mindestbreite(eingeklappt),
            logik::HOEHE_MIN,
        )));
        // Beim ersten Bild nur die Grenze setzen — die Größe hat der Nutzer.
        if self.eingeklappt_zuletzt.is_some() {
            let breite =
                logik::breite_nach_wechsel(eingeklappt, self.breite_mit_chat, self.breite_kompakt);
            // Die Höhe bleibt, wie sie ist.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                breite, jetzt.y,
            )));
        }
        self.eingeklappt_zuletzt = Some(eingeklappt);
    }

    /// Dateien ins Fenster ziehen: sie kommen in den Entwurf des Raums, in
    /// dem man sitzt (ADR 0012 §3). Solange sie darüber schweben, sagt ein
    /// Hinweis, was passiert.
    fn hineinziehen(&mut self, ctx: &egui::Context, aktionen: &mut Vec<Aktion>) {
        if !self.o.verbunden() || self.o.zustand.aktueller_raum.is_none() {
            return;
        }
        let (pfade, schwebt) = ctx.input(|i| {
            let pfade: Vec<String> = i
                .raw
                .dropped_files
                .iter()
                .map(|d| d.path().display().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            (pfade, !i.raw.hovered_files.is_empty())
        });
        if !pfade.is_empty() {
            self.o.ansicht = Ansicht::Raeume;
            if self.o.einstellungen.chat_eingeklappt {
                self.o.einstellungen.chat_eingeklappt = false;
                aktionen.push(self.o.speichern());
            }
            aktionen.push(Aktion::Befehl(Befehl::AnhaengeHinzufuegen { pfade }));
        }
        if schwebt {
            let f = self.o.thema().farben();
            let flaeche = ctx.content_rect();
            let maler = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("hineinziehen"),
            ));
            maler.rect_filled(flaeche, 0.0, f.bg_canvas.gamma_multiply(0.85));
            maler.rect_stroke(
                flaeche.shrink(thema::SPACE_4),
                0.0,
                egui::Stroke::new(thema::SIZE_FOCUS, f.accent_default),
                egui::StrokeKind::Inside,
            );
            maler.text(
                flaeche.center(),
                egui::Align2::CENTER_CENTER,
                crate::texte::t(self.o.sprache(), "file.drop"),
                egui::FontId::new(thema::FONT_SIZE_XL, thema::fett()),
                f.text_default,
            );
        }
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

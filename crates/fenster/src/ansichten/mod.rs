// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Ansichten. Jede ist eine Funktion `(ui, modell, aktionen)`: sie liest
//! das Modell, zeichnet und legt Aktionen ab — rechnen tut sie nicht.

pub mod dialog;
pub mod effekt;
pub mod einstellungen;
pub mod protokoll;
pub mod raeume;
pub mod server;
pub mod sperren;

use crate::logik;
use crate::modell::{Ansicht, Hinweis, Oberflaeche};
use crate::texte::{t, tf};
use crate::thema::{self, fett, Ton};
use eframe::egui::{self, RichText, Stroke};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::{Befehl, Verbindung};
pub use sprechwerk_oberflaeche::bausteine::{self, hauptknopf, karte, kuerzel, pegelbalken};

/// Text in einer Bedeutungsfarbe.
pub fn text(o: &Oberflaeche, inhalt: impl Into<String>, art: Ton) -> RichText {
    bausteine::text(o.thema().farben(), inhalt, art)
}

/// Überschrift einer Seite mit Unterzeile.
pub fn seitenkopf(ui: &mut egui::Ui, o: &Oberflaeche, titel: &str, unter: &str) {
    bausteine::seitenkopf(ui, o.thema().farben(), titel, unter)
}

/// Eine leere Fläche mit Titel und Erklärung.
pub fn leer(ui: &mut egui::Ui, o: &Oberflaeche, titel: &str, erklaerung: &str) {
    bausteine::leer(ui, o.thema().farben(), titel, erklaerung)
}

/// Kopfleiste: Marke, Ansichten, eigenes Mikrofon und Ton, Trennen.
pub fn kopf(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::Panel::top("kopf")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::symmetric(
                    thema::SPACE_4 as i8,
                    thema::SPACE_2 as i8,
                )),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("sprechwerk")
                        .font(egui::FontId::new(thema::FONT_SIZE_LG, fett()))
                        .color(f.accent_default),
                );
                ui.add_space(thema::SPACE_4);
                let mut reiter = vec![(Ansicht::Server, t(s, "server.title"))];
                if o.verbunden() {
                    reiter.push((Ansicht::Raeume, t(s, "app.rooms")));
                    if o.zustand.darf_protokoll_lesen() {
                        reiter.push((Ansicht::Protokoll, t(s, "app.audit")));
                    }
                    if logik::darf_sperren(&o.zustand) {
                        reiter.push((Ansicht::Sperren, t(s, "app.bans")));
                    }
                }
                reiter.push((Ansicht::Einstellungen, t(s, "app.settings")));
                for (ansicht, name) in reiter {
                    if ui.selectable_label(o.ansicht == ansicht, name).clicked() {
                        aktionen.extend(o.oeffnen(ansicht));
                    }
                }

                if o.verbunden() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t(s, "app.disconnect")).clicked() {
                            aktionen.push(Aktion::Befehl(Befehl::Trennen));
                            aktionen.push(Aktion::AudioStoppen);
                        }
                        let ton = if o.audio.taub {
                            "app.sound_unmute"
                        } else {
                            "app.sound_mute"
                        };
                        if ui.selectable_label(o.audio.taub, t(s, ton)).clicked() {
                            aktionen.push(Aktion::HoerenAus(!o.audio.taub));
                        }
                        let mik = if o.audio.stumm {
                            "app.mic_unmute"
                        } else {
                            "app.mic_mute"
                        };
                        if ui.selectable_label(o.audio.stumm, t(s, mik)).clicked() {
                            aktionen.push(Aktion::MikrofonStumm(!o.audio.stumm));
                        }
                        let pegel = if o.audio.stumm { 0 } else { o.audio.pegel };
                        pegelbalken(ui, &f, pegel, None, thema::SPACE_8);
                    });
                }
            });
        });
}

/// Fußleiste: Verbindung, Umlaufzeit, Güte, Hinweis.
pub fn fuss(ui: &mut egui::Ui, o: &Oberflaeche) {
    let s = o.sprache();
    let f = *o.thema().farben();
    egui::Panel::bottom("fuss")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::symmetric(
                    thema::SPACE_4 as i8,
                    thema::SPACE_1 as i8,
                )),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                let (stand, farbe) = match &o.zustand.verbindung {
                    Some(Verbindung::Verbunden {
                        server_name,
                        server,
                        ..
                    }) => (format!("{server_name} · {server}"), f.status_ok),
                    Some(Verbindung::Verbindet { server }) => {
                        (tf(s, "app.connecting", &[server]), f.status_warn)
                    }
                    Some(Verbindung::Fehler { text }) => (text.clone(), f.status_error),
                    _ => (t(s, "app.not_connected"), f.text_faint),
                };
                ui.label(RichText::new(stand).color(farbe));
                if let Some(l) = logik::latenz_text(o.zustand.latenz_ms) {
                    ui.label(RichText::new(format!("· {l}")).color(f.text_muted))
                        .on_hover_text(t(s, "app.latency"));
                }
                if let Some((q, gut)) = logik::qualitaet(s, &o.zustand) {
                    let c = if gut { f.status_ok } else { f.status_warn };
                    ui.label(RichText::new(format!("· {q}")).color(c));
                }
                let meldung = match (&o.hinweis, &o.zustand.meldung) {
                    (Some(Hinweis::Fehler(x)), _) => Some((x.clone(), f.status_error)),
                    (Some(Hinweis::Info(x)), _) => Some((x.clone(), f.text_muted)),
                    (None, Some(x)) => Some((x.clone(), f.status_warn)),
                    (None, None) => None,
                };
                if let Some((m, c)) = meldung {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(m).color(c));
                    });
                }
            });
        });
}

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Ansichten. Jede ist eine Funktion `(ui, modell, aktionen)`: sie liest
//! das Modell, zeichnet und legt Aktionen ab — rechnen tut sie nicht.

pub mod dialog;
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
pub use sprechwerk_oberflaeche::bausteine::{
    self, hauptknopf, hauptknopf_mit, icon, icon_knopf, icon_text_knopf, karte, knopf, kuerzel,
    pegelbalken, Icon,
};

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

/// Ab dieser Breite stehen die Reiter mit Text da, darunter nur als Icon.
const BREIT: f32 = thema::SPACE_8 * 14.0;

/// Kopfleiste: Marke, Ansichten, eigenes Mikrofon und Ton, Trennen.
///
/// `Sides` teilt die Zeile: rechts die Knöpfe in fester Breite, links bekommt
/// den Rest — bei einem schmalen Fenster schieben sich nie Teile übereinander.
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
            let breit = ui.available_width() >= BREIT;
            // Rechts nur Abzüge des Modells — links wird es verändert.
            let verbunden = o.verbunden();
            let audio = o.audio;
            let mut rechts = Vec::new();
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_SM)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.label(
                            RichText::new("sprechwerk")
                                .font(egui::FontId::new(thema::FONT_SIZE_LG, fett()))
                                .color(f.accent_default),
                        );
                        ui.add_space(thema::SPACE_3);
                        let mut reiter = vec![(Ansicht::Server, Icon::Server, "server.title")];
                        if o.verbunden() {
                            reiter.push((Ansicht::Raeume, Icon::Sprechblase, "app.rooms"));
                            if o.zustand.darf_protokoll_lesen() {
                                reiter.push((Ansicht::Protokoll, Icon::Protokoll, "app.audit"));
                            }
                            if logik::darf_sperren(&o.zustand) {
                                reiter.push((Ansicht::Sperren, Icon::Sperre, "app.bans"));
                            }
                        }
                        reiter.push((Ansicht::Einstellungen, Icon::Einstellungen, "app.settings"));
                        for (ansicht, bild, schluessel) in reiter {
                            let gewaehlt = o.ansicht == ansicht;
                            let name = t(s, schluessel);
                            let antwort = if breit {
                                icon_text_knopf(ui, &f, bild, &name, gewaehlt)
                            } else {
                                icon_knopf(ui, &f, bild, &name, gewaehlt)
                            };
                            if antwort.clicked() {
                                aktionen.extend(o.oeffnen(ansicht));
                            }
                        }
                    },
                    |ui| {
                        if !verbunden {
                            return;
                        }
                        if icon_knopf(ui, &f, Icon::Trennen, &t(s, "app.disconnect"), false)
                            .clicked()
                        {
                            rechts.push(Aktion::Befehl(Befehl::Trennen));
                            rechts.push(Aktion::AudioStoppen);
                        }
                        let (bild, name) = if audio.taub {
                            (Icon::KopfhoererAus, "app.sound_unmute")
                        } else {
                            (Icon::Kopfhoerer, "app.sound_mute")
                        };
                        if icon_knopf(ui, &f, bild, &t(s, name), audio.taub).clicked() {
                            rechts.push(Aktion::HoerenAus(!audio.taub));
                        }
                        let (bild, name) = if audio.stumm {
                            (Icon::MikrofonAus, "app.mic_unmute")
                        } else {
                            (Icon::Mikrofon, "app.mic_mute")
                        };
                        if icon_knopf(ui, &f, bild, &t(s, name), audio.stumm).clicked() {
                            rechts.push(Aktion::MikrofonStumm(!audio.stumm));
                        }
                        let pegel = if audio.stumm { 0 } else { audio.pegel };
                        pegelbalken(ui, &f, pegel, None, thema::SPACE_7);
                    },
                );
            aktionen.extend(rechts);
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

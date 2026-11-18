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
    self, hauptknopf, hauptknopf_mit, icon, icon_knopf, icon_text_knopf, karte, knopf, pegelbalken,
    Icon,
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

/// Unter dieser Breite ist die Kopfleiste kompakt (Werkzeuge im Menü).
pub const KOMPAKT: f32 = thema::SPACE_8 * 9.0;

/// Kopfleiste wie die Werkzeugleiste von TeamSpeak: links der Server, auf
/// dem man ist (ein Klick führt zum Kanalbaum, der Hauptansicht), rechts die
/// Werkzeuge — eigenes Mikrofon und Ton, dann Protokoll und Sperren (nur mit
/// Recht), Serverliste, Einstellungen, Trennen.
///
/// `Sides` teilt die Zeile: rechts feste Breite, links wird gekürzt — bei
/// einem schmalen Fenster schiebt sich nichts übereinander.
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
            let verbunden = o.verbunden();
            let audio = o.audio;
            let ansicht = o.ansicht;
            let darf_protokoll = o.zustand.darf_protokoll_lesen();
            let darf_sperren = logik::darf_sperren(&o.zustand);
            let servername = match &o.zustand.verbindung {
                Some(Verbindung::Verbunden { server_name, .. }) => Some(server_name.clone()),
                _ => None,
            };
            let mut wahl_links: Option<Ansicht> = None;
            let mut wahl: Option<Ansicht> = None;
            let mut rechts = Vec::new();
            // Schmal: die Werkzeuge in ein Menü, die Marke weg — Mikrofon und
            // Ton bleiben immer direkt erreichbar.
            let kompakt = ui.available_width() < KOMPAKT;
            let werkzeuge = [
                (
                    Ansicht::Protokoll,
                    Icon::Protokoll,
                    "app.audit",
                    darf_protokoll,
                ),
                (Ansicht::Sperren, Icon::Sperre, "app.bans", darf_sperren),
                (Ansicht::Server, Icon::Leute, "app.menu_servers", true),
                (
                    Ansicht::Einstellungen,
                    Icon::Einstellungen,
                    "app.settings",
                    true,
                ),
            ];
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_SM)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        if !kompakt || servername.is_none() {
                            ui.label(
                                RichText::new("sprechwerk")
                                    .font(egui::FontId::new(thema::FONT_SIZE_LG, fett()))
                                    .color(f.accent_default),
                            );
                            ui.add_space(thema::SPACE_3);
                        }
                        if let Some(name) = &servername {
                            if icon_text_knopf(
                                ui,
                                &f,
                                Icon::Server,
                                name,
                                ansicht == Ansicht::Raeume,
                            )
                            .on_hover_text(t(s, "app.tree"))
                            .clicked()
                            {
                                wahl_links = Some(Ansicht::Raeume);
                            }
                        }
                    },
                    |ui| {
                        // Von rechts nach links.
                        if kompakt {
                            let menue = ui
                                .menu_image_button(
                                    Icon::Mehr.bild(thema::FONT_SIZE_LG, f.text_muted),
                                    |ui| {
                                        for (ziel, bild, schluessel, sichtbar) in werkzeuge {
                                            if sichtbar
                                                && ui
                                                    .add(egui::Button::image_and_text(
                                                        bild.bild(
                                                            thema::FONT_SIZE_MD,
                                                            f.text_muted,
                                                        ),
                                                        t(s, schluessel),
                                                    ))
                                                    .clicked()
                                            {
                                                wahl = Some(ziel);
                                                ui.close();
                                            }
                                        }
                                        if verbunden {
                                            ui.separator();
                                            if ui
                                                .add(egui::Button::image_and_text(
                                                    Icon::Trennen
                                                        .bild(thema::FONT_SIZE_MD, f.status_error),
                                                    t(s, "app.disconnect"),
                                                ))
                                                .clicked()
                                            {
                                                rechts.push(Aktion::Befehl(Befehl::Trennen));
                                                rechts.push(Aktion::AudioStoppen);
                                                ui.close();
                                            }
                                        }
                                    },
                                )
                                .response;
                            let name = t(s, "app.menu");
                            menue.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, &name)
                            });
                        } else {
                            if verbunden
                                && icon_knopf(ui, &f, Icon::Trennen, &t(s, "app.disconnect"), false)
                                    .clicked()
                            {
                                rechts.push(Aktion::Befehl(Befehl::Trennen));
                                rechts.push(Aktion::AudioStoppen);
                            }
                            for (ziel, bild, schluessel, sichtbar) in werkzeuge.iter().rev() {
                                if *sichtbar
                                    && icon_knopf(
                                        ui,
                                        &f,
                                        *bild,
                                        &t(s, schluessel),
                                        ansicht == *ziel,
                                    )
                                    .clicked()
                                {
                                    wahl = Some(*ziel);
                                }
                            }
                        }
                        if !verbunden {
                            return;
                        }
                        if !kompakt {
                            ui.separator();
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
                    },
                );
            if let Some(ziel) = wahl_links.or(wahl) {
                aktionen.extend(o.oeffnen(ziel));
            }
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
            // Eine Zeile, die kürzt statt überzulaufen: links der Stand,
            // rechts eine Meldung — beide mit „…", wenn der Platz fehlt.
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
            let mut zeile = egui::text::LayoutJob::default();
            let format = |farbe| egui::TextFormat {
                font_id: egui::FontId::proportional(thema::FONT_SIZE_SM),
                color: farbe,
                ..Default::default()
            };
            zeile.append(&stand, 0.0, format(farbe));
            if let Some(l) = logik::latenz_text(o.zustand.latenz_ms) {
                zeile.append(&format!(" · {l}"), 0.0, format(f.text_muted));
            }
            if let Some((q, gut)) = logik::qualitaet(s, &o.zustand) {
                let c = if gut { f.status_ok } else { f.status_warn };
                zeile.append(&format!(" · {q}"), 0.0, format(c));
            }
            let meldung = match (&o.hinweis, &o.zustand.meldung) {
                (Some(Hinweis::Fehler(x)), _) => Some((x.clone(), f.status_error)),
                (Some(Hinweis::Info(x)), _) => Some((x.clone(), f.text_muted)),
                (None, Some(x)) => Some((x.clone(), f.status_warn)),
                (None, None) => None,
            };
            egui::Sides::new().shrink_right().truncate().show(
                ui,
                |ui| {
                    ui.add(egui::Label::new(zeile).truncate())
                        .on_hover_text(t(s, "app.latency"));
                },
                |ui| {
                    if let Some((m, c)) = meldung {
                        ui.add(egui::Label::new(RichText::new(&m).color(c)).truncate())
                            .on_hover_text(m);
                    }
                },
            );
        });
}

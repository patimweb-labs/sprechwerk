// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Verhängte Sperren mit Grund und Knopf zum Aufheben.

use super::{knopf, leer, seitenkopf, text, Icon};
use crate::logik;
use crate::modell::Oberflaeche;
use crate::texte::t;
use crate::thema::{self, Ton};
use eframe::egui::{self, RichText};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Befehl;

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    // Die ganze Seite im Seitenrahmen — Überschrift und leere Ansicht
    // eingeschlossen, sonst kleben sie ohne Rand an der Kante.
    sprechwerk_oberflaeche::bausteine::seite(ui, |ui| inhalt(ui, o, aktionen));
}

fn inhalt(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    seitenkopf(ui, o, &t(s, "bans.title"), &t(s, "bans.sub"));
    if !logik::darf_sperren(&o.zustand) {
        ui.label(text(o, t(s, "bans.no_right"), Ton::Warnung));
        return;
    }
    if o.zustand.sperren.is_empty() {
        leer(ui, o, &t(s, "bans.empty.title"), &t(s, "bans.empty.desc"));
        return;
    }
    let f = *o.thema().farben();
    let sperren = o.zustand.sperren.clone();
    sprechwerk_oberflaeche::bausteine::kasten(ui, &f, |ui| {
        for (i, sp) in sperren.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            let (was, art) = match (&sp.kennung, &sp.adresse) {
                (Some(k), _) => (logik::kennung_gruppiert(k, 16), t(s, "bans.kind_key")),
                (None, Some(a)) => (a.clone(), t(s, "bans.kind_address")),
                (None, None) => (String::new(), String::new()),
            };
            let mut aufheben = false;
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_LG)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(&was).monospace());
                            ui.label(
                                text(
                                    o,
                                    format!("{art} · {} · {}", sp.grund, sp.handelnder_name),
                                    Ton::Blass,
                                )
                                .small(),
                            );
                        });
                    },
                    |ui| {
                        aufheben = knopf(ui, &f, Icon::Schliessen, &t(s, "bans.lift")).clicked();
                    },
                );
            if aufheben {
                aktionen.push(Aktion::Befehl(Befehl::Entsperren {
                    kennung: sp.kennung.clone(),
                    adresse: sp.adresse.clone(),
                }));
                aktionen.push(Aktion::Befehl(Befehl::Sperrliste));
            }
        }
    });
}

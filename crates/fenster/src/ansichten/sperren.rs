// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Verhängte Sperren mit Grund und Knopf zum Aufheben.

use super::{leer, seitenkopf, text};
use crate::logik;
use crate::modell::Oberflaeche;
use crate::texte::t;
use crate::thema::{self, Ton};
use eframe::egui::{self, RichText};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Befehl;

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    ui.add_space(thema::SPACE_2);
    seitenkopf(ui, o, &t(s, "bans.title"), &t(s, "bans.sub"));
    if !logik::darf_sperren(&o.zustand) {
        ui.label(text(o, t(s, "bans.no_right"), Ton::Warnung));
        return;
    }
    if o.zustand.sperren.is_empty() {
        leer(ui, o, &t(s, "bans.empty.title"), &t(s, "bans.empty.desc"));
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("sperren")
            .num_columns(5)
            .striped(true)
            .spacing([thema::SPACE_4, thema::SPACE_2])
            .show(ui, |ui| {
                for k in [
                    "bans.col.what",
                    "bans.col.kind",
                    "bans.col.reason",
                    "bans.col.by",
                ] {
                    ui.label(text(o, t(s, k), Ton::Leise).strong());
                }
                ui.label("");
                ui.end_row();
                for sp in o.zustand.sperren.clone() {
                    let (was, art) = match (&sp.kennung, &sp.adresse) {
                        (Some(k), _) => (logik::kennung_gruppiert(k, 16), t(s, "bans.kind_key")),
                        (None, Some(a)) => (a.clone(), t(s, "bans.kind_address")),
                        (None, None) => ("—".into(), String::new()),
                    };
                    ui.label(RichText::new(was).monospace());
                    ui.label(art);
                    ui.label(&sp.grund);
                    ui.label(&sp.handelnder_name);
                    if ui.button(t(s, "bans.lift")).clicked() {
                        aktionen.push(Aktion::Befehl(Befehl::Entsperren {
                            kennung: sp.kennung.clone(),
                            adresse: sp.adresse.clone(),
                        }));
                        aktionen.push(Aktion::Befehl(Befehl::Sperrliste));
                    }
                    ui.end_row();
                }
            });
    });
}

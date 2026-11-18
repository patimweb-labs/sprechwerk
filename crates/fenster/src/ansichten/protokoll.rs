// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Prüfprotokoll: was geschah, wer es tat — und was abgelehnt wurde.

use super::{knopf, leer, seitenkopf, text, Icon};
use crate::logik;
use crate::modell::Oberflaeche;
use crate::texte::t;
use crate::thema::{self, Ton};
use eframe::egui;
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_ipc::Befehl;

pub fn zeigen(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    ui.add_space(thema::SPACE_2);
    seitenkopf(ui, o, &t(s, "audit.title"), &t(s, "audit.sub"));
    if !o.zustand.darf_protokoll_lesen() {
        ui.label(text(o, t(s, "audit.no_right"), Ton::Warnung));
        return;
    }
    if knopf(
        ui,
        o.thema().farben(),
        Icon::Neuladen,
        &t(s, "audit.refresh"),
    )
    .clicked()
    {
        aktionen.push(Aktion::Befehl(Befehl::Protokoll { ab: 0, anzahl: 50 }));
    }
    ui.add_space(thema::SPACE_2);
    if o.zustand.protokoll.is_empty() {
        leer(ui, o, &t(s, "audit.empty.title"), &t(s, "audit.empty.desc"));
        return;
    }
    let versatz = {
        use chrono::Offset;
        chrono::Local::now().offset().fix().local_minus_utc() as i64
    };
    egui::ScrollArea::both().show(ui, |ui| {
        egui::Grid::new("protokoll")
            .num_columns(5)
            .striped(true)
            .spacing([thema::SPACE_4, thema::SPACE_2])
            .show(ui, |ui| {
                for k in [
                    "audit.col.time",
                    "audit.col.who",
                    "audit.col.what",
                    "audit.col.target",
                    "audit.col.result",
                ] {
                    ui.label(text(o, t(s, k), Ton::Leise).strong());
                }
                ui.end_row();
                for e in &o.zustand.protokoll {
                    ui.label(egui::RichText::new(logik::uhrzeit(e.zeit, versatz)).monospace());
                    let wer = if e.handelnder_name.is_empty() {
                        t(s, "audit.system")
                    } else {
                        e.handelnder_name.clone()
                    };
                    ui.label(wer);
                    let schluessel = format!("audit.action.{}", e.aktion);
                    let was = t(s, &schluessel);
                    let was = if was == schluessel {
                        e.aktion.clone()
                    } else {
                        was
                    };
                    ui.label(was);
                    let mut ziel = e.ziel.clone();
                    if let (Some(v), Some(n)) = (&e.vorher, &e.nachher) {
                        ziel.push_str(&format!(" ({v} → {n})"));
                    }
                    if let Some(g) = &e.grund {
                        ziel.push_str(&format!(" · {g}"));
                    }
                    ui.label(ziel);
                    if e.ergebnis.starts_with("abgelehnt") {
                        ui.label(text(o, &e.ergebnis, Ton::Fehler));
                    } else {
                        ui.label(text(o, &e.ergebnis, Ton::Gut));
                    }
                    ui.end_row();
                }
            });
    });
}

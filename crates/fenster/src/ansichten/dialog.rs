// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der eine offene Dialog. Was er zeigt, folgt aus `Dialog` im Modell.

use super::{hauptknopf, text};
use crate::modell::{Dialog, Oberflaeche};
use crate::texte::{rolle, t, tf};
use crate::thema::{self, Ton};
use eframe::egui;
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_domain::Role;

pub fn zeigen(ctx: &egui::Context, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    if o.dialog == Dialog::Keiner {
        // Der nächste Dialog bekommt den Fokus wieder ins erste Feld.
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::unique("dialog-fokus")));
        return;
    }
    let s = o.sprache();
    let f = *o.thema().farben();
    let raeume = o.zustand.raeume.clone();
    let (titel, erklaerung, bestaetigen) = match &o.dialog {
        Dialog::Keiner => return,
        Dialog::ServerHinzufuegen { .. } => (
            t(s, "server.add.title"),
            t(s, "server.add.desc"),
            t(s, "server.add"),
        ),
        Dialog::ServerEntfernen { .. } => (
            t(s, "server.remove.title"),
            t(s, "server.remove.desc"),
            t(s, "server.remove.confirm"),
        ),
        Dialog::RaumAnlegen { .. } => (
            t(s, "room.add.title"),
            t(s, "room.add.desc"),
            t(s, "room.add.confirm"),
        ),
        Dialog::RaumUmbenennen { .. } => (
            t(s, "room.rename.title"),
            String::new(),
            t(s, "room.rename"),
        ),
        Dialog::RaumLoeschen { name, .. } => (
            t(s, "room.delete.title"),
            tf(s, "room.delete.desc", &[name]),
            t(s, "room.delete"),
        ),
        Dialog::Verschieben { name, .. } => (
            tf(s, "member.move.title", &[name]),
            t(s, "member.move.desc"),
            t(s, "member.move.confirm"),
        ),
        Dialog::Rolle { name, .. } => (
            tf(s, "admin.members.role_for", &[name]),
            t(s, "member.role.desc"),
            t(s, "member.role.apply"),
        ),
        Dialog::Trennen { name, .. } => (
            tf(s, "member.kick.title", &[name]),
            t(s, "member.kick.desc"),
            t(s, "member.kick.confirm"),
        ),
        Dialog::Sperren { name, .. } => (
            tf(s, "member.ban.title", &[name]),
            t(s, "member.ban.desc"),
            t(s, "member.ban.confirm"),
        ),
    };
    let leise = text(o, erklaerung, Ton::Leise);
    let mut ok = false;
    let mut abbrechen = false;
    egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        ui.set_width(thema::SPACE_8 * 6.0);
        ui.heading(titel);
        ui.label(leise);
        ui.add_space(thema::SPACE_2);
        let mut feld = |ui: &mut egui::Ui, wert: &mut String, hinweis: String| {
            let a = ui.add(
                egui::TextEdit::singleline(wert)
                    .hint_text(hinweis)
                    .desired_width(f32::INFINITY),
            );
            if a.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            a.request_focus_once(ui);
        };
        match &mut o.dialog {
            Dialog::ServerHinzufuegen { name, adresse } => {
                feld(ui, name, t(s, "server.add.name"));
                feld(ui, adresse, t(s, "server.add.address"));
            }
            Dialog::RaumAnlegen { name } => feld(ui, name, t(s, "room.add.placeholder")),
            Dialog::RaumUmbenennen { name, .. } => feld(ui, name, t(s, "room.rename.placeholder")),
            Dialog::Trennen { grund, .. } => feld(ui, grund, t(s, "member.kick.reason")),
            Dialog::Sperren { grund, .. } => feld(ui, grund, t(s, "member.ban.reason")),
            Dialog::Verschieben { ziel, .. } => {
                for r in &raeume {
                    ui.radio_value(ziel, Some(r.id), &r.name);
                }
            }
            Dialog::Rolle { rolle: wahl, .. } => {
                for r in [
                    Role::Administrator,
                    Role::Moderator,
                    Role::Mitglied,
                    Role::Gast,
                ] {
                    ui.radio_value(wahl, r, rolle(s, r));
                }
            }
            _ => {}
        }
        ui.add_space(thema::SPACE_3);
        ui.horizontal(|ui| {
            if hauptknopf(ui, &f, &bestaetigen).clicked() {
                ok = true;
            }
            if ui.button(t(s, "dialog.cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                abbrechen = true;
            }
        });
    });
    if ok {
        aktionen.extend(o.dialog_bestaetigen());
    } else if abbrechen {
        o.dialog = Dialog::Keiner;
    }
}

/// Fokus beim ersten Bild des Dialogs ins erste Feld.
trait FokusEinmal {
    fn request_focus_once(&self, ui: &egui::Ui);
}

impl FokusEinmal for egui::Response {
    fn request_focus_once(&self, ui: &egui::Ui) {
        let id = egui::Id::unique("dialog-fokus");
        let schon = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        if !schon {
            self.request_focus();
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }
    }
}

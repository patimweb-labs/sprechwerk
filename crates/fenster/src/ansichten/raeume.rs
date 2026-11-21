// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Räume und Chat: links die Räume mit ihren Leuten, rechts der Verlauf des
//! eigenen Raums. Eingeklappt wird der Chat zur schmalen Schiene am rechten
//! Rand, und die Raumliste bekommt die Fläche (gemerkt in den Einstellungen). Eingreifen (stumm, verschieben, sperren) über das Menü am
//! Namen — es erscheint nur, wenn die Rolle es erlaubt (AGENTS.md §2.3).

use super::{icon, icon_knopf, leer, text, Icon};
use crate::app::{Dateiquelle, Zwischenspeicher};
use crate::logik::{self, Stueck, REAKTIONEN};
use crate::modell::{Dialog, Oberflaeche};
use crate::texte::{rolle, rolle_kurz, t, tf, Sprache};
use crate::thema::{self, fett, Ton};
use base64::Engine;
use eframe::egui::{self, RichText, Stroke};
use sprechwerk_client::sitzung::Aktion;
use sprechwerk_domain::MessageId;
use sprechwerk_ipc::{
    Befehl, Dateiansicht, Dateiart, Entwurfsstand, Ladestand, Linkentwurf, Linkvorschau, Mitglied,
    Nachricht, NachrichtInhalt, Textschluessel,
};
use sprechwerk_oberflaeche::bausteine::rollenabzeichen;

pub fn zeigen(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    aktionen: &mut Vec<Aktion>,
) {
    betrachter(ui.ctx(), o, speicher, datei, aktionen);
    let f = *o.thema().farben();
    let s = o.sprache();
    // Rand nur oben: links und rechts setzt die Liste ihn innen, damit der
    // Scrollbalken an der Kante der Leiste sitzt.
    let leiste = egui::Frame::new()
        .fill(f.bg_panel)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin {
            top: thema::SPACE_3 as i8,
            ..Default::default()
        });
    let innen = egui::Margin::symmetric(thema::SPACE_3 as i8, 0);

    if o.einstellungen.chat_eingeklappt {
        // Schmale Schiene rechts: ein Knopf holt den Chat zurück.
        egui::Panel::right("chatschiene")
            .resizable(false)
            .exact_size(thema::SIZE_CONTROL_LG + thema::SPACE_2 * 2.0)
            .frame(
                egui::Frame::new()
                    .fill(f.bg_panel)
                    .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                    .inner_margin(egui::Margin::same(thema::SPACE_2 as i8)),
            )
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if icon_knopf(ui, &f, Icon::Sprechblase, &t(s, "chat.expand"), false).clicked()
                    {
                        o.einstellungen.chat_eingeklappt = false;
                        aktionen.push(o.speichern());
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(leiste)
            .show(ui, |ui| raumliste(ui, o, aktionen, innen));
        return;
    }

    egui::Panel::left("raeume")
        .resizable(true)
        .default_size(thema::SPACE_8 * 4.5)
        .min_size(thema::SPACE_8 * 3.0)
        .frame(leiste)
        .show(ui, |ui| raumliste(ui, o, aktionen, innen));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(f.bg_canvas))
        .show(ui, |ui| chat(ui, o, speicher, datei, aktionen));
}

/// Der Kanalbaum: der Server als Wurzel, darunter die Räume, darunter die
/// Leute — wie bei TeamSpeak. Ein Raum wird mit **Doppelklick** betreten (ein
/// einfacher Klick soll niemanden versehentlich umziehen lassen); über
/// Tastatur oder Screenreader genügt das Auslösen des Eintrags.
fn raumliste(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    aktionen: &mut Vec<Aktion>,
    innen: egui::Margin,
) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let servername = match &o.zustand.verbindung {
        Some(sprechwerk_ipc::Verbindung::Verbunden { server_name, .. }) => server_name.clone(),
        _ => String::new(),
    };
    let mut anlegen = false;
    egui::Frame::new().inner_margin(innen).show(ui, |ui| {
        egui::Sides::new()
            .height(thema::SIZE_CONTROL_SM)
            .shrink_left()
            .truncate()
            .show(
                ui,
                |ui| {
                    ui.add(Icon::Server.bild(thema::FONT_SIZE_LG, f.accent_default));
                    ui.label(
                        RichText::new(&servername)
                            .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                    );
                },
                |ui| {
                    if o.zustand.darf_raeume_verwalten() {
                        anlegen =
                            icon_knopf(ui, &f, Icon::Plus, &t(s, "room.add"), false).clicked();
                    }
                },
            );
    });
    if anlegen {
        o.dialog = Dialog::RaumAnlegen {
            name: String::new(),
        };
    }
    ui.add_space(thema::SPACE_1);
    let raeume = o.zustand.raeume.clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new().inner_margin(innen).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = thema::SPACE_1;
                for raum in &raeume {
                    let zahl = logik::anwesende(&o.zustand, raum.id).count();
                    let zahl = if zahl > 0 {
                        zahl.to_string()
                    } else {
                        String::new()
                    };
                    let breite = ui.available_width();
                    let antwort = ui
                        .horizontal(|ui| {
                            ui.add_space(thema::SPACE_3);
                            ui.add(
                                egui::Button::new(
                                    RichText::new(&raum.name)
                                        .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                                )
                                .selected(raum.hier)
                                .frame_when_inactive(raum.hier)
                                .right_text(RichText::new(zahl).color(f.text_faint))
                                .min_size(egui::vec2(
                                    breite - thema::SPACE_3,
                                    thema::SIZE_CONTROL_SM,
                                )),
                            )
                        })
                        .inner
                        .on_hover_text(t(s, "room.join_hint"));
                    // Doppelklick mit der Maus — oder Auslösen ohne Maus.
                    let ohne_maus = antwort.clicked() && !ui.input(|i| i.pointer.any_click());
                    if (antwort.double_clicked() || ohne_maus) && !raum.hier {
                        aktionen.push(Aktion::Befehl(Befehl::Betreten { raum: raum.id }));
                    }
                    antwort.context_menu(|ui| {
                        if !raum.hier && ui.button(t(s, "room.enter")).clicked() {
                            aktionen.push(Aktion::Befehl(Befehl::Betreten { raum: raum.id }));
                            ui.close();
                        }
                        if o.zustand.darf_raeume_verwalten() {
                            if ui.button(t(s, "room.rename")).clicked() {
                                o.dialog = Dialog::RaumUmbenennen {
                                    raum: raum.id,
                                    name: raum.name.clone(),
                                };
                                ui.close();
                            }
                            if ui.button(t(s, "room.delete")).clicked() {
                                o.dialog = Dialog::RaumLoeschen {
                                    raum: raum.id,
                                    name: raum.name.clone(),
                                };
                                ui.close();
                            }
                        }
                    });
                    if raum.hier && logik::schluessel_vorhanden(&o.zustand, raum.id) == Some(false)
                    {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(thema::SPACE_6);
                            icon(ui, &f, Icon::Warnung, Ton::Warnung);
                            ui.label(text(o, t(s, "room.key_missing"), Ton::Warnung).small());
                            if ui.small_button(t(s, "room.key_request")).clicked() {
                                aktionen.push(Aktion::Befehl(Befehl::SchluesselAnfordern {
                                    raum: raum.id,
                                }));
                            }
                        });
                    }
                    let leute: Vec<Mitglied> =
                        logik::anwesende(&o.zustand, raum.id).cloned().collect();
                    for m in &leute {
                        person(ui, o, m, aktionen);
                    }
                }
            })
        });
}

/// Das Sprechlicht neben dem Namen, wie bei TeamSpeak: ein Quadrat in der
/// Farbe des Sprechsignals (Orange, ADR 0015), das aufleuchtet, solange jemand
/// spricht (und damit mit der Stimme flackert).
/// Stumm und taub zeigen statt dessen ihr Icon — das ist wichtiger.
fn sprechlicht(ui: &mut egui::Ui, f: &thema::Farben, status: logik::Personenstatus) {
    let groesse = thema::FONT_SIZE_MD;
    match status {
        logik::Personenstatus::Stumm => {
            ui.add(Icon::MikrofonAus.bild(groesse, f.status_error));
        }
        logik::Personenstatus::Taub => {
            ui.add(Icon::KopfhoererAus.bild(groesse, f.status_error));
        }
        logik::Personenstatus::Spricht | logik::Personenstatus::Still => {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(groesse, groesse), egui::Sense::hover());
            let quadrat = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(thema::SPACE_3, thema::SPACE_3),
            );
            if status == logik::Personenstatus::Spricht {
                ui.painter().rect_filled(quadrat, 0.0, f.signal_default);
            } else {
                ui.painter().rect_stroke(
                    quadrat,
                    0.0,
                    Stroke::new(thema::SIZE_BORDER, f.border_strong),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

/// Eine Person im Baum: Name, dann in festen Spalten Sprechlicht (spricht,
/// still, stumm, taub) und Rolle. Das Menü zum Eingreifen nur mit Recht und nie an sich selbst.
fn person(ui: &mut egui::Ui, o: &mut Oberflaeche, m: &Mitglied, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let selbst = m.ich.then(|| {
        logik::ich_spreche(
            &o.audio,
            o.einstellungen.schwelle,
            o.einstellungen.push_to_talk,
            o.spricht,
        )
    });
    let status = logik::personenstatus(m, selbst);
    let zustand = match status {
        logik::Personenstatus::Spricht => t(s, "member.speaking"),
        logik::Personenstatus::Still => String::new(),
        logik::Personenstatus::Stumm => t(s, "state.muted"),
        logik::Personenstatus::Taub => t(s, "state.deaf"),
    };
    let breite = ui.available_width();
    let antwort = ui
        .horizontal(|ui| {
            ui.set_min_width(breite);
            // Bündig mit den Raumnamen (Abstand vor dem Raumknopf plus dessen
            // Innenrand): Fettschrift und Abzeichen grenzen genug ab,
            // Einrücken kostete nur Platz für den Namen.
            ui.add_space(thema::SPACE_3 + ui.spacing().button_padding.x);
            let name = if m.ich {
                RichText::new(format!("{} ({})", m.name, t(s, "chat.you"))).color(f.accent_default)
            } else {
                RichText::new(&m.name)
            };
            // Feste Spalten von rechts: Rollenabzeichen, Sprechlicht — bei
            // jeder Person an derselben Stelle (auch ohne Abzeichen). Der
            // Name nimmt den Rest und wird gekürzt.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(thema::SPACE_2);
                rollenabzeichen(ui, &f, m.rolle, &rolle_kurz(s, m.rolle), &rolle(s, m.rolle));
                ui.add_space(thema::SPACE_2);
                sprechlicht(ui, &f, status);
                ui.add_space(thema::SPACE_2);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(name).truncate());
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    let hinweis = if zustand.is_empty() {
        rolle(s, m.rolle)
    } else {
        format!("{} · {zustand}", rolle(s, m.rolle))
    };
    let antwort = antwort.on_hover_text(hinweis);
    if !m.ich && logik::darf_eingreifen(&o.zustand) {
        antwort.context_menu(|ui| mitgliedmenue(ui, o, m, aktionen));
    }
}

fn mitgliedmenue(ui: &mut egui::Ui, o: &mut Oberflaeche, m: &Mitglied, aktionen: &mut Vec<Aktion>) {
    let s = o.sprache();
    let befehl = |b: Befehl| Aktion::Befehl(b);
    let mik = if m.stumm {
        "member.unmute"
    } else {
        "member.mute"
    };
    if ui.button(t(s, mik)).clicked() {
        aktionen.push(befehl(Befehl::Stummschalten {
            mitglied: m.id,
            taub: false,
            an: !m.stumm,
        }));
        ui.close();
    }
    let ton = if m.taub {
        "member.undeaf"
    } else {
        "member.deaf"
    };
    if ui.button(t(s, ton)).clicked() {
        aktionen.push(befehl(Befehl::Stummschalten {
            mitglied: m.id,
            taub: true,
            an: !m.taub,
        }));
        ui.close();
    }
    ui.separator();
    if ui.button(t(s, "member.move")).clicked() {
        o.dialog = Dialog::Verschieben {
            mitglied: m.id,
            name: m.name.clone(),
            ziel: None,
        };
        ui.close();
    }
    if ui.button(t(s, "member.kick_room")).clicked() {
        aktionen.push(befehl(Befehl::AusRaumWerfen { mitglied: m.id }));
        ui.close();
    }
    if o.zustand.darf_rollen_vergeben() && ui.button(t(s, "member.role")).clicked() {
        o.dialog = Dialog::Rolle {
            mitglied: m.id,
            name: m.name.clone(),
            rolle: m.rolle,
        };
        ui.close();
    }
    ui.separator();
    if ui.button(t(s, "member.kick")).clicked() {
        o.dialog = Dialog::Trennen {
            mitglied: m.id,
            name: m.name.clone(),
            grund: String::new(),
        };
        ui.close();
    }
    if logik::darf_sperren(&o.zustand) && ui.button(t(s, "member.ban")).clicked() {
        o.dialog = Dialog::Sperren {
            mitglied: m.id,
            name: m.name.clone(),
            grund: String::new(),
        };
        ui.close();
    }
}

fn chat(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let Some(raum) = o.zustand.raeume.iter().find(|r| r.hier).cloned() else {
        leer(ui, o, &t(s, "room.none"), &t(s, "room.choose"));
        return;
    };

    // Kopf
    egui::Panel::top("chatkopf")
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
            // Der Hinweis links wird gekürzt, der Knopf rechts bleibt immer da.
            let mut einklappen = false;
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_SM)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.label(
                            RichText::new(&raum.name)
                                .font(egui::FontId::new(thema::FONT_SIZE_LG, fett())),
                        );
                        icon(ui, &f, Icon::Schloss, Ton::Blass).on_hover_text(t(s, "chat.e2e"));
                        ui.label(text(o, t(s, "chat.e2e"), Ton::Blass).small());
                    },
                    |ui| {
                        einklappen =
                            icon_knopf(ui, &f, Icon::PanelZu, &t(s, "chat.collapse"), false)
                                .clicked();
                    },
                );
            if einklappen {
                o.einstellungen.chat_eingeklappt = true;
                aktionen.push(o.speichern());
            }
            if o.zustand.textschluessel == Textschluessel::Wartet {
                ui.label(text(o, t(s, "chat.key_wait"), Ton::Warnung));
            }
        });

    // Eingabe
    egui::Panel::bottom("eingabe")
        .frame(
            egui::Frame::new()
                .fill(f.bg_panel)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::same(thema::SPACE_3 as i8)),
        )
        .show(ui, |ui| eingabe(ui, o, speicher, aktionen));

    // Verlauf
    // Ohne Rand am Panel: der Scrollbalken sitzt am Rand, der Abstand liegt
    // innen um die Beiträge.
    let innen = egui::Margin::same(thema::SPACE_4 as i8);
    egui::CentralPanel::default()
        .frame(egui::Frame::new())
        .show(ui, |ui| {
            let nachrichten: Vec<Nachricht> =
                logik::sichtbar(&o.zustand.nachrichten).cloned().collect();
            if nachrichten.is_empty() {
                egui::Frame::new().inner_margin(innen).show(ui, |ui| {
                    leer(ui, o, &t(s, "room.empty.title"), &t(s, "room.empty.desc"));
                });
                return;
            }
            let versatz = ortszeit_versatz();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new().inner_margin(innen).show(ui, |ui| {
                        let mut vorher: Option<&Nachricht> = None;
                        for n in &nachrichten {
                            let kopf = logik::neuer_block(vorher, n);
                            if kopf && vorher.is_some() {
                                ui.add_space(thema::SPACE_3);
                            }
                            beitrag(ui, o, s, n, kopf, versatz, speicher, datei, aktionen);
                            vorher = Some(n);
                        }
                    })
                });
        });
}

#[allow(clippy::too_many_arguments)]
fn beitrag(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    s: Sprache,
    n: &Nachricht,
    kopf: bool,
    versatz: i64,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    aktionen: &mut Vec<Aktion>,
) {
    let f = *o.thema().farben();
    let ich = Some(n.verfasser) == o.zustand.ich();
    let antwort = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = thema::SPACE_1;
            if kopf {
                ui.horizontal(|ui| {
                    let name = if ich {
                        format!("{} ({})", n.verfasser_name, t(s, "chat.you"))
                    } else {
                        n.verfasser_name.clone()
                    };
                    let farbe = if ich {
                        f.accent_default
                    } else {
                        f.text_default
                    };
                    ui.label(
                        RichText::new(name)
                            .font(egui::FontId::new(thema::FONT_SIZE_MD, fett()))
                            .color(farbe),
                    );
                    ui.label(text(o, logik::uhrzeit(n.zeit, versatz), Ton::Blass).small());
                });
            }
            match &n.inhalt {
                NachrichtInhalt::Text {
                    text: inhalt,
                    vorschau,
                } => {
                    textzeile(ui, inhalt);
                    if let Some(v) = vorschau {
                        vorschaukarte(ui, o, v, speicher, None);
                    }
                }
                NachrichtInhalt::Anhang {
                    text: inhalt,
                    dateien,
                } => {
                    if !inhalt.is_empty() {
                        textzeile(ui, inhalt);
                    }
                    ui.horizontal_wrapped(|ui| {
                        for d in dateien {
                            dateikachel(ui, o, n.id, d, speicher, datei, aktionen);
                        }
                    });
                }
                NachrichtInhalt::Wartet => {
                    ui.label(text(o, t(s, "chat.waiting"), Ton::Blass).italics());
                }
                NachrichtInhalt::Unlesbar => {
                    ui.label(text(o, t(s, "chat.unreadable"), Ton::Warnung));
                }
                NachrichtInhalt::Neuer { art } => {
                    ui.label(text(o, tf(s, "chat.newer", &[art]), Ton::Blass));
                }
                NachrichtInhalt::Geloescht => {
                    ui.label(text(o, t(s, "chat.deleted"), Ton::Blass).italics());
                }
                NachrichtInhalt::Reaktion { .. } => {}
            }
            if !n.reaktionen.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for r in &n.reaktionen {
                        let beschriftung =
                            format!("{} {}", logik::darstellbar(&r.zeichen), r.anzahl);
                        let hinweis = tf(
                            s,
                            "react.count",
                            &[
                                &t(s, &format!("react.{}", r.zeichen)),
                                &r.namen.join(", "),
                                &r.anzahl,
                            ],
                        );
                        if ui
                            .selectable_label(r.ich, beschriftung)
                            .on_hover_text(hinweis)
                            .clicked()
                        {
                            aktionen.push(Aktion::Befehl(Befehl::Reagieren {
                                nachricht: n.id,
                                zeichen: r.zeichen.clone(),
                            }));
                        }
                    }
                });
            }
        })
        .response
        .interact(egui::Sense::click());

    let lesbar = matches!(
        n.inhalt,
        NachrichtInhalt::Text { .. } | NachrichtInhalt::Anhang { .. }
    );
    if lesbar {
        reaktionsknopf(ui, o, s, n.id, antwort.rect, speicher, aktionen);
    }
    if lesbar || n.loeschbar {
        antwort.context_menu(|ui| {
            if lesbar {
                ui.horizontal(|ui| {
                    for z in REAKTIONEN {
                        if ui
                            .button(logik::darstellbar(z))
                            .on_hover_text(t(s, &format!("react.{z}")))
                            .clicked()
                        {
                            aktionen.push(Aktion::Befehl(Befehl::Reagieren {
                                nachricht: n.id,
                                zeichen: z.to_string(),
                            }));
                            ui.close();
                        }
                    }
                });
                if let NachrichtInhalt::Text { text, .. } = &n.inhalt {
                    if ui
                        .add(egui::Button::image_and_text(
                            Icon::Kopieren.bild(thema::FONT_SIZE_MD, f.text_muted),
                            t(s, "chat.copy"),
                        ))
                        .clicked()
                    {
                        ui.ctx().copy_text(text.clone());
                        ui.close();
                    }
                }
            }
            if n.loeschbar
                && !n.geloescht
                && ui
                    .add(egui::Button::image_and_text(
                        Icon::Papierkorb.bild(thema::FONT_SIZE_MD, f.status_error),
                        t(s, "chat.delete"),
                    ))
                    .clicked()
            {
                aktionen.push(Aktion::Befehl(Befehl::NachrichtLoeschen {
                    nachricht: n.id,
                }));
                ui.close();
            }
        });
    }
}

fn eingabe(
    ui: &mut egui::Ui,
    o: &mut Oberflaeche,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    match o.zustand.entwurf.clone() {
        Linkentwurf::Laedt { .. } => {
            ui.label(text(o, t(s, "preview.loading"), Ton::Blass).small());
        }
        Linkentwurf::Fertig { vorschau } => {
            let mut weg = false;
            vorschaukarte(ui, o, &vorschau, speicher, Some(&mut weg));
            if weg {
                aktionen.push(Aktion::Befehl(Befehl::VorschauVerwerfen));
            }
        }
        Linkentwurf::Keiner | Linkentwurf::Ohne { .. } => {}
    }
    let darf_schreiben =
        o.zustand.textschluessel != Textschluessel::Wartet && o.zustand.meine_rolle().is_some();
    entwurfsdateien(ui, o, aktionen);
    ui.horizontal(|ui| {
        let f = *o.thema().farben();
        if ui
            .add_enabled_ui(darf_schreiben, |ui| {
                icon_knopf(ui, &f, Icon::Anhang, &t(s, "file.attach"), false)
            })
            .inner
            .clicked()
        {
            aktionen.push(Aktion::AnhaengeWaehlen);
        }
        // Rechts neben dem Feld: Emoji und Senden.
        let senden_breite = (thema::SIZE_CONTROL_SM + thema::SPACE_2) * 2.0 + thema::SPACE_2;
        let feld = ui.add_enabled(
            darf_schreiben,
            egui::TextEdit::multiline(&mut o.entwurf)
                .id_salt("chat-eingabe")
                .desired_rows(1)
                .desired_width(ui.available_width() - senden_breite)
                .hint_text(t(s, "chat.placeholder"))
                .return_key(egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                )),
        );
        if feld.changed() {
            aktionen.extend(o.entwurf_geaendert());
        }
        bearbeitungsmenue(&feld, o, s);
        let emoji = ui
            .add_enabled_ui(darf_schreiben, |ui| {
                emoji_knopf(ui, &f, Icon::Smiley, &t(s, "emoji.insert"), s)
            })
            .inner;
        if let Some(e) = emoji {
            einfuegen_am_cursor(ui.ctx(), &feld, &mut o.entwurf, e);
            aktionen.extend(o.entwurf_geaendert());
        }
        let enter =
            feld.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        let knopf = ui
            .add_enabled_ui(darf_schreiben, |ui| {
                icon_knopf(ui, &f, Icon::Senden, &t(s, "chat.send"), false)
            })
            .inner;
        if enter || knopf.clicked() {
            aktionen.extend(o.senden());
            feld.request_focus();
        }
    });
}

fn vorschaukarte(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    v: &Linkvorschau,
    speicher: &mut Zwischenspeicher,
    verwerfen: Option<&mut bool>,
) {
    let f = *o.thema().farben();
    egui::Frame::new()
        .fill(f.bg_raised)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_2 as i8))
        .show(ui, |ui| {
            ui.set_max_width(thema::SPACE_8 * 7.0);
            ui.horizontal(|ui| {
                if let Some(textur) = bild(ui.ctx(), speicher, v) {
                    ui.add(
                        egui::Image::new(&textur)
                            .max_height(thema::SPACE_8)
                            .max_width(thema::SPACE_8 * 1.5),
                    );
                }
                ui.vertical(|ui| {
                    if ui
                        .link(
                            RichText::new(&v.titel)
                                .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                        )
                        .clicked()
                    {
                        oeffne_link(ui.ctx(), &v.url);
                    }
                    if let Some(b) = &v.beschreibung {
                        ui.label(text(o, b, Ton::Leise).small());
                    }
                    ui.label(text(o, &v.url, Ton::Blass).small());
                    if let Some(weg) = verwerfen {
                        if ui.small_button(t(o.sprache(), "preview.remove")).clicked() {
                            *weg = true;
                        }
                    }
                });
            });
        });
}

/// Vorschaubild (JPEG, Base64) einmal dekodieren und als Textur merken.
fn bild(
    ctx: &egui::Context,
    speicher: &mut Zwischenspeicher,
    v: &Linkvorschau,
) -> Option<egui::TextureHandle> {
    let roh = v.bild.as_ref()?;
    speicher
        .bilder
        .entry(v.url.clone())
        .or_insert_with(|| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(roh).ok()?;
            let bild = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let groesse = [bild.width() as usize, bild.height() as usize];
            let farbbild = egui::ColorImage::from_rgba_unmultiplied(groesse, bild.as_raw());
            Some(ctx.load_texture(format!("vorschau:{}", v.url), farbbild, Default::default()))
        })
        .clone()
}

/// Links aus dem Chat kommen von anderen: nur `http(s)`, im Browser des
/// Systems — nie im Fenster, nie ein anderes Programm.
fn oeffne_link(ctx: &egui::Context, url: &str) {
    if sprechwerk_ipc::abbilden::ist_weblink(url) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

/// Versatz der Ortszeit zu UTC in Sekunden.
fn ortszeit_versatz() -> i64 {
    use chrono::Offset;
    chrono::Local::now().offset().fix().local_minus_utc() as i64
}

/// Text mit anklickbaren Links.
fn textzeile(ui: &mut egui::Ui, inhalt: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for stueck in logik::teile(inhalt) {
            match stueck {
                Stueck::Text(x) => {
                    ui.label(logik::darstellbar(&x));
                }
                Stueck::Link(url) => {
                    if ui.link(&url).clicked() {
                        oeffne_link(ui.ctx(), &url);
                    }
                }
            }
        }
    });
}

// ---- Anhänge (ADR 0012 §3) ------------------------------------------------

/// Größte Kante einer Vorschau im Verlauf.
const KACHEL: f32 = thema::SPACE_8 * 4.0;

/// Ein Bild als Textur — aus Bytes, einmal dekodiert und gemerkt. Große
/// Bilder werden fürs Fenster verkleinert (die Datei selbst bleibt, wie sie ist).
fn textur(
    ctx: &egui::Context,
    speicher: &mut Zwischenspeicher,
    schluessel: &str,
    bytes: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<egui::TextureHandle> {
    if let Some(t) = speicher.bilder.get(schluessel) {
        return t.clone();
    }
    let bytes = bytes()?;
    let textur = image::load_from_memory(&bytes).ok().map(|bild| {
        let bild = if bild.width() > 2048 || bild.height() > 2048 {
            bild.thumbnail(2048, 2048)
        } else {
            bild
        };
        let rgba = bild.to_rgba8();
        let groesse = [rgba.width() as usize, rgba.height() as usize];
        let farbbild = egui::ColorImage::from_rgba_unmultiplied(groesse, rgba.as_raw());
        ctx.load_texture(schluessel.to_string(), farbbild, Default::default())
    });
    speicher
        .bilder
        .insert(schluessel.to_string(), textur.clone());
    textur
}

/// Vorschaubild einer Datei (vom Absender, JPEG Base64).
fn dateivorschau(
    ctx: &egui::Context,
    speicher: &mut Zwischenspeicher,
    d: &Dateiansicht,
) -> Option<egui::TextureHandle> {
    let roh = d.vorschau.as_ref()?;
    textur(ctx, speicher, &format!("datei-vorschau:{}", d.id), || {
        base64::engine::general_purpose::STANDARD.decode(roh).ok()
    })
}

/// Das Bild selbst, sobald es geladen ist — sonst die Vorschau.
fn dateibild(
    ctx: &egui::Context,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    d: &Dateiansicht,
) -> Option<egui::TextureHandle> {
    if d.art == Dateiart::Bild && d.stand == Ladestand::Bereit {
        if let Some(t) = textur(ctx, speicher, &format!("datei:{}", d.id), || datei(&d.id)) {
            return Some(t);
        }
    }
    dateivorschau(ctx, speicher, d)
}

/// Größe einer Vorschau: Seitenverhältnis der Datei, höchstens `KACHEL`.
fn kachelgroesse(d: &Dateiansicht, textur: Option<&egui::TextureHandle>) -> egui::Vec2 {
    let (b, h) = match (d.breite, d.hoehe, textur) {
        (Some(b), Some(h), _) if b > 0 && h > 0 => (b as f32, h as f32),
        (_, _, Some(t)) => (t.size()[0] as f32, t.size()[1] as f32),
        // Ohne Maße und ohne Vorschau: eine Fläche im Videoformat.
        _ => return egui::vec2(KACHEL * 0.75, KACHEL * 0.75 * 9.0 / 16.0),
    };
    let faktor = (KACHEL / b.max(h))
        .min(1.0)
        .max(thema::SPACE_8 * 1.5 / b.max(h));
    egui::vec2(b * faktor, h * faktor)
}

fn ladetext(s: Sprache, stand: &Ladestand) -> Option<String> {
    match stand {
        Ladestand::Laedt { prozent } => Some(tf(s, "file.loading", &[prozent])),
        Ladestand::Fehler { text } => Some(tf(s, "file.failed", &[text])),
        Ladestand::Keiner | Ladestand::Bereit => None,
    }
}

/// Eine Datei im Verlauf: Bild als Vorschau (Klick: Betrachter), Video als
/// Kachel mit Abspielzeichen (Klick: Player des Systems), sonst eine Karte
/// mit Speichern. Rechtsklick: speichern oder im Programm öffnen.
#[allow(clippy::too_many_arguments)]
fn dateikachel(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    nachricht: MessageId,
    d: &Dateiansicht,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    aktionen: &mut Vec<Aktion>,
) {
    let s = o.sprache();
    let f = *o.thema().farben();
    let speichern = Aktion::DateiSpeichernUnter {
        nachricht,
        datei: d.id.clone(),
        name: d.name.clone(),
    };
    let extern_ = Aktion::DateiExtern {
        nachricht,
        datei: d.id.clone(),
        name: d.name.clone(),
    };
    let antwort = match d.art {
        Dateiart::Bild | Dateiart::Video => {
            let bild = dateibild(ui.ctx(), speicher, datei, d);
            let groesse = kachelgroesse(d, bild.as_ref());
            let (rect, antwort) = ui.allocate_exact_size(groesse, egui::Sense::click());
            let p = ui.painter();
            p.rect_filled(rect, 0.0, f.bg_raised);
            if let Some(t) = &bild {
                p.image(
                    t.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else if d.art == Dateiart::Bild {
                // Noch kein Bild, keine Vorschau: ein Dateizeichen als Platzhalter.
                let feld = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(thema::SPACE_6, thema::SPACE_6),
                );
                Icon::Datei
                    .bild(thema::SPACE_6, f.text_faint)
                    .paint_at(ui, feld);
            }
            if d.art == Dateiart::Video {
                // Abspielzeichen in der Mitte (gespielt wird im Programm des Systems).
                let feld = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(thema::SIZE_CONTROL_LG, thema::SIZE_CONTROL_LG),
                );
                p.rect_filled(feld, 0.0, f.bg_canvas.gamma_multiply(0.8));
                Icon::Start
                    .bild(thema::FONT_SIZE_XL, f.text_default)
                    .paint_at(ui, feld.shrink(thema::SPACE_2));
            }
            if let Some(x) = ladetext(s, &d.stand) {
                let zeile = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.bottom() - thema::SIZE_CONTROL_SM),
                    rect.max,
                );
                p.rect_filled(zeile, 0.0, f.bg_canvas.gamma_multiply(0.85));
                p.text(
                    zeile.left_center() + egui::vec2(thema::SPACE_2, 0.0),
                    egui::Align2::LEFT_CENTER,
                    x,
                    egui::FontId::proportional(thema::FONT_SIZE_SM),
                    f.text_default,
                );
            }
            let name = if d.art == Dateiart::Video {
                format!("{}: {}", t(s, "file.play"), d.name)
            } else {
                tf(s, "file.open", &[&d.name])
            };
            let antwort =
                antwort.on_hover_text(format!("{} · {}", d.name, logik::groesse_text(d.groesse)));
            antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            if antwort.clicked() {
                aktionen.push(if d.art == Dateiart::Bild {
                    Aktion::Befehl(Befehl::DateiOeffnen {
                        nachricht,
                        datei: d.id.clone(),
                    })
                } else {
                    extern_.clone()
                });
            }
            antwort
        }
        Dateiart::Datei => {
            // Die Karte selbst ist keine Klickfläche — sonst läge sie über dem
            // Knopf; Klick und Kontextmenü hängen am Knopf.
            let mut knopf = None;
            egui::Frame::new()
                .fill(f.bg_raised)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::same(thema::SPACE_2 as i8))
                .show(ui, |ui| {
                    ui.set_max_width(KACHEL);
                    ui.horizontal(|ui| {
                        ui.add(Icon::Datei.bild(thema::SPACE_6, f.text_muted));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&d.name)
                                        .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                                )
                                .truncate(),
                            );
                            let unter = ladetext(s, &d.stand)
                                .unwrap_or_else(|| logik::groesse_text(d.groesse));
                            ui.label(text(o, unter, Ton::Blass).small());
                        });
                        knopf = Some(icon_knopf(
                            ui,
                            &f,
                            Icon::Herunterladen,
                            &tf(s, "file.save_named", &[&d.name]),
                            false,
                        ));
                    });
                });
            let knopf = knopf.expect("Knopf gezeichnet");
            if knopf.clicked() {
                aktionen.push(speichern.clone());
            }
            knopf
        }
    };
    antwort.context_menu(|ui| {
        if ui
            .add(egui::Button::image_and_text(
                Icon::Herunterladen.bild(thema::FONT_SIZE_MD, f.text_muted),
                t(s, "file.save"),
            ))
            .clicked()
        {
            aktionen.push(speichern);
            ui.close();
        }
        if ui
            .add(egui::Button::image_and_text(
                Icon::Extern.bild(thema::FONT_SIZE_MD, f.text_muted),
                t(s, "file.open_system"),
            ))
            .clicked()
        {
            aktionen.push(extern_);
            ui.close();
        }
    });
}

/// Die Dateien im Entwurf, über dem Eingabefeld: Name, Stand, entfernen.
fn entwurfsdateien(ui: &mut egui::Ui, o: &mut Oberflaeche, aktionen: &mut Vec<Aktion>) {
    if o.zustand.anhaenge.is_empty() {
        return;
    }
    let s = o.sprache();
    let f = *o.thema().farben();
    let entwuerfe = o.zustand.anhaenge.clone();
    ui.horizontal_wrapped(|ui| {
        for e in &entwuerfe {
            egui::Frame::new()
                .fill(f.bg_raised)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .inner_margin(egui::Margin::symmetric(
                    thema::SPACE_2 as i8,
                    thema::SPACE_1 as i8,
                ))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let symbol = match e.art {
                            Dateiart::Bild => Icon::Datei,
                            Dateiart::Video => Icon::Film,
                            Dateiart::Datei => Icon::Datei,
                        };
                        ui.add(symbol.bild(thema::FONT_SIZE_MD, f.text_muted));
                        ui.add(egui::Label::new(&e.name).truncate());
                        let (stand, ton) = match &e.stand {
                            Entwurfsstand::Vorbereitet => (t(s, "file.preparing"), Ton::Blass),
                            Entwurfsstand::Laedt { prozent } => {
                                (tf(s, "file.uploading", &[prozent]), Ton::Blass)
                            }
                            Entwurfsstand::Bereit => (t(s, "file.ready"), Ton::Gut),
                            Entwurfsstand::Fehler { text } => {
                                (tf(s, "file.failed", &[text]), Ton::Fehler)
                            }
                        };
                        ui.label(text(o, stand, ton).small());
                        if icon_knopf(
                            ui,
                            &f,
                            Icon::Schliessen,
                            &tf(s, "file.remove", &[&e.name]),
                            false,
                        )
                        .clicked()
                        {
                            aktionen.push(Aktion::Befehl(Befehl::AnhangEntfernen { nr: e.nr }));
                        }
                    });
                });
        }
    });
    if entwuerfe.iter().any(|e| e.stand != Entwurfsstand::Bereit) {
        ui.label(text(o, t(s, "file.wait"), Ton::Blass).small());
    }
    ui.add_space(thema::SPACE_1);
}

/// Der Betrachter: ein Bild groß über dem Fenster. Was offen ist, steht im
/// Modell (`zustand.betrachter`); Esc oder Schließen geben es dem Client zurück.
fn betrachter(
    ctx: &egui::Context,
    o: &Oberflaeche,
    speicher: &mut Zwischenspeicher,
    datei: Dateiquelle,
    aktionen: &mut Vec<Aktion>,
) {
    let Some(b) = o.zustand.betrachter.clone() else {
        return;
    };
    let s = o.sprache();
    let f = *o.thema().farben();
    let d = &b.datei;
    let platz = ctx.content_rect().size() * 0.85;
    let mut zu = false;
    let modal = egui::Modal::new(egui::Id::unique("betrachter")).show(ctx, |ui| {
        ui.set_max_size(platz);
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&d.name).font(egui::FontId::new(thema::FONT_SIZE_LG, fett())),
                    )
                    .truncate(),
                );
                ui.label(text(o, logik::groesse_text(d.groesse), Ton::Blass).small());
            },
            |ui| {
                zu = icon_knopf(ui, &f, Icon::Schliessen, &t(s, "file.close"), false).clicked();
                if icon_knopf(ui, &f, Icon::Extern, &t(s, "file.open_system"), false).clicked() {
                    aktionen.push(Aktion::DateiExtern {
                        nachricht: b.nachricht,
                        datei: d.id.clone(),
                        name: d.name.clone(),
                    });
                }
                if icon_knopf(ui, &f, Icon::Herunterladen, &t(s, "file.save"), false).clicked() {
                    aktionen.push(Aktion::DateiSpeichernUnter {
                        nachricht: b.nachricht,
                        datei: d.id.clone(),
                        name: d.name.clone(),
                    });
                }
            },
        );
        ui.add_space(thema::SPACE_2);
        match dateibild(ctx, speicher, datei, d) {
            Some(t) => {
                let groesse = t.size_vec2();
                let faktor = ((platz.x - thema::SPACE_6) / groesse.x)
                    .min((platz.y - thema::SPACE_8) / groesse.y)
                    .min(1.0);
                ui.add(egui::Image::new(&t).fit_to_exact_size(groesse * faktor));
            }
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(ladetext(s, &d.stand).unwrap_or_default());
                });
            }
        }
        if ladetext(s, &d.stand).is_some() {
            ui.label(text(o, ladetext(s, &d.stand).unwrap_or_default(), Ton::Blass).small());
        }
        // Wird das Bild geladen, solange der Betrachter offen ist, zeichnen.
        if d.stand != Ladestand::Bereit {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
    });
    if zu || modal.should_close() {
        aktionen.push(Aktion::Befehl(Befehl::BetrachterSchliessen));
    }
}

// ---- Emoji, Reaktionen, Zwischenablage -------------------------------------

/// Ein Knopf, der die Emoji-Auswahl öffnet. Liefert das gewählte Emoji.
fn emoji_knopf(
    ui: &mut egui::Ui,
    f: &thema::Farben,
    icon: Icon,
    name: &str,
    s: Sprache,
) -> Option<&'static str> {
    let mut gewaehlt = None;
    let antwort = ui
        .menu_image_button(icon.bild(thema::FONT_SIZE_LG, f.text_muted), |ui| {
            gewaehlt = emoji_raster(ui, s);
            if gewaehlt.is_some() {
                ui.close();
            }
        })
        .response
        .on_hover_text(name);
    let name = name.to_string();
    antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    gewaehlt
}

/// Die Emoji-Auswahl: Gruppen untereinander, Zeichen in Reihen.
fn emoji_raster(ui: &mut egui::Ui, s: Sprache) -> Option<&'static str> {
    let mut gewaehlt = None;
    ui.set_max_width(thema::SPACE_8 * 5.5);
    egui::ScrollArea::vertical()
        .max_height(thema::SPACE_8 * 5.0)
        .show(ui, |ui| {
            for (gruppe, zeichen) in logik::EMOJI {
                ui.label(RichText::new(t(s, gruppe)).small());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(thema::SPACE_1, thema::SPACE_1);
                    for z in zeichen {
                        let knopf = egui::Button::new(
                            RichText::new(logik::darstellbar(z))
                                .font(egui::FontId::proportional(thema::FONT_SIZE_XL)),
                        )
                        .frame(false)
                        .min_size(egui::vec2(thema::SIZE_CONTROL_SM, thema::SIZE_CONTROL_SM));
                        if ui.add(knopf).clicked() {
                            gewaehlt = Some(*z);
                        }
                    }
                });
            }
        });
    gewaehlt
}

/// Fügt ein Emoji an der Cursorstelle des Eingabefelds ein und setzt den
/// Cursor dahinter; ohne bekannte Stelle ans Ende.
fn einfuegen_am_cursor(ctx: &egui::Context, feld: &egui::Response, text: &mut String, neu: &str) {
    let mut zustand = egui::text_edit::TextEditState::load(ctx, feld.id).unwrap_or_default();
    let stelle = zustand
        .cursor
        .char_range()
        .map_or(text.chars().count(), |r| r.primary.index.0);
    let danach = logik::einfuegen(text, stelle, neu);
    zustand
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(danach),
        )));
    zustand.store(ctx, feld.id);
    feld.request_focus();
}

/// Rechtsklick ins Eingabefeld: Ausschneiden, Kopieren, Einfügen, Alles
/// markieren — über die Befehle des Fensters, also mit der echten
/// Zwischenablage des Systems (wie Strg+X/C/V).
fn bearbeitungsmenue(feld: &egui::Response, o: &Oberflaeche, s: Sprache) {
    let ctx = feld.ctx.clone();
    let text = o.entwurf.clone();
    feld.context_menu(|ui| {
        for (schluessel, befehl) in [
            ("edit.cut", egui::ViewportCommand::RequestCut),
            ("edit.copy", egui::ViewportCommand::RequestCopy),
            ("edit.paste", egui::ViewportCommand::RequestPaste),
        ] {
            if ui.button(t(s, schluessel)).clicked() {
                feld.request_focus();
                ctx.send_viewport_cmd(befehl);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(t(s, "edit.select_all")).clicked() {
            let mut zustand =
                egui::text_edit::TextEditState::load(&ctx, feld.id).unwrap_or_default();
            zustand
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
            zustand.store(&ctx, feld.id);
            feld.request_focus();
            ui.close();
        }
    });
}

/// Reagieren: über einer Nachricht erscheint oben rechts ein Knopf; er
/// öffnet die schnellen Reaktionen und darunter alle Emoji. Rechtsklick auf
/// die Nachricht bietet dasselbe.
fn reaktionsknopf(
    ui: &mut egui::Ui,
    o: &Oberflaeche,
    s: Sprache,
    nachricht: MessageId,
    rect: egui::Rect,
    speicher: &mut Zwischenspeicher,
    aktionen: &mut Vec<Aktion>,
) {
    let f = *o.thema().farben();
    let offen = speicher.reaktion == Some(nachricht);
    // Nicht `rect_contains_pointer`: das fragt auch nach der Ebene — liegt
    // die Maus auf dem Knopf (eigene Ebene darüber), verschwände er sonst.
    let ueber = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|p| rect.contains(p) && ui.clip_rect().contains(p));
    if !offen && !ueber {
        return;
    }
    let ctx = ui.ctx().clone();
    let groesse = egui::vec2(thema::SIZE_CONTROL_SM, thema::SIZE_CONTROL_SM);
    let ort = egui::pos2(rect.right() - groesse.x, rect.top());
    let knopf = egui::Area::new(egui::Id::unique(("reagieren", nachricht)))
        .order(egui::Order::Foreground)
        .fixed_pos(ort)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(f.bg_raised)
                .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
                .show(ui, |ui| {
                    icon_knopf(ui, &f, Icon::Reagieren, &t(s, "chat.react"), offen)
                })
                .inner
        })
        .inner;
    if knopf.clicked() {
        speicher.reaktion = if offen { None } else { Some(nachricht) };
    }
    if speicher.reaktion != Some(nachricht) {
        return;
    }
    let auswahl = egui::Area::new(egui::Id::unique(("reaktionen", nachricht)))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(egui::pos2(
            knopf.rect.right(),
            knopf.rect.bottom() + thema::SPACE_1,
        ))
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style())
                .show(ui, |ui| {
                    let mut gewaehlt = None;
                    ui.horizontal(|ui| {
                        for z in REAKTIONEN {
                            let k = egui::Button::new(
                                RichText::new(z)
                                    .font(egui::FontId::proportional(thema::FONT_SIZE_XL)),
                            )
                            .frame(false);
                            if ui
                                .add(k)
                                .on_hover_text(t(s, &format!("react.{z}")))
                                .clicked()
                            {
                                gewaehlt = Some(z);
                            }
                        }
                    });
                    ui.separator();
                    if let Some(z) = emoji_raster(ui, s) {
                        gewaehlt = Some(z);
                    }
                    gewaehlt
                })
                .inner
        });
    if let Some(z) = auswahl.inner {
        aktionen.push(Aktion::Befehl(Befehl::Reagieren {
            nachricht,
            zeichen: z.to_string(),
        }));
        speicher.reaktion = None;
        return;
    }
    // Schließen: Esc oder ein Klick daneben.
    let daneben = ctx.input(|i| {
        i.key_pressed(egui::Key::Escape)
            || (i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !auswahl.response.rect.contains(p) && !knopf.rect.contains(p)))
    });
    if daneben {
        speicher.reaktion = None;
    }
}

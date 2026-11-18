// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der Server mit Fenster (ADR 0013): derselbe Dienst wie `sprechwerk` auf
//! der Konsole, gestartet im eigenen Prozess und verwaltet über dieselbe
//! [`Verwaltung`] — ohne Browser, ohne Steuerkanal-Umweg.
//!
//! Aufbau wie im Client-Fenster: [`Modell`] ist die Wahrheit, [`zeichnen`]
//! ist `render(modell)`, Handlungen sind [`Aktion`]en.

use eframe::egui::{self, RichText, Stroke};
use sprechwerk_domain::Role;
use sprechwerk_oberflaeche::bausteine::{
    hauptknopf_mit, icon_knopf, icon_text_knopf, karte, knopf, leer, seitenkopf, text, Icon,
};
use sprechwerk_oberflaeche::texte::{rolle, t, tf, Sprache};
use sprechwerk_oberflaeche::thema::{self, fett, Thema, Ton};
use sprechwerk_server::konfig::Konfig;
use sprechwerk_server::mitschrift::Zeile;
use sprechwerk_server::verwaltung::{Rollenfehler, Stand, Verwaltung};
use std::path::PathBuf;

/// Läuft der Dienst?
#[derive(Clone, Default)]
pub enum Lauf {
    #[default]
    Gestoppt,
    Startet,
    Laeuft(Verwaltung),
    Fehler(String),
}

impl Lauf {
    pub fn verwaltung(&self) -> Option<&Verwaltung> {
        match self {
            Lauf::Laeuft(v) => Some(v),
            _ => None,
        }
    }
}

/// Welche Seite offen ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Seite {
    #[default]
    Uebersicht,
    Mitglieder,
    Protokoll,
    Einstellungen,
}

/// Einstellungen für den nächsten Start (Entwurf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entwurf {
    pub name: String,
    pub port: u16,
    pub daten: PathBuf,
    pub passwort: String,
    pub standard_rolle: Role,
}

impl Entwurf {
    /// Vorgaben: Datenverzeichnis im Profil des Nutzers, nicht neben dem
    /// Programm (das liegt nach der Installation schreibgeschützt).
    pub fn vorgabe() -> Self {
        Self {
            name: "Sprechwerk".into(),
            port: 7443,
            daten: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("de.weppelmann.Sprechwerk")
                .join("server"),
            passwort: String::new(),
            standard_rolle: Role::Mitglied,
        }
    }

    /// Die Konfiguration für den Dienst. Fehler sind Texte für die Anzeige.
    pub fn konfig(&self) -> Result<Konfig, String> {
        let mut k = Konfig::default();
        k.server.name = self.name.trim().to_string();
        k.server.port = self.port;
        k.server.daten = self.daten.clone();
        k.server.passwort = self.passwort.clone();
        k.server.standard_rolle = self.standard_rolle.text().to_string();
        k.sicherung.verzeichnis = PathBuf::new();
        // Kein zweiter Weg hinein: das Fenster verwaltet selbst.
        k.verwaltung.aktiv = false;
        k.pruefen().map_err(|f| f.to_string())?;
        Ok(k)
    }
}

/// Das ganze Modell des Server-Fensters.
#[derive(Clone)]
pub struct Modell {
    pub lauf: Lauf,
    pub seite: Seite,
    pub entwurf: Entwurf,
    pub stand: Option<Stand>,
    pub zeilen: Vec<Zeile>,
    pub weiter_ab: u64,
    pub meldung: Option<(String, bool)>,
    pub thema: Thema,
    pub sprache: Sprache,
}

impl Default for Modell {
    fn default() -> Self {
        Self {
            lauf: Lauf::Gestoppt,
            seite: Seite::Uebersicht,
            entwurf: Entwurf::vorgabe(),
            stand: None,
            zeilen: Vec::new(),
            weiter_ab: 0,
            meldung: None,
            thema: Thema::Dunkel,
            sprache: Sprache::De,
        }
    }
}

/// Was der Betreiber will.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aktion {
    Starten,
    Anhalten,
    Rolle { mitglied: String, rolle: Role },
}

/// Höchstzahl gezeigter Protokollzeilen (die Mitschrift hält mehr).
const ZEILEN: usize = 2_000;

impl Modell {
    /// Neuen Stand und neue Zeilen aus der laufenden Verwaltung holen.
    pub fn auffrischen(&mut self) {
        let Some(v) = self.lauf.verwaltung() else {
            return;
        };
        self.stand = Some(v.stand());
        let auszug = v.protokoll(self.weiter_ab);
        self.weiter_ab = auszug.weiter_ab;
        self.zeilen.extend(auszug.zeilen);
        let zuviel = self.zeilen.len().saturating_sub(ZEILEN);
        self.zeilen.drain(..zuviel);
    }

    /// Ergebnis einer Rollenänderung in Modell und Meldung.
    pub fn rolle_gesetzt(&mut self, name: &str, ergebnis: Result<Stand, Rollenfehler>) {
        match ergebnis {
            Ok(stand) => {
                self.stand = Some(stand);
                self.meldung = Some((tf(self.sprache, "admin.role.set", &[&name]), true));
            }
            Err(Rollenfehler::LetzterAdministrator { name }) => {
                self.meldung = Some((tf(self.sprache, "admin.role.last", &[&name]), false));
            }
            Err(anders) => self.meldung = Some((anders.to_string(), false)),
        }
    }
}

/// Zeichnet das ganze Fenster. Gibt die Aktionen zurück.
pub fn zeichnen(ui: &mut egui::Ui, m: &mut Modell) -> Vec<Aktion> {
    let mut aktionen = Vec::new();
    let s = m.sprache;
    let f = *m.thema.farben();
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
            // Rechts Zustand und Start/Stopp in fester Breite, links der Rest:
            // nichts schiebt sich übereinander. Schmal: Reiter nur als Icon.
            let breit = ui.available_width() >= thema::SPACE_8 * 12.0;
            let lauf = m.lauf.clone();
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
                        ui.label(text(&f, t(s, "srv.title"), Ton::Leise));
                        ui.add_space(thema::SPACE_3);
                        for (seite, bild, k) in [
                            (Seite::Uebersicht, Icon::Server, "srv.overview"),
                            (Seite::Mitglieder, Icon::Leute, "admin.members.title"),
                            (Seite::Protokoll, Icon::Protokoll, "admin.log.title"),
                            (Seite::Einstellungen, Icon::Einstellungen, "srv.settings"),
                        ] {
                            let name = t(s, k);
                            let antwort = if breit {
                                icon_text_knopf(ui, &f, bild, &name, m.seite == seite)
                            } else {
                                icon_knopf(ui, &f, bild, &name, m.seite == seite)
                            };
                            if antwort.clicked() {
                                m.seite = seite;
                            }
                        }
                    },
                    |ui| match &lauf {
                        Lauf::Gestoppt | Lauf::Fehler(_) => {
                            if hauptknopf_mit(ui, &f, Icon::Start, &t(s, "srv.start")).clicked() {
                                rechts.push(Aktion::Starten);
                            }
                            ui.label(text(&f, t(s, "srv.state.stopped"), Ton::Blass));
                        }
                        Lauf::Startet => {
                            ui.spinner();
                            ui.label(text(&f, t(s, "srv.state.starting"), Ton::Warnung));
                        }
                        Lauf::Laeuft(_) => {
                            if knopf(ui, &f, Icon::Stopp, &t(s, "srv.stop")).clicked() {
                                rechts.push(Aktion::Anhalten);
                            }
                            ui.label(text(&f, t(s, "srv.state.running"), Ton::Gut));
                        }
                    },
                );
            aktionen.extend(rechts);
        });

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
                if let Some(st) = &m.stand {
                    ui.label(text(
                        &f,
                        tf(
                            s,
                            "admin.status.counts",
                            &[&st.verbundene, &st.mitglieder.len()],
                        ),
                        Ton::Leise,
                    ));
                }
                if let Lauf::Fehler(x) = &m.lauf {
                    ui.label(text(&f, x, Ton::Fehler));
                } else if let Some((x, gut)) = &m.meldung {
                    ui.label(text(&f, x, if *gut { Ton::Gut } else { Ton::Fehler }));
                }
            });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(f.bg_canvas)
                .inner_margin(egui::Margin::symmetric(thema::SPACE_5 as i8, 0)),
        )
        .show(ui, |ui| match m.seite {
            Seite::Uebersicht => uebersicht(ui, m),
            Seite::Mitglieder => mitglieder(ui, m, &mut aktionen),
            Seite::Protokoll => protokoll(ui, m),
            Seite::Einstellungen => einstellungen(ui, m),
        });
    aktionen
}

fn uebersicht(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    ui.add_space(thema::SPACE_2);
    seitenkopf(
        ui,
        &f,
        &t(s, "admin.server.title"),
        &t(s, "admin.server.fingerprint_hint"),
    );
    let Some(st) = m.stand.clone() else {
        leer(ui, &f, &t(s, "srv.state.stopped"), &t(s, "srv.not_running"));
        return;
    };
    karte(ui, &f, &st.name, None, |ui| {
        egui::Grid::new("fakten")
            .num_columns(2)
            .spacing([thema::SPACE_4, thema::SPACE_2])
            .show(ui, |ui| {
                let zeit = |ms: i64| uhrzeit(ms);
                let sicherung = match (st.sicherung.aktiv, st.sicherung.letzte) {
                    (false, _) => t(s, "admin.server.backup_off"),
                    (true, None) => t(s, "admin.server.backup_none"),
                    (true, Some(z)) => zeit(z),
                };
                for (k, w) in [
                    ("admin.server.address", st.adresse.clone()),
                    ("admin.server.version", st.version.clone()),
                    ("admin.server.uptime", zeit(st.gestartet)),
                    ("admin.server.connected", st.verbundene.to_string()),
                    ("admin.server.rooms", st.raeume.to_string()),
                    ("admin.server.backup", sicherung),
                ] {
                    ui.label(text(&f, t(s, k), Ton::Leise));
                    ui.label(w);
                    ui.end_row();
                }
            });
    });
    ui.add_space(thema::SPACE_4);
    karte(ui, &f, &t(s, "admin.server.fingerprint"), None, |ui| {
        let fp = gruppiert(&st.fingerabdruck);
        ui.label(RichText::new(&fp).monospace());
        if knopf(ui, &f, Icon::Kopieren, &t(s, "srv.copy")).clicked() {
            ui.ctx().copy_text(fp);
        }
    });
    if st.erster_wird_administrator {
        ui.add_space(thema::SPACE_4);
        karte(
            ui,
            &f,
            &t(s, "admin.first.title"),
            Some(&t(s, "admin.first.desc")),
            |_| {},
        );
    }
}

fn mitglieder(ui: &mut egui::Ui, m: &mut Modell, aktionen: &mut Vec<Aktion>) {
    let s = m.sprache;
    let f = *m.thema.farben();
    ui.add_space(thema::SPACE_2);
    seitenkopf(
        ui,
        &f,
        &t(s, "admin.members.title"),
        &t(s, "admin.members.hint"),
    );
    let Some(st) = m.stand.clone() else {
        leer(ui, &f, &t(s, "srv.state.stopped"), &t(s, "srv.not_running"));
        return;
    };
    if st.mitglieder.is_empty() {
        leer(
            ui,
            &f,
            &t(s, "admin.members.empty"),
            &t(s, "admin.members.empty_desc"),
        );
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("mitglieder")
            .num_columns(4)
            .striped(true)
            .spacing([thema::SPACE_4, thema::SPACE_2])
            .show(ui, |ui| {
                for z in &st.mitglieder {
                    ui.label(
                        RichText::new(&z.name).font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                    );
                    let status = match (&z.verbunden, &z.raum) {
                        (true, Some(r)) => tf(s, "admin.members.in_room", &[r]),
                        (true, None) => t(s, "admin.members.online"),
                        (false, _) => "—".into(),
                    };
                    ui.label(text(
                        &f,
                        status,
                        if z.verbunden { Ton::Gut } else { Ton::Blass },
                    ));
                    ui.label(RichText::new(z.kennung.get(..16).unwrap_or(&z.kennung)).monospace());
                    let mut wahl = z.rolle;
                    egui::ComboBox::from_id_salt(("rolle", z.id))
                        .selected_text(rolle(s, wahl))
                        .show_ui(ui, |ui| {
                            for r in [
                                Role::Administrator,
                                Role::Moderator,
                                Role::Mitglied,
                                Role::Gast,
                            ] {
                                ui.selectable_value(&mut wahl, r, rolle(s, r));
                            }
                        })
                        .response
                        .widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::ComboBox,
                                true,
                                tf(s, "admin.members.role_for", &[&z.name]),
                            )
                        });
                    if wahl != z.rolle {
                        aktionen.push(Aktion::Rolle {
                            mitglied: z.id.text(),
                            rolle: wahl,
                        });
                    }
                    ui.end_row();
                }
            });
    });
}

fn protokoll(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    ui.add_space(thema::SPACE_2);
    seitenkopf(ui, &f, &t(s, "admin.log.title"), &t(s, "admin.log.hint"));
    if m.zeilen.is_empty() {
        leer(ui, &f, &t(s, "admin.log.empty"), "");
        return;
    }
    egui::Frame::new()
        .fill(f.bg_input)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_3 as i8))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for z in &m.zeilen {
                        ui.label(RichText::new(entfaerbt(&z.text)).monospace());
                    }
                });
        });
}

fn einstellungen(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    let frei = matches!(m.lauf, Lauf::Gestoppt | Lauf::Fehler(_));
    ui.add_space(thema::SPACE_2);
    seitenkopf(ui, &f, &t(s, "srv.settings"), &t(s, "srv.settings.sub"));
    karte(ui, &f, &t(s, "admin.server.title"), None, |ui| {
        ui.add_enabled_ui(frei, |ui| {
            egui::Grid::new("einstellungen")
                .num_columns(2)
                .spacing([thema::SPACE_4, thema::SPACE_2])
                .show(ui, |ui| {
                    let l = ui.label(t(s, "admin.server.name"));
                    ui.text_edit_singleline(&mut m.entwurf.name)
                        .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t(s, "srv.port_udp"));
                    ui.add(egui::DragValue::new(&mut m.entwurf.port).range(1..=65535))
                        .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t(s, "srv.password"));
                    ui.add(egui::TextEdit::singleline(&mut m.entwurf.passwort).password(true))
                        .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t(s, "srv.default_role"));
                    let wahl = &mut m.entwurf.standard_rolle;
                    egui::ComboBox::from_id_salt("standard_rolle")
                        .selected_text(rolle(s, *wahl))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(wahl, Role::Mitglied, rolle(s, Role::Mitglied));
                            ui.selectable_value(wahl, Role::Gast, rolle(s, Role::Gast));
                        })
                        .response
                        .labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t(s, "srv.data"));
                    let mut pfad = m.entwurf.daten.display().to_string();
                    if ui
                        .text_edit_singleline(&mut pfad)
                        .labelled_by(l.id)
                        .changed()
                    {
                        m.entwurf.daten = PathBuf::from(pfad);
                    }
                    ui.end_row();
                });
        });
    });
    ui.add_space(thema::SPACE_4);
    karte(ui, &f, &t(s, "set.appearance"), None, |ui| {
        ui.horizontal(|ui| {
            ui.label(t(s, "set.scheme"));
            for th in Thema::ALLE {
                if ui
                    .selectable_label(m.thema == th, t(s, th.text_schluessel()))
                    .clicked()
                {
                    m.thema = th;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(t(s, "set.language"));
            for sp in Sprache::ALLE {
                if ui.selectable_label(m.sprache == sp, sp.anzeige()).clicked() {
                    m.sprache = sp;
                }
            }
        });
    });
}

/// Farbcodes aus der Konsolenausgabe entfernen (die Mitschrift ist dieselbe
/// wie in `docker logs`, dort mit Farben).
pub fn entfaerbt(text: &str) -> String {
    let mut aus = String::with_capacity(text.len());
    let mut zeichen = text.chars().peekable();
    while let Some(c) = zeichen.next() {
        if c == '\u{1b}' && zeichen.peek() == Some(&'[') {
            for d in zeichen.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            aus.push(c);
        }
    }
    aus
}

fn gruppiert(hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(":")
}

fn uhrzeit(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%d.%m. %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farbcodes_verschwinden() {
        assert_eq!(
            entfaerbt("\u{1b}[2m12:00\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m lauscht"),
            "12:00  INFO lauscht"
        );
    }

    #[test]
    fn entwurf_ergibt_gueltige_konfig_ohne_steuerkanal() {
        let mut e = Entwurf::vorgabe();
        e.daten = std::env::temp_dir().join("sprechwerk-server-fenster-test");
        let k = e.konfig().unwrap();
        assert!(!k.verwaltung.aktiv, "das Fenster verwaltet selbst");
        assert_eq!(k.sicherung.verzeichnis, e.daten.join("sicherungen"));
        e.daten = PathBuf::new();
        assert!(e.konfig().is_err());
    }
}

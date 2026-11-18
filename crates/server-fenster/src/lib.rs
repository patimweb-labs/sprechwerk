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
    auswahl, fakten, feld, feldbreite, hauptknopf_mit, icon_knopf, icon_text_knopf, karte, knopf,
    kopierbar, leer, rand, seite, seitenkopf, status, status_klein, text, Icon,
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
            // `SPRECHWERK_DATEN` wie beim Konsolen-Server: so nimmt das Fenster
            // dasselbe Verzeichnis (und dasselbe Zertifikat), wenn man umsteigt.
            daten: std::env::var_os("SPRECHWERK_DATEN")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("de.weppelmann.Sprechwerk")
                        .join("server")
                }),
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
            let breit = ui.available_width() >= thema::SPACE_8 * 10.0;
            // Sehr schmal: der Zustand nur als Quadrat (Text im Hinweis).
            let eng = ui.available_width() < thema::SPACE_8 * 7.0;
            let lauf = m.lauf.clone();
            let mut rechts = Vec::new();
            egui::Sides::new()
                .height(thema::SIZE_CONTROL_SM)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        if breit {
                            ui.label(
                                RichText::new("sprechwerk")
                                    .font(egui::FontId::new(thema::FONT_SIZE_LG, fett()))
                                    .color(f.accent_default),
                            );
                            ui.label(text(&f, t(s, "srv.title"), Ton::Leise));
                            ui.add_space(thema::SPACE_3);
                        }
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
                    |ui| {
                        // Schmal: Start/Stopp nur als Icon, der Zustand als Schild bleibt.
                        let schmal = !breit;
                        match &lauf {
                            Lauf::Gestoppt | Lauf::Fehler(_) => {
                                let name = t(s, "srv.start");
                                let a = if schmal {
                                    icon_knopf(ui, &f, Icon::Start, &name, true)
                                } else {
                                    hauptknopf_mit(ui, &f, Icon::Start, &name)
                                };
                                if a.clicked() {
                                    rechts.push(Aktion::Starten);
                                }
                                zustand(ui, &f, eng, Ton::Blass, &t(s, "srv.state.stopped"));
                            }
                            Lauf::Startet => {
                                ui.spinner();
                                zustand(ui, &f, eng, Ton::Warnung, &t(s, "srv.state.starting"));
                            }
                            Lauf::Laeuft(_) => {
                                let name = t(s, "srv.stop");
                                let a = if schmal {
                                    icon_knopf(ui, &f, Icon::Stopp, &name, false)
                                } else {
                                    knopf(ui, &f, Icon::Stopp, &name)
                                };
                                if a.clicked() {
                                    rechts.push(Aktion::Anhalten);
                                }
                                zustand(ui, &f, eng, Ton::Gut, &t(s, "srv.state.running"));
                            }
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
            ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
            let zahlen = m.stand.as_ref().map(|st| {
                tf(
                    s,
                    "admin.status.counts",
                    &[&st.verbundene, &st.mitglieder.len()],
                )
            });
            let meldung = match (&m.lauf, &m.meldung) {
                (Lauf::Fehler(x), _) => Some((x.clone(), Ton::Fehler)),
                (_, Some((x, gut))) => Some((x.clone(), if *gut { Ton::Gut } else { Ton::Fehler })),
                _ => None,
            };
            egui::Sides::new().shrink_right().truncate().show(
                ui,
                |ui| {
                    if let Some(z) = zahlen {
                        ui.add(egui::Label::new(text(&f, z, Ton::Leise)).truncate());
                    }
                },
                |ui| {
                    if let Some((x, ton)) = meldung {
                        ui.add(egui::Label::new(text(&f, &x, ton)).truncate())
                            .on_hover_text(x);
                    }
                },
            );
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(f.bg_canvas))
        .show(ui, |ui| match m.seite {
            Seite::Uebersicht => uebersicht(ui, m),
            Seite::Mitglieder => mitglieder(ui, m, &mut aktionen),
            Seite::Protokoll => protokoll(ui, m),
            Seite::Einstellungen => einstellungen(ui, m),
        });
    aktionen
}

/// Zustand des Dienstes in der Kopfleiste: mit Text, oder eng nur das Schild.
fn zustand(ui: &mut egui::Ui, f: &thema::Farben, eng: bool, ton: Ton, beschriftung: &str) {
    if eng {
        status_klein(ui, f, ton, beschriftung);
    } else {
        status(ui, f, ton, beschriftung);
    }
}

fn uebersicht(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    seite(ui, |ui| {
        seitenkopf(ui, &f, &t(s, "srv.overview"), &t(s, "srv.overview.sub"));
        let Some(st) = m.stand.clone() else {
            leer(ui, &f, &t(s, "srv.state.stopped"), &t(s, "srv.not_running"));
            return;
        };
        if st.erster_wird_administrator {
            karte(
                ui,
                &f,
                &t(s, "admin.first.title"),
                Some(&t(s, "admin.first.desc")),
                |_| {},
            );
            ui.add_space(thema::SPACE_4);
        }
        karte(ui, &f, &st.name, None, |ui| {
            let sicherung = match (st.sicherung.aktiv, st.sicherung.letzte) {
                (false, _) => t(s, "admin.server.backup_off"),
                (true, None) => t(s, "admin.server.backup_none"),
                (true, Some(z)) => uhrzeit(z),
            };
            fakten(
                ui,
                &f,
                &[
                    (t(s, "admin.server.address"), st.adresse.clone(), true),
                    (t(s, "admin.server.version"), st.version.clone(), false),
                    (t(s, "admin.server.uptime"), uhrzeit(st.gestartet), false),
                    (
                        t(s, "admin.server.connected"),
                        st.verbundene.to_string(),
                        false,
                    ),
                    (t(s, "admin.server.rooms"), st.raeume.to_string(), false),
                    (t(s, "admin.server.backup"), sicherung, false),
                ],
            );
        });
        ui.add_space(thema::SPACE_4);
        karte(
            ui,
            &f,
            &t(s, "admin.server.fingerprint"),
            Some(&t(s, "admin.server.fingerprint_hint")),
            |ui| kopierbar(ui, &f, &gruppiert(&st.fingerabdruck), &t(s, "srv.copy")),
        );
    });
}

fn mitglieder(ui: &mut egui::Ui, m: &mut Modell, aktionen: &mut Vec<Aktion>) {
    let s = m.sprache;
    let f = *m.thema.farben();
    seite(ui, |ui| {
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
        karte(
            ui,
            &f,
            &tf(
                s,
                "admin.status.counts",
                &[&st.verbundene, &st.mitglieder.len()],
            ),
            None,
            |ui| {
                for (i, z) in st.mitglieder.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    let mut wahl = z.rolle;
                    egui::Sides::new()
                        .height(thema::SIZE_CONTROL_LG)
                        .shrink_left()
                        .truncate()
                        .show(
                            ui,
                            |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(
                                        RichText::new(&z.name)
                                            .font(egui::FontId::new(thema::FONT_SIZE_MD, fett())),
                                    );
                                    let ort = match (&z.verbunden, &z.raum) {
                                        (true, Some(r)) => tf(s, "admin.members.in_room", &[r]),
                                        (true, None) => t(s, "admin.members.online"),
                                        (false, _) => "—".into(),
                                    };
                                    let kennung = z.kennung.get(..16).unwrap_or(&z.kennung);
                                    ui.label(
                                        text(
                                            &f,
                                            format!("{ort} · {kennung}"),
                                            if z.verbunden { Ton::Gut } else { Ton::Blass },
                                        )
                                        .small(),
                                    );
                                });
                            },
                            |ui| {
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
                            },
                        );
                    if wahl != z.rolle {
                        aktionen.push(Aktion::Rolle {
                            mitglied: z.id.text(),
                            rolle: wahl,
                        });
                    }
                }
            },
        );
    });
}

fn protokoll(ui: &mut egui::Ui, m: &mut Modell) {
    rand(ui, |ui| protokoll_inhalt(ui, m));
}

fn protokoll_inhalt(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    ui.add_space(thema::SPACE_2);
    seitenkopf(ui, &f, &t(s, "admin.log.title"), &t(s, "admin.log.hint"));
    if m.zeilen.is_empty() {
        leer(ui, &f, &t(s, "admin.log.empty"), "");
        return;
    }
    // Zeilen brechen um — so verschwindet auch im schmalen Fenster nichts.
    egui::Frame::new()
        .fill(f.bg_input)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_3 as i8))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = thema::SPACE_1;
                    for z in &m.zeilen {
                        ui.add(
                            egui::Label::new(RichText::new(entfaerbt(&z.text)).monospace()).wrap(),
                        );
                    }
                });
        });
}

fn einstellungen(ui: &mut egui::Ui, m: &mut Modell) {
    let s = m.sprache;
    let f = *m.thema.farben();
    let frei = matches!(m.lauf, Lauf::Gestoppt | Lauf::Fehler(_));
    seite(ui, |ui| {
        seitenkopf(ui, &f, &t(s, "srv.settings"), &t(s, "srv.settings.sub"));
        karte(ui, &f, &t(s, "admin.server.title"), None, |ui| {
            ui.add_enabled_ui(frei, |ui| {
                feld(ui, &f, &t(s, "admin.server.name"), None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut m.entwurf.name)
                            .desired_width(feldbreite(ui)),
                    )
                });
                feld(ui, &f, &t(s, "srv.port_udp"), None, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut m.entwurf.port)
                            .range(1..=65535)
                            .speed(0.0),
                    )
                });
                feld(ui, &f, &t(s, "srv.password"), None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut m.entwurf.passwort)
                            .password(true)
                            .desired_width(feldbreite(ui)),
                    )
                });
                feld(ui, &f, &t(s, "srv.default_role"), None, |ui| {
                    let wahl = &mut m.entwurf.standard_rolle;
                    egui::ComboBox::from_id_salt("standard_rolle")
                        .selected_text(rolle(s, *wahl))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(wahl, Role::Mitglied, rolle(s, Role::Mitglied));
                            ui.selectable_value(wahl, Role::Gast, rolle(s, Role::Gast));
                        })
                        .response
                });
                feld(ui, &f, &t(s, "srv.data"), None, |ui| {
                    let mut pfad = m.entwurf.daten.display().to_string();
                    let a = ui.add(
                        egui::TextEdit::singleline(&mut pfad).desired_width(ui.available_width()),
                    );
                    if a.changed() {
                        m.entwurf.daten = PathBuf::from(pfad);
                    }
                    a
                });
            });
        });
        ui.add_space(thema::SPACE_4);
        karte(ui, &f, &t(s, "set.appearance"), None, |ui| {
            feld(ui, &f, &t(s, "set.scheme"), None, |ui| {
                let moeglich: Vec<(Thema, String)> = Thema::ALLE
                    .iter()
                    .map(|th| (*th, t(s, th.text_schluessel())))
                    .collect();
                ui.horizontal_wrapped(|ui| auswahl(ui, &mut m.thema, &moeglich))
                    .response
            });
            feld(ui, &f, &t(s, "set.language"), None, |ui| {
                let moeglich: Vec<(Sprache, String)> = Sprache::ALLE
                    .iter()
                    .map(|sp| (*sp, sp.anzeige().to_string()))
                    .collect();
                ui.horizontal_wrapped(|ui| auswahl(ui, &mut m.sprache, &moeglich))
                    .response
            });
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

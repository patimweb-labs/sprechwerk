// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Bausteine beider Fenster: Karte, Seitenkopf, Pegelbalken, Hauptknopf,
//! leere Fläche, Kürzel. Sie bekommen Farben aus dem Theme, nie Werte.

use crate::thema::{self, fett, Farben, Thema, Ton};
use egui::{Color32, RichText, Sense, Stroke};

/// Text in einer Bedeutungsfarbe.
pub fn text(f: &Farben, inhalt: impl Into<String>, art: Ton) -> RichText {
    RichText::new(inhalt).color(thema::ton(f, art))
}

/// Überschrift einer Seite mit Unterzeile.
pub fn seitenkopf(ui: &mut egui::Ui, f: &Farben, titel: &str, unter: &str) {
    ui.add_space(thema::SPACE_4);
    ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_2XL, fett())));
    ui.label(text(f, unter, Ton::Leise));
    ui.add_space(thema::SPACE_4);
}

/// Eine Karte: Rahmen, Titel, Inhalt.
pub fn karte<R>(
    ui: &mut egui::Ui,
    f: &Farben,
    titel: &str,
    unter: Option<&str>,
    inhalt: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(f.bg_panel)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_LG, fett())));
            if let Some(u) = unter {
                ui.label(RichText::new(u).color(f.text_muted));
            }
            ui.add_space(thema::SPACE_2);
            inhalt(ui)
        })
        .inner
}

/// Schmaler Pegelbalken 0–100 mit optionaler Marke (Sprechschwelle).
/// Über der Schwelle leuchtet er im Sprechsignal (Orange, ADR 0015).
pub fn pegelbalken(ui: &mut egui::Ui, f: &Farben, pegel: u8, marke: Option<u8>, breite: f32) {
    let (rect, antwort) =
        ui.allocate_exact_size(egui::vec2(breite, thema::SPACE_2), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, f.bg_input);
    let mut voll = rect;
    voll.set_width(rect.width() * pegel.min(100) as f32 / 100.0);
    let farbe = match marke {
        Some(m) if pegel < m => f.text_faint,
        _ => f.signal_default,
    };
    p.rect_filled(voll, 0.0, farbe);
    if let Some(m) = marke {
        let x = rect.left() + rect.width() * m.min(100) as f32 / 100.0;
        p.line_segment(
            [
                egui::pos2(x, rect.top() - thema::SIZE_FOCUS),
                egui::pos2(x, rect.bottom() + thema::SIZE_FOCUS),
            ],
            Stroke::new(thema::SIZE_BORDER, f.text_default),
        );
    }
    antwort.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, format!("{pegel} %"))
    });
}

/// Wert 0–100 aus einer waagerechten Position im Balken (rein, für Tests).
pub fn wert_aus_position(x: f32, links: f32, breite: f32) -> u8 {
    if breite <= 0.0 {
        return 0;
    }
    (((x - links) / breite).clamp(0.0, 1.0) * 100.0).round() as u8
}

/// Pegel und Sprechschwelle in einem: der Balken zeigt den Pegel live, die
/// Marke ist die Schwelle — ziehen oder klicken setzt sie, die Pfeiltasten
/// auch. Unter der Schwelle ist der Balken blass, darüber in der
/// Akzentfarbe: man sieht sofort, ob die Stimme durchkommt. Ohne Schwelle
/// (`None`, Push-to-Talk) ist es nur ein Pegel.
pub fn pegelregler(
    ui: &mut egui::Ui,
    f: &Farben,
    pegel: u8,
    schwelle: Option<&mut u8>,
    breite: f32,
    name: &str,
) -> egui::Response {
    let hoehe = thema::SPACE_4;
    let sinn = if schwelle.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, mut antwort) = ui.allocate_exact_size(egui::vec2(breite, hoehe), sinn);
    let mut marke = None;
    if let Some(s) = schwelle {
        if let Some(pos) = antwort.interact_pointer_pos() {
            let neu = wert_aus_position(pos.x, rect.left(), rect.width());
            if neu != *s {
                *s = neu;
                antwort.mark_changed();
            }
        }
        if antwort.has_focus() {
            let (links, rechts) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                )
            });
            if links && *s > 0 {
                *s -= 1;
                antwort.mark_changed();
            }
            if rechts && *s < 100 {
                *s += 1;
                antwort.mark_changed();
            }
        }
        marke = Some(*s);
    }
    let p = ui.painter();
    p.rect_filled(rect, 0.0, f.bg_input);
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(thema::SIZE_BORDER, f.border_subtle),
        egui::StrokeKind::Inside,
    );
    let mut voll = rect.shrink(thema::SIZE_BORDER);
    voll.set_width(voll.width() * pegel.min(100) as f32 / 100.0);
    let farbe = match marke {
        Some(m) if pegel < m => f.text_faint,
        _ => f.signal_default,
    };
    p.rect_filled(voll, 0.0, farbe);
    if let Some(m) = marke {
        let x = rect.left() + rect.width() * m.min(100) as f32 / 100.0;
        let linie = if antwort.hovered() || antwort.dragged() || antwort.has_focus() {
            thema::SIZE_FOCUS
        } else {
            thema::SIZE_BORDER
        };
        p.line_segment(
            [
                egui::pos2(x, rect.top() - thema::SPACE_1),
                egui::pos2(x, rect.bottom() + thema::SPACE_1),
            ],
            Stroke::new(linie, f.text_default),
        );
        if antwort.has_focus() {
            p.rect_stroke(
                rect.expand(thema::SIZE_FOCUS),
                0.0,
                Stroke::new(thema::SIZE_FOCUS, f.focus_ring),
                egui::StrokeKind::Outside,
            );
        }
    }
    let name = name.to_string();
    match marke {
        Some(m) => antwort.widget_info(|| egui::WidgetInfo::slider(true, m as f64, &name)),
        None => antwort.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::ProgressIndicator,
                true,
                format!("{name}: {pegel} %"),
            )
        }),
    }
    if marke.is_some() {
        antwort = antwort.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    }
    antwort
}

/// Farben des Rollenabzeichens (Fläche, Schrift). Mitglieder — die Regel —
/// haben keins: so fällt auf, wer mehr (oder weniger) darf.
pub fn rollenfarben(f: &Farben, rolle: sprechwerk_domain::Role) -> Option<(Color32, Color32)> {
    use sprechwerk_domain::Role;
    match rolle {
        Role::Administrator => Some((f.accent_default, f.text_on_accent)),
        Role::Moderator => Some((f.status_ok, f.text_on_accent)),
        Role::Gast => Some((f.bg_selected, f.text_muted)),
        Role::Mitglied => None,
    }
}

/// Seitenlänge eines Rollenabzeichens.
pub const ABZEICHEN: f32 = thema::SPACE_4;

/// Quadratisches Abzeichen mit dem Anfangsbuchstaben der Rolle (A, M, G).
/// Nimmt immer denselben Platz ein — auch ohne Abzeichen —, damit die
/// Spalten im Baum bündig bleiben. `name` ist Hinweis und zugänglicher Name.
pub fn rollenabzeichen(
    ui: &mut egui::Ui,
    f: &Farben,
    rolle: sprechwerk_domain::Role,
    kurz: &str,
    name: &str,
) -> egui::Response {
    let (rect, antwort) = ui.allocate_exact_size(egui::vec2(ABZEICHEN, ABZEICHEN), Sense::hover());
    let Some((flaeche, schrift)) = rollenfarben(f, rolle) else {
        return antwort;
    };
    ui.painter().rect_filled(rect, 0.0, flaeche);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        kurz,
        egui::FontId::new(thema::FONT_SIZE_XS, fett()),
        schrift,
    );
    let name = name.to_string();
    antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    antwort.on_hover_text(name)
}

/// Hauptschaltfläche (Akzent).
pub fn hauptknopf(ui: &mut egui::Ui, f: &Farben, beschriftung: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(beschriftung).color(f.text_on_accent))
            .fill(f.accent_default)
            .stroke(Stroke::new(thema::SIZE_BORDER, f.accent_default)),
    )
}

/// Eine leere Fläche mit Titel und Erklärung.
pub fn leer(ui: &mut egui::Ui, f: &Farben, titel: &str, erklaerung: &str) {
    // Linksbündig wie die Karten der Seite — nicht mittig in einer Fläche,
    // in der alles andere links steht.
    kasten(ui, f, |ui| {
        ui.label(RichText::new(titel).font(egui::FontId::new(thema::FONT_SIZE_MD, fett())));
        if !erklaerung.is_empty() {
            ui.add(egui::Label::new(text(f, erklaerung, Ton::Leise)).wrap());
        }
    });
}

/// Ein Kasten wie eine Karte, ohne Titel (Listen, leere Ansichten).
pub fn kasten<R>(ui: &mut egui::Ui, f: &Farben, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(f.bg_panel)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.border_subtle))
        .inner_margin(egui::Margin::same(thema::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            inhalt(ui)
        })
        .inner
}

/// Kürzel-Quadrat (Avatar): Akzent für mich, sonst neutral; Rand im
/// Sprechsignal (Orange), wenn die Person spricht.
pub fn kuerzel(ui: &mut egui::Ui, f: &Farben, name: &str, ich: bool, spricht: bool) {
    let groesse = thema::SIZE_CONTROL_SM;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(groesse, groesse), Sense::hover());
    let p = ui.painter();
    let (flaeche, schrift): (Color32, Color32) = if ich {
        (f.accent_default, f.text_on_accent)
    } else {
        (f.bg_selected, f.text_default)
    };
    p.rect_filled(rect, 0.0, flaeche);
    if spricht {
        p.rect_stroke(
            rect.expand(thema::SIZE_FOCUS),
            0.0,
            Stroke::new(thema::SIZE_FOCUS, f.signal_default),
            egui::StrokeKind::Outside,
        );
    }
    let buchstabe: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        buchstabe,
        egui::FontId::new(thema::FONT_SIZE_MD, fett()),
        schrift,
    );
}

/// Seitenverhältnisse (Breite zu Höhe) der Marken-SVGs aus `assets/`.
const ZEICHEN_SEITEN: f32 = 438.0 / 341.0;
const SCHRIFTZUG_SEITEN: f32 = 909.0 / 100.0;

/// Die Marke im Kopf: das Zeichen und die Wortmarke als SVG aus `assets/`
/// (ADR 0015) — kein Text, keine Schrift. Im Farbthema bringt das Zeichen
/// seine türkis-orangen Farben mit; im Mono-Thema nimmt es die Textfarbe an,
/// damit nichts Farbiges auf einer einfarbigen Fläche steht. Die Wortmarke
/// wird immer mit der Textfarbe eingefärbt. Beide Fenster zeichnen sie so.
pub fn marke(ui: &mut egui::Ui, f: &Farben, thema: Thema) {
    // Die Wortmarke steht in der Höhe ihrer Großbuchstaben; das Zeichen ist
    // etwas höher als eine Zeile und füllt die Kopfleiste.
    let hoehe = thema::FONT_SIZE_LG;
    let zeichen = thema::SIZE_CONTROL_SM - thema::SPACE_1;
    let bild = if thema == Thema::Mono {
        egui::Image::new(egui::include_image!(
            "../../../assets/sprechwerk-symbolic.svg"
        ))
        .tint(f.text_default)
    } else {
        egui::Image::new(egui::include_image!(
            "../../../assets/sprechwerk-zeichen.svg"
        ))
    };
    ui.add(
        bild.fit_to_exact_size(egui::vec2(zeichen * ZEICHEN_SEITEN, zeichen))
            .alt_text("sprechwerk-Zeichen"),
    );
    ui.add_space(thema::SPACE_1);
    ui.add(
        egui::Image::new(egui::include_image!(
            "../../../assets/sprechwerk-schriftzug.svg"
        ))
        .tint(f.text_default)
        .fit_to_exact_size(egui::vec2(hoehe * SCHRIFTZUG_SEITEN, hoehe))
        .alt_text("sprechwerk"),
    );
}

/// Die Icons der Fenster — dieselbe kuratierte Lucide-Auswahl wie
/// `@weppelmann/icons` (erzeugt mit `scripts/icons-holen.py`). Ein Summentyp:
/// eine Ansicht kann kein Icon verlangen, das es nicht gibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Mehr,
    Schloss,
    PanelRechts,
    PanelZu,
    Schliessen,
    Mikrofon,
    MikrofonAus,
    Kopfhoerer,
    KopfhoererAus,
    Stecker,
    Trennen,
    Einstellungen,
    Neuladen,
    Lautsprecher,
    Chat,
    Sprechblase,
    Papierkorb,
    Kopieren,
    Reagieren,
    Server,
    Leute,
    Protokoll,
    Sperre,
    Start,
    Stopp,
    Senden,
    Warnung,
    Anhang,
    Datei,
    Film,
    Herunterladen,
    Extern,
    Smiley,
}

impl Icon {
    pub fn quelle(self) -> egui::ImageSource<'static> {
        match self {
            Icon::Plus => egui::include_image!("../icons/plus.svg"),
            Icon::Mehr => egui::include_image!("../icons/more.svg"),
            Icon::Schloss => egui::include_image!("../icons/lock.svg"),
            Icon::PanelRechts => egui::include_image!("../icons/panel_right.svg"),
            Icon::PanelZu => egui::include_image!("../icons/panel_close.svg"),
            Icon::Schliessen => egui::include_image!("../icons/close.svg"),
            Icon::Mikrofon => egui::include_image!("../icons/mic.svg"),
            Icon::MikrofonAus => egui::include_image!("../icons/mic_off.svg"),
            Icon::Kopfhoerer => egui::include_image!("../icons/headphones.svg"),
            Icon::KopfhoererAus => egui::include_image!("../icons/headphone_off.svg"),
            Icon::Stecker => egui::include_image!("../icons/plug.svg"),
            Icon::Trennen => egui::include_image!("../icons/unplug.svg"),
            Icon::Einstellungen => egui::include_image!("../icons/settings.svg"),
            Icon::Neuladen => egui::include_image!("../icons/refresh.svg"),
            Icon::Lautsprecher => egui::include_image!("../icons/speaker.svg"),
            Icon::Chat => egui::include_image!("../icons/chat.svg"),
            Icon::Sprechblase => egui::include_image!("../icons/sprechblase.svg"),
            Icon::Papierkorb => egui::include_image!("../icons/trash.svg"),
            Icon::Kopieren => egui::include_image!("../icons/copy.svg"),
            Icon::Reagieren => egui::include_image!("../icons/react.svg"),
            Icon::Server => egui::include_image!("../icons/server.svg"),
            Icon::Leute => egui::include_image!("../icons/users.svg"),
            Icon::Protokoll => egui::include_image!("../icons/log.svg"),
            Icon::Sperre => egui::include_image!("../icons/ban.svg"),
            Icon::Start => egui::include_image!("../icons/play.svg"),
            Icon::Stopp => egui::include_image!("../icons/stop.svg"),
            Icon::Senden => egui::include_image!("../icons/send.svg"),
            Icon::Warnung => egui::include_image!("../icons/warning.svg"),
            Icon::Anhang => egui::include_image!("../icons/anhang.svg"),
            Icon::Datei => egui::include_image!("../icons/datei.svg"),
            Icon::Film => egui::include_image!("../icons/film.svg"),
            Icon::Herunterladen => egui::include_image!("../icons/herunterladen.svg"),
            Icon::Extern => egui::include_image!("../icons/external.svg"),
            Icon::Smiley => egui::include_image!("../icons/smiley.svg"),
        }
    }

    /// Das Icon als Bild in Größe und Farbe.
    pub fn bild(self, groesse: f32, farbe: Color32) -> egui::Image<'static> {
        egui::Image::new(self.quelle())
            .tint(farbe)
            .fit_to_exact_size(egui::vec2(groesse, groesse))
    }
}

/// Ein Icon ohne Knopf (Schloss neben dem Raumnamen u. ä.).
pub fn icon(ui: &mut egui::Ui, f: &Farben, icon: Icon, ton: Ton) -> egui::Response {
    ui.add(icon.bild(thema::FONT_SIZE_MD, thema::ton(f, ton)))
}

/// Knopf nur mit Icon. `name` ist Hinweis **und** zugänglicher Name — ein
/// Screenreader liest ihn vor, die Maus zeigt ihn beim Darüberfahren.
/// `an` hebt den Knopf als eingeschaltet hervor (Mikrofon stumm …).
pub fn icon_knopf(
    ui: &mut egui::Ui,
    f: &Farben,
    icon: Icon,
    name: &str,
    an: bool,
) -> egui::Response {
    let farbe = if an { f.accent_default } else { f.text_muted };
    let antwort = ui
        .add(
            egui::Button::image(icon.bild(thema::FONT_SIZE_LG, farbe))
                .selected(an)
                .frame_when_inactive(an)
                .min_size(egui::vec2(thema::SIZE_CONTROL_SM, thema::SIZE_CONTROL_SM)),
        )
        .on_hover_text(name);
    let name = name.to_string();
    antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    antwort
}

/// Knopf mit Icon und Text (Reiter, Hauptaktionen).
pub fn icon_text_knopf(
    ui: &mut egui::Ui,
    f: &Farben,
    icon: Icon,
    text: &str,
    gewaehlt: bool,
) -> egui::Response {
    let (farbe, schrift) = if gewaehlt {
        (f.accent_default, f.text_default)
    } else {
        (f.text_muted, f.text_muted)
    };
    ui.add(
        egui::Button::image_and_text(
            icon.bild(thema::FONT_SIZE_MD, farbe),
            RichText::new(text).color(schrift),
        )
        .selected(gewaehlt)
        .frame_when_inactive(gewaehlt),
    )
}

/// Gewöhnlicher Knopf mit Icon vor dem Text (Neu laden, Sprechtest …).
pub fn knopf(ui: &mut egui::Ui, f: &Farben, icon: Icon, text: &str) -> egui::Response {
    ui.add(egui::Button::image_and_text(
        icon.bild(thema::FONT_SIZE_MD, f.text_muted),
        text,
    ))
}

/// Hauptschaltfläche (Akzent) mit Icon vor dem Text.
pub fn hauptknopf_mit(ui: &mut egui::Ui, f: &Farben, icon: Icon, text: &str) -> egui::Response {
    ui.add(
        egui::Button::image_and_text(
            icon.bild(thema::FONT_SIZE_MD, f.text_on_accent),
            RichText::new(text).color(f.text_on_accent),
        )
        .fill(f.accent_default)
        .stroke(Stroke::new(thema::SIZE_BORDER, f.accent_default)),
    )
}

// ---- Seiten und Formulare: einheitlich in beiden Fenstern ----------------

/// Größte Breite des Inhalts einer Seite — lange Zeilen lesen sich schlecht.
/// Größte Breite eines Eingabefelds — lange Felder lesen sich schlecht,
/// und alle Felder einer Seite sind gleich breit.
pub const FELDBREITE_MAX: f32 = thema::SPACE_8 * 10.0;
/// Ab dieser Breite steht die Beschriftung links neben dem Feld, darunter
/// darüber.
const FORMULAR_NEBENEINANDER: f32 = thema::SPACE_8 * 8.0;
/// Breite der Beschriftungsspalte.
const BESCHRIFTUNG: f32 = thema::SPACE_8 * 3.0;

/// Abstand des Inhalts zum Fensterrand.
pub const SEITENRAND: f32 = thema::SPACE_5;

/// Eine Seite: senkrecht scrollbar, so breit wie das Fenster — die Karten
/// füllen die Fläche, statt mit leerem Rand daneben zu hängen. Der Rand liegt
/// **innen**: der Scrollbalken sitzt am Fensterrand, nur der Inhalt hält
/// Abstand.
pub fn seite<R>(ui: &mut egui::Ui, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            rand(ui, |ui| {
                // Nie breiter als das Fenster — sonst wüchse der Inhalt mit
                // dem längsten Element über den Rand.
                ui.set_max_width(ui.available_width());
                ui.add_space(thema::SPACE_2);
                let r = inhalt(ui);
                ui.add_space(thema::SPACE_6);
                r
            })
        })
        .inner
}

/// Der Seitenrand für Seiten mit eigenem Scrollbereich (Protokoll).
pub fn rand<R>(ui: &mut egui::Ui, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(SEITENRAND as i8, 0))
        .show(ui, inhalt)
        .inner
}

/// Eine Formularzeile: Beschriftung, Bedienelement, Hinweis darunter. Breit
/// nebeneinander, schmal untereinander. Die Beschriftung wird zum
/// zugänglichen Namen des Elements.
pub fn feld(
    ui: &mut egui::Ui,
    f: &Farben,
    beschriftung: &str,
    hinweis: Option<&str>,
    inhalt: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    formularzeile(ui, f, beschriftung, hinweis, true, inhalt)
}

/// Wie [`feld`], aber die Beschriftung wird **nicht** zum Namen des
/// Elements — für Knöpfe und Regler, die einen eigenen Namen tragen
/// („Sprechtest starten" statt „Sprechtest").
pub fn zeile(
    ui: &mut egui::Ui,
    f: &Farben,
    beschriftung: &str,
    hinweis: Option<&str>,
    inhalt: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    formularzeile(ui, f, beschriftung, hinweis, false, inhalt)
}

fn formularzeile(
    ui: &mut egui::Ui,
    f: &Farben,
    beschriftung: &str,
    hinweis: Option<&str>,
    benennt: bool,
    inhalt: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let nebeneinander = ui.available_width() >= FORMULAR_NEBENEINANDER;
    let verknuepfen = |a: egui::Response, l: &egui::Response| {
        if benennt {
            a.labelled_by(l.id)
        } else {
            a
        }
    };
    let antwort = if nebeneinander {
        ui.horizontal(|ui| {
            let l = beschriftungsspalte(ui, f, beschriftung, thema::SIZE_CONTROL_SM);
            ui.vertical(|ui| {
                let a = verknuepfen(inhalt(ui), &l);
                if let Some(h) = hinweis {
                    ui.add(egui::Label::new(text(f, h, Ton::Blass).small()).wrap());
                }
                a
            })
            .inner
        })
        .inner
    } else {
        ui.vertical(|ui| {
            let l = ui.label(text(f, beschriftung, Ton::Leise));
            let a = verknuepfen(inhalt(ui), &l);
            if let Some(h) = hinweis {
                ui.add(egui::Label::new(text(f, h, Ton::Blass).small()).wrap());
            }
            a
        })
        .inner
    };
    ui.add_space(thema::SPACE_1);
    antwort
}

/// Die linke Spalte eines Formulars: feste Breite, Text linksbündig, gekürzt.
fn beschriftungsspalte(
    ui: &mut egui::Ui,
    f: &Farben,
    beschriftung: &str,
    hoehe: f32,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(BESCHRIFTUNG, hoehe),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(BESCHRIFTUNG, hoehe));
            ui.add(egui::Label::new(text(f, beschriftung, Ton::Leise)).truncate())
        },
    )
    .inner
}

/// Breite für Eingabefelder: so breit wie Platz ist, höchstens eine Spalte.
pub fn feldbreite(ui: &egui::Ui) -> f32 {
    ui.available_width().min(FELDBREITE_MAX)
}

/// Fakten (Beschriftung → Wert), Werte brechen um statt aus dem Rand zu
/// laufen. `mono` für Kennungen und Adressen.
pub fn fakten(ui: &mut egui::Ui, f: &Farben, zeilen: &[(String, String, bool)]) {
    let nebeneinander = ui.available_width() >= FORMULAR_NEBENEINANDER;
    for (k, w, mono) in zeilen {
        let wert = if *mono {
            RichText::new(w).monospace()
        } else {
            RichText::new(w)
        };
        if nebeneinander {
            ui.horizontal_top(|ui| {
                beschriftungsspalte(ui, f, k, thema::FONT_SIZE_LG);
                ui.add(egui::Label::new(wert).wrap());
            });
        } else {
            ui.label(text(f, k, Ton::Leise).small());
            ui.add(egui::Label::new(wert).wrap());
        }
        ui.add_space(thema::SPACE_1);
    }
}

/// Auswahl aus wenigen Möglichkeiten als Reihe (Farbschema, Sprache …).
/// Bricht um, wenn der Platz nicht reicht.
pub fn auswahl<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    wert: &mut T,
    moeglichkeiten: &[(T, String)],
) -> bool {
    let mut geaendert = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = thema::SPACE_1;
        for (m, name) in moeglichkeiten {
            let gewaehlt = *wert == *m;
            if ui
                .add(egui::Button::selectable(gewaehlt, name.as_str()).frame_when_inactive(true))
                .clicked()
                && !gewaehlt
            {
                *wert = *m;
                geaendert = true;
            }
        }
    });
    geaendert
}

/// Zustand als kleines Schild: Quadrat in der Statusfarbe, Text daneben.
pub fn status(ui: &mut egui::Ui, f: &Farben, ton: Ton, beschriftung: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(thema::SPACE_2, thema::SPACE_2), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, thema::ton(f, ton));
        ui.label(text(f, beschriftung, ton));
    })
    .response
}

/// Dasselbe Schild ohne Text (wenn der Platz fehlt) — der Text steht im
/// Hinweis und ist der zugängliche Name.
pub fn status_klein(ui: &mut egui::Ui, f: &Farben, ton: Ton, beschriftung: &str) -> egui::Response {
    let (rect, antwort) =
        ui.allocate_exact_size(egui::vec2(thema::SPACE_2, thema::SPACE_2), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, thema::ton(f, ton));
    let name = beschriftung.to_string();
    antwort.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    antwort.on_hover_text(beschriftung)
}

/// Ein Wert zum Weitergeben (Fingerabdruck, Kennung): umbrechend, daneben
/// ein Knopf zum Kopieren.
pub fn kopierbar(ui: &mut egui::Ui, f: &Farben, wert: &str, kopieren: &str) {
    ui.horizontal_top(|ui| {
        if icon_knopf(ui, f, Icon::Kopieren, kopieren, false).clicked() {
            ui.ctx().copy_text(wert.to_string());
        }
        ui.add(egui::Label::new(RichText::new(wert).monospace()).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schwelle_aus_der_position_im_balken() {
        assert_eq!(wert_aus_position(10.0, 10.0, 200.0), 0);
        assert_eq!(wert_aus_position(110.0, 10.0, 200.0), 50);
        assert_eq!(wert_aus_position(210.0, 10.0, 200.0), 100);
        assert_eq!(wert_aus_position(-50.0, 10.0, 200.0), 0, "links daneben");
        assert_eq!(wert_aus_position(999.0, 10.0, 200.0), 100, "rechts daneben");
        assert_eq!(wert_aus_position(5.0, 0.0, 0.0), 0, "ohne Breite");
    }
}

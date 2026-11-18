// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Effektbühne: Konfetti, Herzen, Beben über dem Fenster. Was spielt, steht
//! im Modell (`zustand.effekt`, mit hochzählender Nummer); hier wird nur die
//! Startzeit gemerkt und gezeichnet.

use crate::app::Zwischenspeicher;
use crate::logik;
use crate::modell::Oberflaeche;
use crate::thema;
use eframe::egui::{self, Color32, Stroke};
use sprechwerk_ipc::Effekt;

/// Wie lange ein Effekt läuft (Sekunden): längste Falldauer plus Verzug.
const DAUER: f64 = 3.6;

pub fn zeigen(ctx: &egui::Context, o: &Oberflaeche, speicher: &mut Zwischenspeicher) {
    let Some(auftritt) = &o.zustand.effekt else {
        return;
    };
    let jetzt = ctx.input(|i| i.time);
    let start = match speicher.effekt {
        Some((nr, start)) if nr == auftritt.nr => start,
        _ => {
            speicher.effekt = Some((auftritt.nr, jetzt));
            jetzt
        }
    };
    let alter = (jetzt - start) as f32;
    if alter as f64 > DAUER {
        return;
    }
    ctx.request_repaint();
    let f = o.thema().farben();
    let farben: [Color32; 5] = [
        f.accent_default,
        f.status_ok,
        f.status_warn,
        f.status_error,
        f.text_default,
    ];
    let flaeche = ctx.content_rect();
    let maler = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("effekt"),
    ));
    match auftritt.effekt {
        Effekt::Konfetti | Effekt::Herzen => {
            let herz = auftritt.effekt == Effekt::Herzen;
            for t in logik::teilchen(auftritt.nr, if herz { 24 } else { 60 }) {
                let lauf = ((alter - t.verzug) / t.dauer).clamp(0.0, 1.0);
                if lauf <= 0.0 || lauf >= 1.0 {
                    continue;
                }
                let x = flaeche.left() + flaeche.width() * (t.x + t.drift * lauf) / 100.0;
                let (y, farbe) = if herz {
                    (flaeche.bottom() - flaeche.height() * lauf, f.status_error)
                } else {
                    (flaeche.top() + flaeche.height() * lauf, farben[t.farbe])
                };
                let deckung = (1.0 - lauf).min(1.0);
                let farbe = farbe.gamma_multiply(deckung);
                if herz {
                    maler.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        "❤",
                        egui::FontId::proportional(thema::FONT_SIZE_2XL),
                        farbe,
                    );
                } else {
                    let r = egui::Rect::from_center_size(
                        egui::pos2(x, y),
                        egui::vec2(thema::SPACE_2, thema::SPACE_1),
                    );
                    let winkel = (t.drehung * lauf).to_radians();
                    let ecken: Vec<egui::Pos2> = [
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                    ]
                    .iter()
                    .map(|p| {
                        let v = *p - r.center();
                        r.center()
                            + egui::vec2(
                                v.x * winkel.cos() - v.y * winkel.sin(),
                                v.x * winkel.sin() + v.y * winkel.cos(),
                            )
                    })
                    .collect();
                    maler.add(egui::Shape::convex_polygon(ecken, farbe, Stroke::NONE));
                }
            }
        }
        Effekt::Beben => {
            // Ein Rahmen, der kurz zittert — das Fenster selbst bleibt ruhig
            // (bewegte Inhalte sind schwer lesbar und für manche unangenehm).
            let staerke = (1.0 - alter / 0.8).max(0.0) * thema::SPACE_2;
            let versatz = (alter * 60.0).sin() * staerke;
            let r = flaeche
                .shrink(thema::SPACE_1)
                .translate(egui::vec2(versatz, 0.0));
            maler.rect_stroke(
                r,
                0.0,
                Stroke::new(thema::SIZE_FOCUS * 2.0, f.accent_default),
                egui::StrokeKind::Inside,
            );
        }
        Effekt::Unbekannt => {}
    }
}

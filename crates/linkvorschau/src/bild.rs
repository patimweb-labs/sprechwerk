//! Vorschaubild klein rechnen.
//!
//! Der Empfänger bekommt nie das Originalbild einer fremden Seite, sondern ein
//! neu geschriebenes JPEG von höchstens [`KANTE`] Pixeln — gleich groß, ohne
//! Metadaten, und klein genug für den Umschlag.

use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageReader, Limits, Rgba, RgbaImage};
use std::io::Cursor;

/// Längste Kante des Vorschaubilds in Pixeln.
pub const KANTE: u32 = 480;
/// Größer wird das fertige JPEG nicht; sonst ein zweiter Versuch, kleiner.
pub const MAX_BYTES: usize = 120 * 1024;

/// Liest ein Bild (JPEG, PNG, WebP, GIF), rechnet es klein und schreibt es als
/// JPEG. `None`, wenn es kein lesbares Bild ist oder zu groß bleibt.
pub fn verkleinern(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut leser = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    // Ein fremdes Bild darf den Speicher nicht füllen: 16 000 × 16 000 Pixel
    // in einer 50-KiB-Datei sind schnell gebaut.
    let mut grenzen = Limits::default();
    grenzen.max_image_width = Some(12_000);
    grenzen.max_image_height = Some(12_000);
    grenzen.max_alloc = Some(256 * 1024 * 1024);
    leser.limits(grenzen);
    let bild = leser.decode().ok()?;
    if bild.width() == 0 || bild.height() == 0 {
        return None;
    }
    for (kante, guete) in [(KANTE, 80), (KANTE * 2 / 3, 60)] {
        // `thumbnail` vergrößert auch — ein kleines Bild bleibt, wie es ist.
        let klein = if bild.width() > kante || bild.height() > kante {
            auf_weiss(&bild.thumbnail(kante, kante))
        } else {
            auf_weiss(&bild)
        };
        let mut aus = Vec::new();
        JpegEncoder::new_with_quality(&mut aus, guete)
            .encode_image(&klein)
            .ok()?;
        if aus.len() <= MAX_BYTES {
            return Some(aus);
        }
    }
    None
}

/// JPEG kennt keine Durchsichtigkeit: durchsichtige Stellen werden weiß statt
/// schwarz (ein Vereinslogo auf schwarzem Grund wäre unsichtbar).
fn auf_weiss(bild: &DynamicImage) -> DynamicImage {
    if !bild.color().has_alpha() {
        return DynamicImage::ImageRgb8(bild.to_rgb8());
    }
    let rgba = bild.to_rgba8();
    let mut aus = RgbaImage::new(rgba.width(), rgba.height());
    for (x, y, Rgba([r, g, b, a])) in rgba.enumerate_pixels() {
        let a = u32::from(*a);
        let mische = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        aus.put_pixel(x, y, Rgba([mische(*r), mische(*g), mische(*b), 255]));
    }
    DynamicImage::ImageRgba8(aus).to_rgb8().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(breite: u32, hoehe: u32, alpha: u8) -> Vec<u8> {
        let bild = RgbaImage::from_fn(breite, hoehe, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 128, alpha])
        });
        let mut aus = Vec::new();
        DynamicImage::ImageRgba8(bild)
            .write_to(&mut Cursor::new(&mut aus), image::ImageFormat::Png)
            .unwrap();
        aus
    }

    #[test]
    fn grosses_bild_wird_klein_und_jpeg() {
        let aus = verkleinern(&png(2000, 1000, 255)).unwrap();
        assert!(aus.len() <= MAX_BYTES);
        let zurueck = image::load_from_memory(&aus).unwrap();
        assert_eq!(image::guess_format(&aus).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!((zurueck.width(), zurueck.height()), (KANTE, KANTE / 2));
    }

    #[test]
    fn kleines_bild_bleibt_klein() {
        let aus = verkleinern(&png(64, 32, 255)).unwrap();
        let zurueck = image::load_from_memory(&aus).unwrap();
        assert_eq!((zurueck.width(), zurueck.height()), (64, 32));
    }

    #[test]
    fn durchsichtiges_wird_weiss() {
        let aus = verkleinern(&png(16, 16, 0)).unwrap();
        let zurueck = image::load_from_memory(&aus).unwrap().to_rgb8();
        let p = zurueck.get_pixel(8, 8);
        assert!(p.0.iter().all(|&c| c > 240), "{p:?}");
    }

    #[test]
    fn kein_bild_ist_kein_bild() {
        assert!(verkleinern(b"").is_none());
        assert!(verkleinern(b"<html>kein Bild</html>").is_none());
        let mut kaputt = png(100, 100, 255);
        kaputt.truncate(60);
        assert!(verkleinern(&kaputt).is_none());
    }
}

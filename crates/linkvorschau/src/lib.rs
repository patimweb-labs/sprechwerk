//! Link-Vorschau, abgerufen vom **Absender** (ADR 0012 §2).
//!
//! Wer einen Link schreibt, ruft die Seite ab — Titel, Beschreibung, ein klein
//! gerechnetes Bild — und schickt die Vorschau verschlüsselt mit. Empfänger
//! rufen nichts ab und verraten ihre Adresse keiner fremden Seite; der Server
//! sieht nichts. Wie Signal.
//!
//! Die Seite erfährt die Adresse des Absenders — das ist der Preis, und er
//! liegt bei dem, der den Link teilt. Die Oberfläche lässt die Vorschau
//! abschalten.

pub mod bild;
pub mod seite;

use base64::Engine;
use sprechwerk_protocol::inhalt::Linkvorschau;
use std::time::Duration;
use url::Url;

/// Höchstens so viel HTML wird gelesen — der Kopf steht vorn.
pub const MAX_HTML_BYTES: usize = 512 * 1024;
/// Größer darf ein Bild vor dem Verkleinern nicht sein.
pub const MAX_BILD_BYTES: usize = 8 * 1024 * 1024;
/// Längste Dauer eines Abrufs (Seite oder Bild).
pub const FRIST: Duration = Duration::from_secs(8);

/// Warum es keine Vorschau gibt.
#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("kein http(s)-Link")]
    KeinWeblink,
    #[error("Abruf gescheitert: {0}")]
    Netz(String),
    #[error("Antwort {0}")]
    Status(u16),
    #[error("weder Seite noch Bild")]
    KeineSeite,
    #[error("nichts zu zeigen")]
    Leer,
}

/// Ruft Vorschauen ab. Einmal anlegen, oft benutzen (hält die Verbindungen).
#[derive(Clone)]
pub struct Holer {
    client: reqwest::Client,
}

impl Holer {
    pub fn neu() -> Result<Self, Fehler> {
        let client = reqwest::Client::builder()
            .timeout(FRIST)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(5))
            // Allgemein gehalten: nichts über Nutzer, Gerät oder Server.
            .user_agent("Mozilla/5.0 (compatible; sprechwerk-linkvorschau)")
            .build()
            .map_err(|f| Fehler::Netz(f.to_string()))?;
        Ok(Self { client })
    }

    /// Holt die Vorschau für `adresse`.
    pub async fn holen(&self, adresse: &str) -> Result<Linkvorschau, Fehler> {
        let url = Url::parse(adresse).map_err(|_| Fehler::KeinWeblink)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Fehler::KeinWeblink);
        }
        let (art, endadresse, bytes) = self.abrufen(url.clone(), MAX_HTML_BYTES, true).await?;

        // Ein Link direkt auf ein Bild: das Bild ist die Vorschau.
        if art.starts_with("image/") {
            let bild = bild::verkleinern(&bytes).ok_or(Fehler::Leer)?;
            let name = endadresse
                .path_segments()
                .and_then(|mut s| s.next_back().map(str::to_string))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| host(&endadresse));
            return Ok(Linkvorschau {
                url: adresse.to_string(),
                titel: name,
                beschreibung: None,
                bild: Some(base64::engine::general_purpose::STANDARD.encode(bild)),
            });
        }
        if !(art.starts_with("text/html") || art.starts_with("application/xhtml")) {
            return Err(Fehler::KeineSeite);
        }

        let html = String::from_utf8_lossy(&bytes);
        let s = seite::lesen(&html, &endadresse);
        let bild = match &s.bild {
            Some(b) => self.bild_holen(b.clone()).await,
            None => None,
        };
        let titel = s
            .titel
            .or(s.seitenname)
            .unwrap_or_else(|| host(&endadresse));
        if titel.is_empty() && bild.is_none() {
            return Err(Fehler::Leer);
        }
        Ok(Linkvorschau {
            url: adresse.to_string(),
            titel,
            beschreibung: s.beschreibung,
            bild,
        })
    }

    /// Bild holen und klein rechnen. Scheitert das, gibt es eben keins.
    async fn bild_holen(&self, url: Url) -> Option<String> {
        let (art, _, bytes) = self.abrufen(url, MAX_BILD_BYTES, false).await.ok()?;
        if !art.starts_with("image/") {
            return None;
        }
        let klein = bild::verkleinern(&bytes)?;
        Some(base64::engine::general_purpose::STANDARD.encode(klein))
    }

    /// Ruft ab und liest höchstens `grenze` Bytes. `abschneiden`: beim
    /// Erreichen der Grenze aufhören statt scheitern (HTML: der Kopf steht vorn).
    async fn abrufen(
        &self,
        url: Url,
        grenze: usize,
        abschneiden: bool,
    ) -> Result<(String, Url, Vec<u8>), Fehler> {
        let mut antwort = self
            .client
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,image/*;q=0.8",
            )
            .send()
            .await
            .map_err(|f| Fehler::Netz(f.to_string()))?;
        if !antwort.status().is_success() {
            return Err(Fehler::Status(antwort.status().as_u16()));
        }
        let art = antwort
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|w| w.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let endadresse = antwort.url().clone();
        let mut bytes = Vec::new();
        while let Some(stueck) = antwort
            .chunk()
            .await
            .map_err(|f| Fehler::Netz(f.to_string()))?
        {
            bytes.extend_from_slice(&stueck);
            if bytes.len() >= grenze {
                if abschneiden {
                    bytes.truncate(grenze);
                    break;
                }
                return Err(Fehler::Netz("zu groß".into()));
            }
        }
        Ok((art, endadresse, bytes))
    }
}

/// Bytes als Base64 (so steht ein Bild im Umschlag).
pub fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn host(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Ein winziger HTTP-Server: Pfad → (Inhaltstyp, Körper).
    async fn server(seiten: Vec<(&'static str, &'static str, Vec<u8>)>) -> String {
        let hoerer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let adresse = format!("http://{}", hoerer.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = hoerer.accept().await else {
                    return;
                };
                let seiten = seiten.clone();
                tokio::spawn(async move {
                    let mut puffer = vec![0u8; 4096];
                    let n = s.read(&mut puffer).await.unwrap_or(0);
                    let anfrage = String::from_utf8_lossy(&puffer[..n]).to_string();
                    let pfad = anfrage.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let antwort = match seiten.iter().find(|(p, _, _)| *p == pfad) {
                        Some((_, art, koerper)) => {
                            let mut a = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: {art}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                koerper.len()
                            )
                            .into_bytes();
                            a.extend_from_slice(koerper);
                            a
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    };
                    let _ = s.write_all(&antwort).await;
                });
            }
        });
        adresse
    }

    fn png() -> Vec<u8> {
        let bild = image::RgbaImage::from_pixel(900, 600, image::Rgba([200, 100, 50, 255]));
        let mut aus = Vec::new();
        image::DynamicImage::ImageRgba8(bild)
            .write_to(&mut std::io::Cursor::new(&mut aus), image::ImageFormat::Png)
            .unwrap();
        aus
    }

    #[tokio::test]
    async fn seite_mit_bild() {
        let html = br#"<html><head><title>Fallback</title>
            <meta property="og:title" content="Sommerfest 2026">
            <meta property="og:description" content="Am Vereinsheim">
            <meta property="og:image" content="/bild.png"></head></html>"#;
        let basis = server(vec![
            ("/fest", "text/html; charset=utf-8", html.to_vec()),
            ("/bild.png", "image/png", png()),
        ])
        .await;
        let holer = Holer::neu().unwrap();
        let url = format!("{basis}/fest");
        let v = holer.holen(&url).await.unwrap();
        assert_eq!(v.url, url);
        assert_eq!(v.titel, "Sommerfest 2026");
        assert_eq!(v.beschreibung.as_deref(), Some("Am Vereinsheim"));
        let bild = base64::engine::general_purpose::STANDARD
            .decode(v.bild.unwrap())
            .unwrap();
        let b = image::load_from_memory(&bild).unwrap();
        assert_eq!((b.width(), b.height()), (bild::KANTE, 320));
    }

    #[tokio::test]
    async fn link_auf_ein_bild() {
        let basis = server(vec![("/foto.png", "image/png", png())]).await;
        let v = Holer::neu()
            .unwrap()
            .holen(&format!("{basis}/foto.png"))
            .await
            .unwrap();
        assert_eq!(v.titel, "foto.png");
        assert!(v.bild.is_some());
    }

    #[tokio::test]
    async fn fehlendes_bild_laesst_die_vorschau_stehen() {
        let html = br#"<meta property="og:title" content="Ohne Bild"><meta property="og:image" content="/weg.png">"#;
        let basis = server(vec![("/", "text/html", html.to_vec())]).await;
        let v = Holer::neu()
            .unwrap()
            .holen(&format!("{basis}/"))
            .await
            .unwrap();
        assert_eq!(v.titel, "Ohne Bild");
        assert!(v.bild.is_none());
    }

    #[tokio::test]
    async fn was_keine_seite_ist_gibt_keine_vorschau() {
        let basis = server(vec![("/daten", "application/octet-stream", vec![0; 10])]).await;
        let holer = Holer::neu().unwrap();
        assert!(matches!(
            holer.holen(&format!("{basis}/daten")).await,
            Err(Fehler::KeineSeite)
        ));
        assert!(matches!(
            holer.holen(&format!("{basis}/fehlt")).await,
            Err(Fehler::Status(404))
        ));
        for nein in [
            "ftp://example.org",
            "javascript:alert(1)",
            "kein link",
            "file:///etc/passwd",
        ] {
            assert!(
                matches!(holer.holen(nein).await, Err(Fehler::KeinWeblink)),
                "{nein}"
            );
        }
    }
}

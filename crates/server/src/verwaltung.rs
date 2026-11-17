//! Verwaltungsoberfläche im Browser (SPEC §2, Nachtrag 2026-11-17; ADR 0011).
//!
//! Für den Betreiber: was der Server gerade protokolliert, wer verbunden ist,
//! Adresse und Fingerabdruck zum Weitergeben — und **Rollen vergeben**. Damit
//! wird man Administrator, ohne einen Schlüssel aus der Hand zu geben: mit der
//! App verbinden, hier beim eigenen Namen „Administrator" wählen.
//!
//! Dieselbe Oberfläche für beide Betriebsarten:
//! * **Container:** Port 7444 wird nur an `127.0.0.1` des Hosts gebunden
//!   (`docker-compose.yml`); von außen über einen SSH-Tunnel.
//! * **Programm** (Windows, macOS, Linux): beim Start öffnet sich der Browser
//!   mit dem Anmeldelink, wenn es einen Bildschirm gibt.
//!
//! **Schutz.** Wer die Oberfläche bedienen kann, kann Administratoren ernennen.
//! Deshalb:
//! * Zugang nur mit Code (Vorgabe: 128 Bit Zufall je Start, steht im
//!   Protokoll). Der Code wird im Keks gemerkt (`HttpOnly`, `SameSite=Strict`),
//!   nicht im Browser-Speicher, an den Skripte kämen.
//! * Ändernde Anfragen nehmen nur JSON an — ein Formular einer fremden Seite
//!   kann kein JSON schicken, ohne dass der Browser vorher fragt.
//! * Kein Einbetten in fremde Seiten, keine fremden Skripte, kein `Referer`
//!   (der Anmeldelink trägt den Code).
//! * Die Oberfläche zeigt und vergibt Rollen; Konfiguration, Sperren und Räume
//!   bleiben bei Datei und Client.

use crate::kern::{jetzt_ms, Kern};
use crate::mitschrift::Mitschrift;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sprechwerk_domain::{MemberId, Role};
use std::sync::{Arc, Mutex};
use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/verwaltung_dateien.rs"));

/// Name des Kekses, der die Anmeldung trägt.
const KEKS: &str = "sprechwerk_verwaltung";

/// Pause nach einem falschen Code — macht Durchprobieren eines selbst
/// gewählten Codes teuer, stört beim Vertippen nicht.
const PAUSE_NACH_FEHLER: Duration = Duration::from_millis(400);

/// Die Seite selbst; alles andere unter `assets/`.
const STARTSEITE: &str = "verwaltung.html";

/// Was die Oberfläche zum Arbeiten braucht.
#[derive(Clone)]
pub struct Verwaltung {
    pub kern: Arc<Mutex<Kern>>,
    pub mitschrift: Mitschrift,
    /// Zugangscode, wie er eingegeben wird.
    pub code: Arc<str>,
    /// SHA-256 des Zertifikats (zum Weitergeben an die Mitglieder).
    pub fingerabdruck: Arc<str>,
    /// Start des Prozesses (ms seit der Epoche).
    pub gestartet: i64,
}

impl Verwaltung {
    /// Der Wert im Keks: der Code als Hex — so ist jeder selbst gewählte Code
    /// ein gültiger Kekswert (kein `;`, kein Leerzeichen).
    fn kekswert(&self) -> String {
        hex::encode(self.code.as_bytes())
    }

    fn code_passt(&self, eingabe: &str) -> bool {
        gleich(eingabe.trim().as_bytes(), self.code.as_bytes())
    }

    fn angemeldet(&self, koepfe: &HeaderMap) -> bool {
        let erwartet = self.kekswert();
        koepfe
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|w| w.to_str().ok())
            .flat_map(|w| w.split(';'))
            .filter_map(|paar| paar.trim().split_once('='))
            .any(|(name, wert)| name == KEKS && gleich(wert.as_bytes(), erwartet.as_bytes()))
    }

    fn keks_setzen(&self) -> HeaderValue {
        HeaderValue::from_str(&format!(
            "{KEKS}={}; HttpOnly; SameSite=Strict; Path=/",
            self.kekswert()
        ))
        .expect("Hex ist ein gültiger Kopfwert")
    }
}

/// Vergleich in gleichbleibender Zeit — die Antwortzeit verrät nicht, wie
/// viele Zeichen am Anfang stimmen.
fn gleich(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Ein neuer Zugangscode: der aus der Konfiguration oder 128 Bit Zufall.
pub fn code_waehlen(eingestellt: &str) -> String {
    if eingestellt.is_empty() {
        crate::netz::zufall_hex(16)
    } else {
        eingestellt.to_string()
    }
}

/// Der Link, der direkt anmeldet. `0.0.0.0` (alle Adressen) ist kein Ziel für
/// den Browser — dann `127.0.0.1`.
pub fn anmeldelink(bind: &str, port: u16, code: &str) -> String {
    let host = match bind.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
        _ => bind.to_string(),
    };
    format!("http://{host}:{port}/anmelden?code={}", prozent(code))
}

/// Prozentkodierung für den Abfrageteil (alles außer `A–Z a–z 0–9 - _ . ~`).
fn prozent(text: &str) -> String {
    let mut aus = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            aus.push(b as char);
        } else {
            aus.push_str(&format!("%{b:02X}"));
        }
    }
    aus
}

/// Gibt es einen Bildschirm, auf dem sich ein Browser öffnen kann?
///
/// Unter Linux nur mit `DISPLAY` oder `WAYLAND_DISPLAY` — ein Server unter
/// systemd hat keins. Im Container nie. Windows und macOS: ja (wer dort einen
/// Dienst daraus macht, schaltet `browser` aus).
fn bildschirm_da(linux: bool, container: bool, variable_gesetzt: impl Fn(&str) -> bool) -> bool {
    if container {
        return false;
    }
    !linux || variable_gesetzt("DISPLAY") || variable_gesetzt("WAYLAND_DISPLAY")
}

/// Öffnet den Anmeldelink im Standardbrowser, wenn es einen Bildschirm gibt.
/// Scheitert das, steht der Link trotzdem im Protokoll.
pub fn browser_oeffnen(link: &str) {
    let container = std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists();
    let gesetzt = |name: &str| std::env::var_os(name).is_some_and(|w| !w.is_empty());
    if !bildschirm_da(cfg!(target_os = "linux"), container, gesetzt) {
        return;
    }
    // Ein Programm je System, keine zusätzliche Abhängigkeit. Unter Windows
    // `rundll32` statt `cmd /c start`: `cmd` deutet Zeichen im Link.
    let (programm, argumente): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("rundll32", vec!["url.dll,FileProtocolHandler", link])
    } else if cfg!(target_os = "macos") {
        ("open", vec![link])
    } else {
        ("xdg-open", vec![link])
    };
    if let Err(fehler) = std::process::Command::new(programm)
        .args(argumente)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        tracing::debug!(fehler = %fehler, "Browser nicht geöffnet");
    }
}

/// Lauscht auf `adresse` und bedient die Oberfläche, bis der Prozess endet.
pub async fn bedienen(verwaltung: Verwaltung, adresse: &str) -> anyhow::Result<()> {
    let hoerer = tokio::net::TcpListener::bind(adresse).await?;
    axum::serve(hoerer, router(verwaltung)).await?;
    Ok(())
}

/// Alle Wege der Oberfläche.
pub fn router(verwaltung: Verwaltung) -> Router {
    Router::new()
        .route("/anmelden", get(anmeldelink_folgen))
        .route("/api/anmelden", post(anmelden))
        .route("/api/abmelden", post(abmelden))
        .route("/api/stand", get(stand))
        .route("/api/protokoll", get(protokoll))
        .route("/api/rolle", post(rolle))
        .fallback(datei)
        .layer(axum::middleware::map_response(schutzkoepfe))
        .with_state(verwaltung)
}

/// Köpfe für jede Antwort (siehe Moduldokumentation: „Schutz").
async fn schutzkoepfe(mut antwort: Response) -> Response {
    let k = antwort.headers_mut();
    // `style-src 'unsafe-inline'`: Vue setzt Stilattribute (Breiten, Pegel).
    // Skripte nur aus der eigenen Quelle.
    k.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
             font-src 'self' data:; \
             frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    k.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    k.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    k.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    antwort
}

fn fehler(status: StatusCode, text: &str) -> Response {
    (status, Json(serde_json::json!({ "fehler": text }))).into_response()
}

fn nicht_angemeldet() -> Response {
    fehler(StatusCode::UNAUTHORIZED, "nicht angemeldet")
}

// ---- Anmeldung ---------------------------------------------------------

#[derive(Deserialize)]
struct Linkabfrage {
    code: Option<String>,
}

/// `GET /anmelden?code=…` — der Link aus dem Protokoll. Setzt den Keks und
/// leitet auf `/` weiter, damit der Code nicht in der Adresszeile stehen bleibt.
async fn anmeldelink_folgen(
    State(v): State<Verwaltung>,
    Query(abfrage): Query<Linkabfrage>,
) -> Response {
    let passt = abfrage.code.as_deref().is_some_and(|c| v.code_passt(c));
    if !passt {
        tokio::time::sleep(PAUSE_NACH_FEHLER).await;
    }
    let mut antwort = (StatusCode::SEE_OTHER, [(header::LOCATION, "/")]).into_response();
    if passt {
        antwort
            .headers_mut()
            .insert(header::SET_COOKIE, v.keks_setzen());
    }
    antwort
}

#[derive(Deserialize)]
struct Codeeingabe {
    code: String,
}

/// `POST /api/anmelden` mit `{"code": "…"}` — die Eingabe im Formular.
async fn anmelden(State(v): State<Verwaltung>, Json(eingabe): Json<Codeeingabe>) -> Response {
    if !v.code_passt(&eingabe.code) {
        tokio::time::sleep(PAUSE_NACH_FEHLER).await;
        return fehler(StatusCode::UNAUTHORIZED, "Code falsch");
    }
    let mut antwort = StatusCode::NO_CONTENT.into_response();
    antwort
        .headers_mut()
        .insert(header::SET_COOKIE, v.keks_setzen());
    antwort
}

/// `POST /api/abmelden` — löscht den Keks.
async fn abmelden() -> Response {
    let mut antwort = StatusCode::NO_CONTENT.into_response();
    antwort.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "sprechwerk_verwaltung=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        ),
    );
    antwort
}

// ---- Stand -------------------------------------------------------------

/// Alles, was die Übersicht zeigt. Ein Abruf, ein Bild (AGENTS.md §2.1).
#[derive(Debug, Serialize)]
pub struct Stand {
    pub name: String,
    pub version: &'static str,
    /// Wohin die Mitglieder sich verbinden (UDP).
    pub adresse: String,
    pub port: u16,
    pub fingerabdruck: String,
    pub gestartet: i64,
    pub jetzt: i64,
    pub raeume: usize,
    pub verbundene: usize,
    /// Noch niemand da: der Erste, der sich verbindet, wird Administrator.
    pub erster_wird_administrator: bool,
    pub sicherung: Sicherungsstand,
    pub mitglieder: Vec<Mitgliedszeile>,
}

#[derive(Debug, Serialize)]
pub struct Sicherungsstand {
    pub aktiv: bool,
    pub verzeichnis: String,
    /// Zeitpunkt der jüngsten Datei im Sicherungsverzeichnis.
    pub letzte: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Mitgliedszeile {
    pub id: MemberId,
    pub name: String,
    pub kennung: String,
    pub rolle: Role,
    pub verbunden: bool,
    pub raum: Option<String>,
    pub zuletzt_gesehen: i64,
}

fn letzte_sicherung(verzeichnis: &std::path::Path) -> Option<i64> {
    std::fs::read_dir(verzeichnis)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn stand_bilden(v: &Verwaltung) -> Stand {
    let (mut stand, verzeichnis) = {
        let k = v.kern.lock().expect("Sperre");
        let raumname = |id| {
            k.state
                .raumliste()
                .into_iter()
                .find(|r| r.id == id)
                .map(|r| r.name.clone())
        };
        let mut mitglieder: Vec<Mitgliedszeile> = k
            .state
            .mitglieder
            .values()
            .map(|m| {
                let verbunden = k.teilnehmer.contains_key(&m.id);
                Mitgliedszeile {
                    id: m.id,
                    name: m.name.clone(),
                    kennung: m.schluessel.clone(),
                    rolle: m.rolle,
                    verbunden,
                    raum: if verbunden {
                        m.raum.and_then(raumname)
                    } else {
                        None
                    },
                    zuletzt_gesehen: m.zuletzt_gesehen,
                }
            })
            .collect();
        // Verbundene zuerst, dann nach Name — so steht man selbst oben, sobald
        // man mit der App verbunden ist.
        mitglieder.sort_by(|a, b| {
            b.verbunden
                .cmp(&a.verbunden)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let stand = Stand {
            name: k.state.server_name.clone(),
            version: env!("CARGO_PKG_VERSION"),
            adresse: k.konfig.adresse(),
            port: k.konfig.server.port,
            fingerabdruck: v.fingerabdruck.to_string(),
            gestartet: v.gestartet,
            jetzt: jetzt_ms(),
            raeume: k.state.raeume.len(),
            verbundene: k.teilnehmer.len(),
            erster_wird_administrator: k.state.mitglieder.is_empty(),
            sicherung: Sicherungsstand {
                aktiv: k.konfig.sicherung.aktiv,
                verzeichnis: k.konfig.sicherung.verzeichnis.display().to_string(),
                letzte: None,
            },
            mitglieder,
        };
        (stand, k.konfig.sicherung.verzeichnis.clone())
    };
    // Dateisystem erst nach der Sperre: der Sprachpfad wartet sonst mit.
    stand.sicherung.letzte = letzte_sicherung(&verzeichnis);
    stand
}

/// `GET /api/stand`
async fn stand(State(v): State<Verwaltung>, koepfe: HeaderMap) -> Response {
    if !v.angemeldet(&koepfe) {
        return nicht_angemeldet();
    }
    Json(stand_bilden(&v)).into_response()
}

#[derive(Deserialize)]
struct Protokollabfrage {
    #[serde(default)]
    ab: u64,
}

/// `GET /api/protokoll?ab=N` — Zeilen ab Nummer N.
async fn protokoll(
    State(v): State<Verwaltung>,
    koepfe: HeaderMap,
    Query(abfrage): Query<Protokollabfrage>,
) -> Response {
    if !v.angemeldet(&koepfe) {
        return nicht_angemeldet();
    }
    Json(v.mitschrift.ab(abfrage.ab)).into_response()
}

#[derive(Deserialize)]
struct Rollenwunsch {
    mitglied: MemberId,
    rolle: Role,
}

/// `POST /api/rolle` mit `{"mitglied": "mitglied_…", "rolle": "administrator"}`.
async fn rolle(
    State(v): State<Verwaltung>,
    koepfe: HeaderMap,
    Json(wunsch): Json<Rollenwunsch>,
) -> Response {
    if !v.angemeldet(&koepfe) {
        return nicht_angemeldet();
    }
    let ergebnis = v
        .kern
        .lock()
        .expect("Sperre")
        .betreiber_rolle_setzen(wunsch.mitglied, wunsch.rolle);
    match ergebnis {
        Ok(()) => Json(stand_bilden(&v)).into_response(),
        Err(sprechwerk_domain::Error::LetzterAdministrator) => {
            fehler(StatusCode::CONFLICT, "letzter_administrator")
        }
        Err(sprechwerk_domain::Error::MitgliedUnbekannt) => {
            fehler(StatusCode::NOT_FOUND, "mitglied_unbekannt")
        }
        Err(andere) => fehler(StatusCode::BAD_REQUEST, &andere.to_string()),
    }
}

// ---- Dateien -------------------------------------------------------------

fn dateiart(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, endung)| endung) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Seite, wenn der Server ohne Oberfläche gebaut wurde (`cargo build` ohne
/// vorheriges `npm run verwaltung`). Die Schnittstelle funktioniert trotzdem.
const OHNE_OBERFLAECHE: &str = "<!doctype html><html lang=\"de\"><meta charset=\"utf-8\">\
<title>Sprechwerk – Verwaltung</title><h1>Sprechwerk – Verwaltung</h1>\
<p>Dieser Server wurde ohne Oberfläche gebaut.</p>\
<p>Bauen: <code>npm --prefix ui run verwaltung</code>, danach <code>cargo build -p sprechwerk-server</code>.</p></html>";

/// Alles außer `/api` und `/anmelden`: die eingebettete Oberfläche.
async fn datei(uri: Uri) -> Response {
    let pfad = uri.path().trim_start_matches('/');
    let gesucht = if pfad.is_empty() || pfad == "index.html" {
        STARTSEITE
    } else {
        pfad
    };
    if let Some((name, inhalt)) = DATEIEN.iter().find(|(n, _)| *n == gesucht) {
        // Dateinamen unter `assets/` tragen eine Prüfsumme (Vite) — die dürfen
        // ewig im Zwischenspeicher bleiben. Die Seite selbst nie.
        let zwischenspeicher = if name.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, dateiart(name)),
                (header::CACHE_CONTROL, zwischenspeicher),
            ],
            *inhalt,
        )
            .into_response();
    }
    if gesucht == STARTSEITE {
        return (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            OHNE_OBERFLAECHE,
        )
            .into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::rollenmatrix::{schluessel, verzeichnis};
    use crate::konfig::{Konfig, Server, Sicherung};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    const CODE: &str = "0123456789abcdef0123456789abcdef";

    struct Aufbau {
        verwaltung: Verwaltung,
        ada: MemberId,
        bosse: MemberId,
    }

    fn aufbau(name: &str) -> Aufbau {
        let konfig = Konfig {
            server: Server {
                daten: verzeichnis(&format!("verwaltung-{name}")),
                ..Default::default()
            },
            sicherung: Sicherung {
                aktiv: false,
                ..Default::default()
            },
            verwaltung: Default::default(),
        };
        let mut kern = Kern::neu(konfig).unwrap();
        let (ada, _) = kern.anmelden(&schluessel(1), "Ada", "10.0.0.1").unwrap();
        let (bosse, _) = kern.anmelden(&schluessel(2), "Bosse", "10.0.0.2").unwrap();
        let mitschrift = Mitschrift::neu();
        mitschrift.aufnehmen("sprechwerk startet\nlauscht\n");
        Aufbau {
            verwaltung: Verwaltung {
                kern: Arc::new(Mutex::new(kern)),
                mitschrift,
                code: CODE.into(),
                fingerabdruck: "ab".repeat(32).into(),
                gestartet: 1,
            },
            ada,
            bosse,
        }
    }

    fn keks() -> String {
        format!("{KEKS}={}", hex::encode(CODE))
    }

    async fn frage(v: &Verwaltung, anfrage: Request<Body>) -> (StatusCode, HeaderMap, String) {
        let antwort = router(v.clone()).oneshot(anfrage).await.unwrap();
        let status = antwort.status();
        let koepfe = antwort.headers().clone();
        let roh = axum::body::to_bytes(antwort.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, koepfe, String::from_utf8_lossy(&roh).into_owned())
    }

    fn holen(pfad: &str, mit_keks: bool) -> Request<Body> {
        let mut r = Request::get(pfad);
        if mit_keks {
            r = r.header(header::COOKIE, keks());
        }
        r.body(Body::empty()).unwrap()
    }

    fn schicken(pfad: &str, json: serde_json::Value, mit_keks: bool) -> Request<Body> {
        let mut r = Request::post(pfad).header(header::CONTENT_TYPE, "application/json");
        if mit_keks {
            r = r.header(header::COOKIE, format!("andere=1; {}", keks()));
        }
        r.body(Body::from(json.to_string())).unwrap()
    }

    #[tokio::test]
    async fn ohne_anmeldung_gibt_es_nichts() {
        let a = aufbau("ohne");
        for pfad in ["/api/stand", "/api/protokoll"] {
            let (status, _, _) = frage(&a.verwaltung, holen(pfad, false)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{pfad}");
        }
        let (status, _, _) = frage(
            &a.verwaltung,
            schicken(
                "/api/rolle",
                serde_json::json!({"mitglied": a.bosse, "rolle": "administrator"}),
                false,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // Ein falscher Keks hilft auch nicht.
        let r = Request::get("/api/stand")
            .header(header::COOKIE, format!("{KEKS}=00"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(frage(&a.verwaltung, r).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn anmelden_mit_code_setzt_den_keks() {
        let a = aufbau("anmelden");
        let falsch = schicken(
            "/api/anmelden",
            serde_json::json!({"code": "falsch"}),
            false,
        );
        let (status, koepfe, _) = frage(&a.verwaltung, falsch).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(koepfe.get(header::SET_COOKIE).is_none());

        let richtig = schicken(
            "/api/anmelden",
            serde_json::json!({"code": format!(" {CODE} ")}),
            false,
        );
        let (status, koepfe, _) = frage(&a.verwaltung, richtig).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let gesetzt = koepfe.get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(gesetzt.starts_with(&keks()), "{gesetzt}");
        assert!(gesetzt.contains("HttpOnly") && gesetzt.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn anmeldelink_leitet_weiter_und_vergisst_den_code() {
        let a = aufbau("link");
        let (status, koepfe, _) = frage(
            &a.verwaltung,
            holen(&format!("/anmelden?code={CODE}"), false),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(koepfe.get(header::LOCATION).unwrap(), "/");
        assert!(koepfe.get(header::SET_COOKIE).is_some());

        let (status, koepfe, _) = frage(&a.verwaltung, holen("/anmelden?code=nein", false)).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert!(koepfe.get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn stand_zeigt_mitglieder_und_fingerabdruck() {
        let a = aufbau("stand");
        let (status, _, text) = frage(&a.verwaltung, holen("/api/stand", true)).await;
        assert_eq!(status, StatusCode::OK);
        let stand: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(stand["fingerabdruck"], "ab".repeat(32));
        assert_eq!(stand["erster_wird_administrator"], false);
        let mitglieder = stand["mitglieder"].as_array().unwrap();
        assert_eq!(mitglieder.len(), 2);
        assert_eq!(mitglieder[0]["name"], "Ada");
        assert_eq!(mitglieder[0]["rolle"], "administrator");
        assert_eq!(mitglieder[1]["rolle"], "mitglied");
    }

    #[tokio::test]
    async fn betreiber_ernennt_administrator() {
        let a = aufbau("rolle");
        let (status, _, text) = frage(
            &a.verwaltung,
            schicken(
                "/api/rolle",
                serde_json::json!({"mitglied": a.bosse, "rolle": "administrator"}),
                true,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let k = a.verwaltung.kern.lock().unwrap();
        assert_eq!(
            k.state.mitglied(a.bosse).unwrap().rolle,
            Role::Administrator
        );
        let zeile = k.state.protokoll.last().unwrap();
        assert_eq!(zeile.handelnder_name, sprechwerk_domain::state::BETREIBER);
    }

    #[tokio::test]
    async fn letzter_administrator_bleibt_auch_hier() {
        let a = aufbau("letzter");
        let (status, _, text) = frage(
            &a.verwaltung,
            schicken(
                "/api/rolle",
                serde_json::json!({"mitglied": a.ada, "rolle": "gast"}),
                true,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(text.contains("letzter_administrator"), "{text}");
    }

    #[tokio::test]
    async fn aendern_nur_mit_json() {
        // Ein Formular einer fremden Seite schickt `application/x-www-form-urlencoded`.
        let a = aufbau("formular");
        let r = Request::post("/api/rolle")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, keks())
            .body(Body::from("mitglied=x&rolle=administrator"))
            .unwrap();
        let (status, _, _) = frage(&a.verwaltung, r).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn protokoll_kommt_ab_einer_nummer() {
        let a = aufbau("protokoll");
        let (status, _, text) = frage(&a.verwaltung, holen("/api/protokoll?ab=1", true)).await;
        assert_eq!(status, StatusCode::OK);
        let auszug: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(auszug["zeilen"].as_array().unwrap().len(), 1);
        assert_eq!(auszug["zeilen"][0]["text"], "lauscht");
        assert_eq!(auszug["weiter_ab"], 2);
    }

    #[tokio::test]
    async fn jede_antwort_traegt_die_schutzkoepfe() {
        let a = aufbau("koepfe");
        for anfrage in [holen("/", false), holen("/api/stand", false)] {
            let (_, koepfe, _) = frage(&a.verwaltung, anfrage).await;
            assert_eq!(koepfe.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
            assert_eq!(koepfe.get(header::REFERRER_POLICY).unwrap(), "no-referrer");
            let csp = koepfe
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap()
                .to_str()
                .unwrap();
            assert!(csp.contains("default-src 'self'"));
            assert!(csp.contains("frame-ancestors 'none'"));
        }
    }

    #[tokio::test]
    async fn startseite_gibt_es_immer() {
        // Mit oder ohne gebaute Oberfläche: `/` antwortet mit HTML.
        let a = aufbau("startseite");
        let (status, koepfe, _) = frage(&a.verwaltung, holen("/", false)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(koepfe
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let (status, _, _) = frage(&a.verwaltung, holen("/assets/gibt-es-nicht.js", false)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn anmeldelink_fuer_den_browser() {
        assert_eq!(
            anmeldelink("0.0.0.0", 7444, "abc"),
            "http://127.0.0.1:7444/anmelden?code=abc"
        );
        assert_eq!(
            anmeldelink("::1", 7444, "a b&c"),
            "http://[::1]:7444/anmelden?code=a%20b%26c"
        );
        assert_eq!(
            anmeldelink("192.168.1.5", 80, "x"),
            "http://192.168.1.5:80/anmelden?code=x"
        );
    }

    #[test]
    fn browser_nur_mit_bildschirm() {
        let nichts = |_: &str| false;
        let x11 = |n: &str| n == "DISPLAY";
        let wayland = |n: &str| n == "WAYLAND_DISPLAY";
        assert!(!bildschirm_da(true, false, nichts), "Linux ohne Bildschirm");
        assert!(bildschirm_da(true, false, x11));
        assert!(bildschirm_da(true, false, wayland));
        assert!(!bildschirm_da(true, true, x11), "Container nie");
        assert!(bildschirm_da(false, false, nichts), "Windows/macOS");
    }

    #[test]
    fn code_aus_konfiguration_oder_zufall() {
        assert_eq!(code_waehlen("mein-langer-code-123"), "mein-langer-code-123");
        let a = code_waehlen("");
        assert_eq!(a.len(), 32);
        assert_ne!(a, code_waehlen(""));
    }

    #[test]
    fn vergleich_braucht_gleiche_laenge() {
        assert!(gleich(b"abc", b"abc"));
        assert!(!gleich(b"abc", b"abd"));
        assert!(!gleich(b"abc", b"abcd"));
    }
}

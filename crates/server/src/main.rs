//! sprechwerk — Server.
//!
//! Ein Prozess, ein Container, ein QUIC-Port (**Festlegung SPEC §2/§7**).
//! Startablauf: Konfiguration lesen, Datenbank öffnen, Zustand laden,
//! Sicherung planen, lauschen.

use anyhow::Context;
use sprechwerk_server::verwaltung::{self, Antwort, Auftrag};
use sprechwerk_server::{dienst, gesundheit, kern, konfig};

/// Unterbefehl `beispiel` gibt eine Konfigurationsdatei aus.
const BEISPIEL: &str = "beispiel";

const HILFE: &str = r#"sprechwerk — Server

  sprechwerk [konfigurationsdatei]   Server starten (Vorgabe: sprechwerk.toml)
  sprechwerk beispiel                Beispielkonfiguration ausgeben
  sprechwerk export <datei>          Zustand als Text schreiben
  sprechwerk import <datei>          Zustand aus Text einspielen
  sprechwerk sicherung               Sofortsicherung anlegen
  sprechwerk --gesundheit            Lebenszeichen prüfen (für den Healthcheck)

Verwaltung des laufenden Servers (im Container: docker exec … sprechwerk …):
  sprechwerk stand                   Übersicht: Adresse, Fingerabdruck, Mitglieder
  sprechwerk rolle <wer> <rolle>     Rolle setzen (wer: Name, Kennungsanfang oder mitglied_…;
                                     rolle: administrator, moderator, mitglied, gast)
  sprechwerk protokoll [ab]          Protokollzeilen (ab laufender Nummer)

Konfiguration über Datei und Umgebungsvariablen (SPRECHWERK_*), siehe `beispiel`.
"#;

fn konfig_laden() -> anyhow::Result<konfig::Konfig> {
    let pfad = std::env::var("SPRECHWERK_KONFIG").unwrap_or_else(|_| "sprechwerk.toml".to_string());
    konfig::Konfig::laden(&pfad).with_context(|| format!("Konfiguration {pfad}"))
}

/// Lädt den Zustand für Betriebswerkzeuge. Eine Datenbank ohne Räume (frisch
/// angelegt) bekommt den Empfangsraum, damit Export und Sicherung nie einen
/// unbrauchbaren Zustand schreiben.
fn zustand_laden(konfig: &konfig::Konfig) -> anyhow::Result<sprechwerk_domain::State> {
    let store = sprechwerk_store::Store::oeffnen(konfig.datenbank())?;
    let standard_rolle = konfig.standard_rolle();
    let name = konfig.server.name.clone();
    let mut state = store
        .laden(|| sprechwerk_domain::State::neu(name.clone(), standard_rolle, kern::jetzt_ms()))?;
    if state.raeume.is_empty() {
        state = sprechwerk_domain::State::neu(name, standard_rolle, kern::jetzt_ms());
    }
    Ok(state)
}

fn betrieb_export(ziel: &str) -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = zustand_laden(&konfig)?;
    sprechwerk_store::Store::exportieren(&state, ziel)?;
    println!("Zustand geschrieben: {ziel}");
    Ok(())
}

fn betrieb_import(quelle: &str) -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = sprechwerk_store::Store::importieren(quelle)?;
    let mut store = sprechwerk_store::Store::oeffnen(konfig.datenbank())?;
    // Vor dem Überschreiben sichern: ein Import ist der gefährlichste Befehl.
    let sicherung = konfig
        .sicherung
        .verzeichnis
        .join(format!("vor-import-{}.json", kern::jetzt_ms()));
    if let Ok(alt) = store.laden(|| sprechwerk_domain::State::neu("leer", state.standard_rolle, 0))
    {
        let _ = sprechwerk_store::Store::exportieren(&alt, &sicherung);
    }
    store.speichern(&state)?;
    println!("Zustand eingespielt: {quelle}");
    println!("Bisheriger Zustand gesichert: {}", sicherung.display());
    Ok(())
}

/// Prüft das Lebenszeichen (für `HEALTHCHECK` im Container).
///
/// **Warum nicht eine Testverbindung?** Der erste Client wird Administrator
/// (SPEC §7) — ein Healthcheck, der sich verbindet, belegt diesen Platz mit
/// einem Wegwerfschlüssel. Geprüft wird deshalb die Datei, die der Dienst
/// selbst schreibt.
fn betrieb_gesundheit() -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let daten = &konfig.server.daten;
    let jetzt = kern::jetzt_ms();
    let stand = gesundheit::lese(daten);
    if gesundheit::ist_frisch(stand, jetzt) {
        println!(
            "gesund (Lebenszeichen vor {} ms)",
            jetzt - stand.unwrap_or(jetzt)
        );
        return Ok(());
    }
    anyhow::bail!(
        "nicht gesund: kein frisches Lebenszeichen in {} (Grenze {} ms)",
        daten.display(),
        gesundheit::GRENZE_MS
    )
}

fn betrieb_sicherung() -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    let state = zustand_laden(&konfig)?;
    let pfad = sprechwerk_store::Store::sichern(
        &state,
        &konfig.sicherung.verzeichnis,
        konfig.sicherung.behalten,
        kern::jetzt_ms(),
    )?;
    println!("Sicherung: {}", pfad.display());
    Ok(())
}

/// Wartet auf die Aufforderung, sich zu beenden.
///
/// **Gefunden 2026-11-17:** Hier stand nur `ctrl_c()` — das ist SIGINT. `docker
/// stop` schickt aber SIGTERM, und als Prozess 1 im Container hat der Server
/// dafür keine Vorgabebehandlung: das Signal verpuffte, nach 10 s kam SIGKILL,
/// das letzte Speichern fiel aus. Unter Windows schließt man das Programm über
/// das Konsolenfenster (`CTRL_CLOSE_EVENT`), nicht mit Ctrl-C.
async fn ende_abwarten() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(windows)]
    {
        match tokio::signal::windows::ctrl_close() {
            Ok(mut schliessen) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = schliessen.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let erster = args.next();

    if erster.as_deref() == Some(BEISPIEL) {
        print!("{}", konfig::Konfig::beispiel());
        return Ok(());
    }

    // Betriebswerkzeuge (**Festlegung SPEC §2**: der Text ist die Wahrheit,
    // darum muss man ihn jederzeit herausgeben und zurücklesen können).
    match erster.as_deref() {
        Some("export") => {
            let ziel = args.next().unwrap_or_else(|| "export.json".to_string());
            return betrieb_export(&ziel);
        }
        Some("import") => {
            let quelle = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("Aufruf: sprechwerk import <datei>"))?;
            return betrieb_import(&quelle);
        }
        Some("sicherung") => return betrieb_sicherung(),
        Some("stand") | Some("mitglieder") => return verwalten(Auftrag::Stand),
        Some("protokoll") => {
            let ab = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
            return verwalten(Auftrag::Protokoll { ab });
        }
        Some("rolle") => {
            let (Some(wer), Some(rolle)) = (args.next(), args.next()) else {
                anyhow::bail!("Aufruf: sprechwerk rolle <wer> <rolle>");
            };
            let rolle = sprechwerk_domain::Role::aus_text(&rolle).ok_or_else(|| {
                anyhow::anyhow!(
                    "unbekannte Rolle „{rolle}“ (administrator, moderator, mitglied, gast)"
                )
            })?;
            return verwalten(Auftrag::Rolle { wer, rolle });
        }
        Some("gesundheit") | Some("--gesundheit") => return betrieb_gesundheit(),
        Some("hilfe") | Some("--hilfe") | Some("-h") => {
            print!("{}", HILFE);
            return Ok(());
        }
        _ => {}
    }

    let konfig_pfad = std::env::var("SPRECHWERK_KONFIG")
        .ok()
        .or(erster)
        .unwrap_or_else(|| "sprechwerk.toml".to_string());
    let mitschrift = dienst::protokoll_einrichten();
    let konfig = konfig::Konfig::laden(&konfig_pfad)
        .with_context(|| format!("Konfiguration {konfig_pfad}"))?;

    let laufzeit = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    laufzeit.block_on(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Bei SIGTERM/Ctrl-C noch einmal speichern.
        tokio::spawn(async move {
            let Ok(v) = rx.await else { return };
            ende_abwarten().await;
            tracing::info!("beende, Zustand wird gespeichert");
            dienst::speichern(&v);
            std::process::exit(0);
        });
        dienst::laufen(konfig, mitschrift, move |v| {
            let _ = tx.send(v);
        })
        .await
    })
}

/// Ein Auftrag an den laufenden Server (Konsole).
fn verwalten(auftrag: Auftrag) -> anyhow::Result<()> {
    let konfig = konfig_laden()?;
    if !konfig.verwaltung.aktiv {
        anyhow::bail!("Verwaltung ist abgeschaltet (verwaltung.aktiv = false)");
    }
    match verwaltung::fragen(&konfig.server.daten, konfig.verwaltung.port, auftrag)? {
        Antwort::Stand { stand } => print!("{}", verwaltung::stand_als_text(&stand)),
        Antwort::Protokoll { auszug } => {
            for z in auszug.zeilen {
                println!("{:>6}  {}", z.nr, z.text);
            }
        }
        Antwort::Rollenfehler { fehler } => anyhow::bail!("{fehler}"),
        Antwort::Abgelehnt { grund } => anyhow::bail!("abgelehnt: {grund}"),
    }
    Ok(())
}

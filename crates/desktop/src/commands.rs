// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Naht zwischen Fenster und Logik.
//!
//! Jeder Befehl hier ist eine Übersetzung: Vue schickt etwas, `crates/ipc`
//! beschreibt den Auftrag, `crates/adapters` redet mit dem Server. In dieser
//! Datei steht **keine** Fachlogik — wenn hier gerechnet wird, gehört das
//! Rechnen in ein Crate.

use crate::anwendung::Anwendung;
use crate::einstellungen::{Einstellungen, EinstellungenPfad};
use crate::sprech::Sprechbetrieb;
use serde::Serialize;
use sprechwerk_audio::Sendeeinstellungen;
use sprechwerk_ipc::{Antwort, Zustand};
use std::sync::Arc;
use tauri::State;

/// Der aktuelle Modellstand (die Oberfläche braucht ihn beim Start).
#[tauri::command]
pub fn zustand(anwendung: State<'_, Arc<Anwendung>>) -> Zustand {
    anwendung.zustand()
}

/// Verbindet zu einem Server.
#[tauri::command]
pub async fn verbinden(
    anwendung: State<'_, Arc<Anwendung>>,
    sprech: State<'_, Arc<Sprechbetrieb>>,
    pfad: State<'_, EinstellungenPfad>,
    server: String,
    name: String,
    passwort: Option<String>,
) -> Result<String, String> {
    let ergebnis = anwendung.verbinden(server, name, passwort).await;
    if ergebnis.is_ok() {
        // Der Sprechpfad gehört zur Verbindung: ohne Verbindung kein Ton.
        // Ein Fehler hier darf das Verbinden nicht scheitern lassen — dann
        // steht eben nur das Mikrofon stumm, und die Meldung sagt warum.
        if let Err(fehler) = audio_starten_mit(&sprech, &Einstellungen::laden(&pfad.0)) {
            return Ok(format!("{}|{fehler}", ergebnis.unwrap_or_default()));
        }
    }
    ergebnis
}

/// Trennt die Verbindung.
#[tauri::command]
pub async fn trennen(anwendung: State<'_, Arc<Anwendung>>) -> Result<(), String> {
    match anwendung.befehl(sprechwerk_ipc::Befehl::Trennen).await {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Betritt einen Raum.
#[tauri::command]
pub async fn betreten(
    anwendung: State<'_, Arc<Anwendung>>,
    raum: sprechwerk_domain::RoomId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Betreten { raum })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Schreibt einen Text in den aktuellen Raum.
#[tauri::command]
pub async fn text_senden(
    anwendung: State<'_, Arc<Anwendung>>,
    text: String,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Text { text })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Löscht eine Nachricht (eigene, oder fremde als Moderator).
#[tauri::command]
pub async fn nachricht_loeschen(
    anwendung: State<'_, Arc<Anwendung>>,
    nachricht: sprechwerk_domain::MessageId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::NachrichtLoeschen { nachricht })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Legt einen Raum an.
#[tauri::command]
pub async fn raum_anlegen(
    anwendung: State<'_, Arc<Anwendung>>,
    name: String,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RaumAnlegen { name })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Benennt einen Raum um.
#[tauri::command]
pub async fn raum_umbenennen(
    anwendung: State<'_, Arc<Anwendung>>,
    raum: sprechwerk_domain::RoomId,
    name: String,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RaumUmbenennen { raum, name })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Löscht einen Raum.
#[tauri::command]
pub async fn raum_loeschen(
    anwendung: State<'_, Arc<Anwendung>>,
    raum: sprechwerk_domain::RoomId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RaumLoeschen { raum })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Setzt die Rolle eines Mitglieds.
#[tauri::command]
pub async fn rolle_setzen(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
    rolle: sprechwerk_domain::Role,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RolleSetzen { mitglied, rolle })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Schaltet Mikrofon oder Wiedergabe eines Mitglieds stumm (serverseitig).
#[tauri::command]
pub async fn stummschalten(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
    taub: bool,
    an: bool,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Stummschalten { mitglied, taub, an })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Verschiebt ein Mitglied in einen anderen Raum.
#[tauri::command]
pub async fn verschieben(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
    raum: sprechwerk_domain::RoomId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Verschieben { mitglied, raum })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Wirft ein Mitglied aus seinem Raum (zurück in den Wurzelraum).
#[tauri::command]
pub async fn aus_raum_werfen(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::AusRaumWerfen { mitglied })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Trennt ein Mitglied vom Server (Rauswurf).
#[tauri::command]
pub async fn trennen_mitglied(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
    grund: String,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::MitgliedTrennen { mitglied, grund })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Schaltet die Ende-zu-Ende-Verschlüsselung eines Raums um.
#[tauri::command]
pub async fn raum_verschluesseln(
    anwendung: State<'_, Arc<Anwendung>>,
    raum: sprechwerk_domain::RoomId,
    an: bool,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RaumVerschluesseln { raum, an })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Sperrt ein Mitglied und trennt es.
#[tauri::command]
pub async fn sperren(
    anwendung: State<'_, Arc<Anwendung>>,
    mitglied: sprechwerk_domain::MemberId,
    grund: String,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Sperren { mitglied, grund })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Stand des Sprechpfads (die Oberfläche fragt ihn im Takt ab).
#[derive(Debug, Clone, Serialize)]
pub struct AudioStand {
    /// Läuft Aufnahme und Wiedergabe?
    pub laeuft: bool,
    /// Mikrofonpegel 0–100.
    pub pegel: u8,
    /// Eigenes Mikrofon stumm?
    pub stumm: bool,
    /// Eigener Ton aus?
    pub taub: bool,
}

/// Fordert den Raumschlüssel erneut an (für einen verschlüsselten Raum).
#[tauri::command]
pub async fn schluessel_anfordern(
    anwendung: State<'_, Arc<Anwendung>>,
    raum: sprechwerk_domain::RoomId,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::SchluesselAnfordern { raum })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Misst die Umlaufzeit zum Server (Ping → Pong).
#[tauri::command]
pub async fn ping(anwendung: State<'_, Arc<Anwendung>>) -> Result<(), String> {
    match anwendung.befehl(sprechwerk_ipc::Befehl::Ping).await {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Listet die verhängten Sperren (nur Administrator).
#[tauri::command]
pub async fn sperrliste(anwendung: State<'_, Arc<Anwendung>>) -> Result<(), String> {
    match anwendung.befehl(sprechwerk_ipc::Befehl::Sperrliste).await {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Fragt die Betriebszahlen des Servers ab.
#[tauri::command]
pub async fn zaehler(anwendung: State<'_, Arc<Anwendung>>) -> Result<(), String> {
    match anwendung.befehl(sprechwerk_ipc::Befehl::Zaehler).await {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Liest das Prüfprotokoll (Administrator und Moderator).
///
/// Die Einträge kommen als Rahmen zurück und landen über `abbilden` im Modell;
/// die Ansicht liest sie von dort. Deshalb ist die Antwort leer — der neue
/// Stand geht wie immer über die Melderinne zum Fenster.
#[tauri::command]
pub async fn protokoll(
    anwendung: State<'_, Arc<Anwendung>>,
    ab: usize,
    anzahl: usize,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Protokoll { ab, anzahl })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Hebt eine Sperre auf (nur Administrator).
#[tauri::command]
pub async fn entsperren(
    anwendung: State<'_, Arc<Anwendung>>,
    kennung: Option<String>,
    adresse: Option<String>,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::Entsperren { kennung, adresse })
        .await
    {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(()),
    }
}

/// Startet den Sprechpfad mit den gespeicherten Einstellungen.
#[tauri::command]
pub fn audio_starten(
    sprech: State<'_, Arc<Sprechbetrieb>>,
    pfad: State<'_, EinstellungenPfad>,
) -> Result<(), String> {
    audio_starten_mit(&sprech, &Einstellungen::laden(&pfad.0))
}

/// Beendet den Sprechpfad.
#[tauri::command]
pub fn audio_stoppen(sprech: State<'_, Arc<Sprechbetrieb>>) -> Result<(), String> {
    sprech.stoppen();
    Ok(())
}

/// Stand des Sprechpfads.
#[tauri::command]
pub fn audio_stand(sprech: State<'_, Arc<Sprechbetrieb>>) -> AudioStand {
    AudioStand {
        laeuft: sprech.laeuft(),
        pegel: sprech.pegel(),
        stumm: sprech.ist_stumm(),
        taub: sprech.ist_taub(),
    }
}

/// Push-to-Talk: Taste gedrückt (`true`) oder losgelassen (`false`).
#[tauri::command]
pub fn sprechtaste(sprech: State<'_, Arc<Sprechbetrieb>>, an: bool) -> Result<(), String> {
    sprech.taste_setzen(an);
    Ok(())
}

/// Eigenes Mikrofon stummschalten.
#[tauri::command]
pub fn mikrofon_stumm(sprech: State<'_, Arc<Sprechbetrieb>>, an: bool) -> Result<(), String> {
    sprech.stumm_setzen(an);
    Ok(())
}

/// Eigenen Ton abschalten („taub"): nichts wird abgespielt.
#[tauri::command]
pub fn hoeren_aus(sprech: State<'_, Arc<Sprechbetrieb>>, an: bool) -> Result<(), String> {
    sprech.taub_setzen(an);
    Ok(())
}

/// Wendet geänderte Einstellungen auf den laufenden Sprechpfad an.
///
/// Schwelle, Rauschunterdrückung, Noisegate und Push-to-Talk wirken sofort.
/// Ein anderes **Gerät** verlangt einen Neustart des Pfads — das prüft
/// `audio_starten_mit` am Gerätenamen.
#[tauri::command]
pub fn audio_einstellen(
    sprech: State<'_, Arc<Sprechbetrieb>>,
    einstellungen: Einstellungen,
) -> Result<(), String> {
    sprech.push_to_talk_setzen(einstellungen.push_to_talk);
    sprech.wende_an(
        Some(einstellungen.mikrofon.clone()),
        Some(einstellungen.lautsprecher.clone()),
        sende_einstellungen(&einstellungen),
    )
}

/// Startet den Pfad mit den übergebenen Einstellungen.
fn audio_starten_mit(sprech: &Sprechbetrieb, einstellungen: &Einstellungen) -> Result<(), String> {
    sprech.push_to_talk_setzen(einstellungen.push_to_talk);
    sprech.starten(
        Some(einstellungen.mikrofon.clone()),
        Some(einstellungen.lautsprecher.clone()),
        sende_einstellungen(einstellungen),
    )
}

/// Übersetzt die Fenstereinstellungen in die Sprache des Audiopfads.
fn sende_einstellungen(einstellungen: &Einstellungen) -> Sendeeinstellungen {
    Sendeeinstellungen {
        entrauschen: einstellungen.rauschunterdrueckung,
        noisegate: einstellungen.noisegate,
        schwelle: einstellungen.schwelle,
        vorlauf_ms: einstellungen.vorlauf_ms as u32,
        paritaet: true,
        echounterdrueckung: einstellungen.echounterdrueckung,
    }
}

/// Nur **eine** Messung gleichzeitig: `parec` ist ein Prozess, und zwei
/// gleichzeitige Aufnahmen auf demselben Gerät bremsen den Audiopfad aus.
static MESSUNG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Verfügbare Aufnahme- und Wiedergabegeräte.
///
/// `async`: die Geräteliste kommt aus zwei Prozessaufrufen (`pactl`). Ein
/// **synchroner** Befehl läuft in Tauri auf dem Hauptthread und würde die
/// Oberfläche so lange anhalten.
#[tauri::command]
pub async fn audio_geraete() -> Result<crate::audio::Geraeteliste, String> {
    tauri::async_runtime::spawn_blocking(|| {
        Ok(crate::audio::Geraeteliste {
            mikrofon: crate::audio::mikrofon_geraete().map_err(|e| e.to_string())?,
            lautsprecher: crate::audio::lautsprecher_geraete().map_err(|e| e.to_string())?,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Misst den Eingangspegel (Sprechtest).
///
/// **Zwei Wege, je nach Lage:**
/// 1. Läuft der Sprechpfad, wird dessen Pegel gelesen — der Wert steht schon
///    bereit (Mikrosekunden), und es entsteht **kein** zweiter Aufnahmeprozess.
///    Zwei Aufnahmen am selben Gerät sind der Grund, warum der Test vorher
///    den ganzen Audiopfad ins Stocken brachte.
/// 2. Sonst wird kurz gemessen — aber **nicht auf dem Hauptthread**, sondern
///    über `spawn_blocking`. Ein synchroner Befehl hält sonst die Oberfläche
///    an, und zwar für die ganze Messdauer.
#[tauri::command]
pub async fn audio_pegel(
    sprech: State<'_, Arc<Sprechbetrieb>>,
    geraet: String,
    millisekunden: u64,
) -> Result<u8, String> {
    if sprech.laeuft() {
        return Ok(sprech.pegel());
    }
    tauri::async_runtime::spawn_blocking(move || {
        // Nur eine Messung gleichzeitig; eine zweite bekommt den letzten Wert
        // nicht, sondern eine klare Meldung.
        let Ok(_frei) = MESSUNG.try_lock() else {
            return Err("Es läuft schon eine Messung".to_string());
        };
        crate::audio::pegel(&geraet, millisekunden).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Spielt einen Prüfton auf dem gewählten Gerät (Hörtest).
///
/// `async` + `spawn_blocking`: `paplay` läuft bis zu einem Drittel einer Sekunde,
/// und so lange darf die Oberfläche nicht stehen.
#[tauri::command]
pub async fn audio_testton(geraet: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::testton(&geraet).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Liest die gespeicherten Einstellungen.
#[tauri::command]
pub fn einstellungen_lesen(pfad: State<'_, EinstellungenPfad>) -> Einstellungen {
    Einstellungen::laden(&pfad.0)
}

/// Schreibt die Einstellungen.
#[tauri::command]
pub fn einstellungen_speichern(
    pfad: State<'_, EinstellungenPfad>,
    einstellungen: Einstellungen,
) -> Result<(), String> {
    einstellungen.speichern(&pfad.0)
}

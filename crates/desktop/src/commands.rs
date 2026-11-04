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
    server: String,
    name: String,
    passwort: Option<String>,
) -> Result<String, String> {
    anwendung.verbinden(server, name, passwort).await
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

/// Verfügbare Aufnahme- und Wiedergabegeräte.
#[tauri::command]
pub fn audio_geraete() -> Result<crate::audio::Geraeteliste, String> {
    Ok(crate::audio::Geraeteliste {
        mikrofon: crate::audio::mikrofon_geraete().map_err(|e| e.to_string())?,
        lautsprecher: crate::audio::lautsprecher_geraete().map_err(|e| e.to_string())?,
    })
}

/// Misst den Eingangspegel für kurze Zeit (Sprechtest).
#[tauri::command]
pub fn audio_pegel(geraet: String, millisekunden: u64) -> Result<u8, String> {
    crate::audio::pegel(&geraet, millisekunden).map_err(|e| e.to_string())
}

/// Spielt einen Prüfton auf dem gewählten Gerät (Hörtest).
#[tauri::command]
pub fn audio_testton(geraet: String) -> Result<(), String> {
    crate::audio::testton(&geraet).map_err(|e| e.to_string())
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

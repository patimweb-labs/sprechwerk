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

/// Verlässt den aktuellen Raum.
#[tauri::command]
pub async fn verlassen(anwendung: State<'_, Arc<Anwendung>>) -> Result<(), String> {
    match anwendung.befehl(sprechwerk_ipc::Befehl::Verlassen).await {
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
    eltern: Option<sprechwerk_domain::RoomId>,
) -> Result<(), String> {
    match anwendung
        .befehl(sprechwerk_ipc::Befehl::RaumAnlegen { name, eltern })
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

/// Fragt das Prüfprotokoll an.
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

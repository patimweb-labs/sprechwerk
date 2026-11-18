// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der Client-Kern: alles, was ein sprechwerk-Client tut, ohne Oberfläche.
//!
//! Eine Oberfläche (das native Fenster in `crates/fenster`) ruft nur
//! [`Client`] auf und zeichnet den gemeldeten [`sprechwerk_ipc::Zustand`]
//! (AGENTS.md §2.1: `render(modell)`). Gerechnet wird hier und darunter.

pub mod anwendung;
pub mod audio;
pub mod einstellungen;
pub mod sitzung;
pub mod sprech;
pub mod verschluesselung;

pub use sitzung::{AudioStand, Client};

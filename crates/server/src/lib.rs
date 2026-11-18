// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Der sprechwerk-Server als Bibliothek.
//!
//! Zwei Programme benutzen ihn: `sprechwerk` (Konsole, für Docker und
//! Dienste) und `sprechwerk-server-fenster` (dasselbe mit Fenster). Beide
//! starten [`dienst::laufen`] und verwalten über [`verwaltung::Verwaltung`].

pub mod ablage;
pub mod dienst;
pub mod gesundheit;
pub mod kern;
pub mod konfig;
pub mod mitschrift;
pub mod netz;
pub mod tls;
pub mod verwaltung;

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

// Unterdrückt ein zusätzliches Konsolenfenster unter Windows im Release-Bau.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sprechwerk_desktop::run();
}

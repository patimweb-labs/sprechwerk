// SPDX-License-Identifier: MIT
// Setzt die Änderungszeit einer Datei auf jetzt (`touch`, auch unter Windows).
//
// Nach `npm run verwaltung` berührt es `crates/server/build.rs`: Fehlte die
// Oberfläche beim letzten `cargo build`, beobachtet Cargo nur diese Datei —
// so nimmt der nächste Bau die frisch gebaute Oberfläche auf.
import { utimesSync } from "node:fs";

const jetzt = new Date();
for (const datei of process.argv.slice(2)) utimesSync(datei, jetzt, jetzt);

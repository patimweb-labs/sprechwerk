//! Bettet die gebaute Verwaltungsoberfläche in das Programm ein.
//!
//! Quelle: `ui/dist-verwaltung` (gebaut mit `npm --prefix ui run verwaltung`),
//! oder das Verzeichnis aus `SPRECHWERK_VERWALTUNG_DIST`. Jede Datei wird mit
//! `include_bytes!` Teil des Programms — der Server bleibt **eine** Datei, auch
//! unter Windows und macOS, und im Container braucht es kein Node.
//!
//! Fehlt das Verzeichnis, baut der Server trotzdem: `cargo test` und
//! `cargo clippy` dürfen nicht von Node abhängen. Die Oberfläche zeigt dann
//! eine Seite, die sagt, wie man sie baut.
//!
//! Neu gebaut wird, wenn sich das Verzeichnis ändert. Fehlt es, beobachtet
//! Cargo nur diese Datei — `npm run verwaltung` berührt sie deshalb nach dem
//! Bauen (`touch`), damit der nächste `cargo build` die Oberfläche aufnimmt.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=SPRECHWERK_VERWALTUNG_DIST");
    println!("cargo:rerun-if-changed=build.rs");

    let wurzel = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("Cargo"));
    let quelle = std::env::var_os("SPRECHWERK_VERWALTUNG_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| wurzel.join("../../ui/dist-verwaltung"));

    let mut dateien = Vec::new();
    if quelle.is_dir() {
        println!("cargo:rerun-if-changed={}", quelle.display());
        sammeln(&quelle, &quelle, &mut dateien);
        dateien.sort();
    }

    let mut code = String::from("pub static DATEIEN: &[(&str, &[u8])] = &[\n");
    for (name, pfad) in &dateien {
        // `absolute` statt `canonicalize`: unter Windows setzte das ein
        // `\\?\`-Präfix davor. `include_bytes!` braucht nur einen absoluten Pfad
        // (ein relativer gälte relativ zu `OUT_DIR`).
        let pfad = std::path::absolute(pfad).expect("Pfad");
        writeln!(
            code,
            "    ({name:?}, include_bytes!({:?})),",
            pfad.display().to_string()
        )
        .expect("schreiben");
    }
    code.push_str("];\n");

    let ziel =
        PathBuf::from(std::env::var("OUT_DIR").expect("Cargo")).join("verwaltung_dateien.rs");
    std::fs::write(ziel, code).expect("verwaltung_dateien.rs schreiben");
}

/// Alle Dateien unter `ordner`, mit Pfad relativ zu `wurzel` und `/` als
/// Trenner (auch unter Windows: so heißen sie in der URL).
fn sammeln(wurzel: &Path, ordner: &Path, dateien: &mut Vec<(String, PathBuf)>) {
    let Ok(eintraege) = std::fs::read_dir(ordner) else {
        return;
    };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        if pfad.is_dir() {
            sammeln(wurzel, &pfad, dateien);
        } else if let Ok(relativ) = pfad.strip_prefix(wurzel) {
            let name = relativ
                .components()
                .map(|t| t.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            dateien.push((name, pfad));
        }
    }
}

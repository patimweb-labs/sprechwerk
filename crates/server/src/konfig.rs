//! Konfiguration: Datei plus Umgebung.
//!
//! **Festlegung (SPEC §2):** Konfiguration ist eine Datei im Volume, einzelne
//! Werte lassen sich über Umgebungsvariablen überschreiben (für Docker). Die
//! Verwaltungsoberfläche (`[verwaltung]`, SPEC §2 Nachtrag 2026-11-17) zeigt
//! Protokoll und Rollen, ändert aber keine Konfiguration — die bleibt hier.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Fehler beim Lesen der Konfiguration.
#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("Konfigurationsdatei: {0}")]
    Datei(#[from] std::io::Error),
    #[error("Konfiguration unlesbar: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("ungültiger Wert: {0}")]
    Wert(String),
}

/// Mindestlänge eines selbst gewählten Zugangscodes der Verwaltung.
pub const MINDESTLAENGE_CODE: usize = 16;

/// Die vollständige Konfiguration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Konfig {
    pub server: Server,
    pub sicherung: Sicherung,
    pub verwaltung: Verwaltung,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    /// Anzeigename des Servers.
    pub name: String,
    /// Adresse, auf der gelauscht wird.
    pub bind: String,
    /// UDP-Port (QUIC).
    pub port: u16,
    /// Optionales Serverpasswort; leer = offen (wie TeamSpeak ohne Passwort).
    pub passwort: String,
    /// Rolle, die neue Mitglieder bekommen.
    pub standard_rolle: String,
    /// Verzeichnis für Datenbank, Zertifikat und Sicherungen.
    pub daten: PathBuf,
    /// Wie viele Verbindungen je Adresse gleichzeitig erlaubt sind.
    pub verbindungen_je_adresse: usize,
    /// Wie viele **Steuerrahmen** eine Verbindung je Sekunde schicken darf.
    ///
    /// Sprache läuft als Datagramm daneben und zählt nicht mit. Der Wert ist
    /// großzügig: ein Mensch erzeugt wenige Rahmen je Sekunde (Text, Raumwechsel,
    /// Ping). Wer mehr schickt, programmiert — oder greift an. 0 = unbegrenzt.
    pub rahmen_je_sekunde: u32,
    /// Aufbewahrung von Nachrichten in Tagen; 0 = unbegrenzt.
    pub aufbewahrung_tage: u64,
    /// Staukontrolle für die Sprachverbindung: `bbr` oder `cubic`.
    ///
    /// Cubic halbiert bei Paketverlust die Senderate — gut für Dateien,
    /// schlecht für Sprache. BBR schätzt die Bandbreite und reagiert
    /// gleichmäßiger. Messen, nicht glauben (`scripts/latenz.sh`).
    pub staukontrolle: String,
    /// Sendepuffer für Sprache in KiB. Klein halten: bei Sprache ist
    /// Verwerfen besser als Anstauen.
    pub sprachpuffer_kib: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sicherung {
    /// Sicherung nach Zeitplan.
    pub aktiv: bool,
    /// Abstand in Minuten.
    pub intervall_minuten: u64,
    /// So viele Sicherungen werden behalten.
    pub behalten: usize,
    /// Zielverzeichnis; leer = `<daten>/sicherungen`.
    pub verzeichnis: PathBuf,
}

/// Die Verwaltungsoberfläche im Browser (`crates/server/src/verwaltung.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Verwaltung {
    /// Oberfläche anbieten.
    pub aktiv: bool,
    /// Adresse für HTTP. Vorgabe `127.0.0.1`: nur auf diesem Rechner. Im
    /// Container `0.0.0.0`, der Port wird dort nur an `127.0.0.1` des Hosts
    /// gebunden (`docker-compose.yml`).
    pub bind: String,
    /// TCP-Port.
    pub port: u16,
    /// Zugangscode; leer = bei jedem Start ein neuer (steht im Protokoll).
    pub code: String,
    /// Beim Start den Browser öffnen — nur, wenn es einen Bildschirm gibt.
    pub browser: bool,
}

impl Default for Verwaltung {
    fn default() -> Self {
        Self {
            aktiv: true,
            bind: "127.0.0.1".to_string(),
            port: 7444,
            code: String::new(),
            browser: true,
        }
    }
}

/// Vorgabe für das Datenverzeichnis: `/data`, wenn es das gibt (Container,
/// bisherige Installationen), sonst `daten` **neben dem Programm**.
///
/// Warum nicht immer `/data`: wer das Programm unter Windows oder macOS per
/// Doppelklick startet, hat kein `/data` — unter Windows hieße das `C:\data`.
/// Neben dem Programm liegt es dort, wo man es sucht, und wandert mit, wenn man
/// den Ordner verschiebt.
fn daten_vorgabe() -> PathBuf {
    let container = PathBuf::from("/data");
    if container.is_dir() {
        return container;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|ordner| ordner.join("daten")))
        .unwrap_or_else(|| PathBuf::from("daten"))
}

impl Default for Server {
    fn default() -> Self {
        Self {
            name: "Sprechwerk".to_string(),
            bind: "0.0.0.0".to_string(),
            port: 7443,
            passwort: String::new(),
            standard_rolle: "mitglied".to_string(),
            daten: daten_vorgabe(),
            verbindungen_je_adresse: 8,
            rahmen_je_sekunde: 100,
            aufbewahrung_tage: 0,
            staukontrolle: "bbr".to_string(),
            sprachpuffer_kib: 64,
        }
    }
}

impl Default for Sicherung {
    fn default() -> Self {
        Self {
            aktiv: true,
            intervall_minuten: 1_440,
            behalten: 14,
            verzeichnis: PathBuf::new(),
        }
    }
}

impl Konfig {
    /// Lädt aus Datei (falls vorhanden) und überschreibt mit Umgebung.
    pub fn laden(pfad: impl AsRef<Path>) -> Result<Self, Fehler> {
        let pfad = pfad.as_ref();
        let mut konfig = match std::fs::read_to_string(pfad) {
            Ok(text) => toml::from_str(&text)?,
            Err(fehler) if fehler.kind() == std::io::ErrorKind::NotFound => Konfig::default(),
            Err(fehler) => return Err(Fehler::Datei(fehler)),
        };
        konfig.aus_umgebung()?;
        konfig.pruefen()?;
        Ok(konfig)
    }

    /// Umgebungsvariablen haben Vorrang (Docker-Kompatibilität).
    fn aus_umgebung(&mut self) -> Result<(), Fehler> {
        fn text(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|w| !w.is_empty())
        }
        fn zahl<T: std::str::FromStr>(name: &str) -> Result<Option<T>, Fehler> {
            match text(name) {
                Some(w) => w
                    .parse::<T>()
                    .map(Some)
                    .map_err(|_| Fehler::Wert(format!("{name}={w}"))),
                None => Ok(None),
            }
        }
        fn ja(name: &str) -> Option<bool> {
            text(name).map(|w| matches!(w.as_str(), "1" | "true" | "ja" | "yes" | "on"))
        }

        if let Some(w) = text("SPRECHWERK_NAME") {
            self.server.name = w;
        }
        if let Some(w) = text("SPRECHWERK_BIND") {
            self.server.bind = w;
        }
        if let Some(w) = zahl("SPRECHWERK_PORT")? {
            self.server.port = w;
        }
        if let Some(w) = text("SPRECHWERK_PASSWORT") {
            self.server.passwort = w;
        }
        if let Some(w) = text("SPRECHWERK_STANDARD_ROLLE") {
            self.server.standard_rolle = w;
        }
        if let Some(w) = text("SPRECHWERK_DATEN") {
            self.server.daten = PathBuf::from(w);
        }
        if let Some(w) = zahl("SPRECHWERK_AUFBEWAHRUNG_TAGE")? {
            self.server.aufbewahrung_tage = w;
        }
        if let Some(w) = text("SPRECHWERK_STAUKONTROLLE") {
            self.server.staukontrolle = w;
        }
        if let Some(w) = zahl::<u32>("SPRECHWERK_RAHMEN_JE_SEKUNDE")? {
            self.server.rahmen_je_sekunde = w;
        }
        if let Some(w) = zahl("SPRECHWERK_SPRACHPUFFER_KIB")? {
            self.server.sprachpuffer_kib = w;
        }
        if let Some(w) = ja("SPRECHWERK_SICHERUNG_AKTIV") {
            self.sicherung.aktiv = w;
        }
        if let Some(w) = zahl("SPRECHWERK_SICHERUNG_INTERVALL_MINUTEN")? {
            self.sicherung.intervall_minuten = w;
        }
        if let Some(w) = zahl("SPRECHWERK_SICHERUNG_BEHALTEN")? {
            self.sicherung.behalten = w;
        }
        if let Some(w) = text("SPRECHWERK_SICHERUNG_VERZEICHNIS") {
            self.sicherung.verzeichnis = PathBuf::from(w);
        }
        if let Some(w) = ja("SPRECHWERK_VERWALTUNG_AKTIV") {
            self.verwaltung.aktiv = w;
        }
        if let Some(w) = text("SPRECHWERK_VERWALTUNG_BIND") {
            self.verwaltung.bind = w;
        }
        if let Some(w) = zahl("SPRECHWERK_VERWALTUNG_PORT")? {
            self.verwaltung.port = w;
        }
        if let Some(w) = text("SPRECHWERK_VERWALTUNG_CODE") {
            self.verwaltung.code = w;
        }
        if let Some(w) = ja("SPRECHWERK_VERWALTUNG_BROWSER") {
            self.verwaltung.browser = w;
        }
        Ok(())
    }

    /// Prüft Werte, die sonst erst im Betrieb auffallen.
    fn pruefen(&mut self) -> Result<(), Fehler> {
        if self.server.port == 0 {
            return Err(Fehler::Wert("port darf nicht 0 sein".into()));
        }
        if sprechwerk_domain::Role::aus_text(&self.server.standard_rolle).is_none() {
            return Err(Fehler::Wert(format!(
                "standard_rolle={} (erlaubt: administrator, moderator, mitglied, gast)",
                self.server.standard_rolle
            )));
        }
        if !matches!(self.server.staukontrolle.as_str(), "bbr" | "cubic") {
            return Err(Fehler::Wert(format!(
                "staukontrolle={} (erlaubt: bbr, cubic)",
                self.server.staukontrolle
            )));
        }
        if self.server.sprachpuffer_kib < 8 || self.server.sprachpuffer_kib > 4096 {
            return Err(Fehler::Wert(
                "sprachpuffer_kib muss zwischen 8 und 4096 liegen".into(),
            ));
        }
        if self.server.daten.as_os_str().is_empty() {
            return Err(Fehler::Wert("daten darf nicht leer sein".into()));
        }
        if self.sicherung.intervall_minuten == 0 {
            // Ein Intervall von 0 würde in einer Schleife sichern.
            self.sicherung.aktiv = false;
        }
        if self.sicherung.behalten == 0 {
            return Err(Fehler::Wert(
                "sicherung.behalten muss mindestens 1 sein".into(),
            ));
        }
        if self.sicherung.verzeichnis.as_os_str().is_empty() {
            self.sicherung.verzeichnis = self.server.daten.join("sicherungen");
        }
        if self.verwaltung.port == 0 {
            return Err(Fehler::Wert("verwaltung.port darf nicht 0 sein".into()));
        }
        // Ein selbst gewählter Code ersetzt den zufälligen (128 Bit). Kurze
        // Codes lassen sich durchprobieren — die Oberfläche kann Rollen
        // vergeben, also auch den Administrator.
        let code = self.verwaltung.code.trim();
        if !code.is_empty() && code.chars().count() < MINDESTLAENGE_CODE {
            return Err(Fehler::Wert(format!(
                "verwaltung.code muss mindestens {MINDESTLAENGE_CODE} Zeichen haben (leer = zufällig)"
            )));
        }
        self.verwaltung.code = code.to_string();
        Ok(())
    }

    /// Standardrolle als Aufzählungswert.
    pub fn standard_rolle(&self) -> sprechwerk_domain::Role {
        sprechwerk_domain::Role::aus_text(&self.server.standard_rolle)
            .unwrap_or(sprechwerk_domain::Role::Mitglied)
    }

    /// Adresse zum Lauschen.
    pub fn adresse(&self) -> String {
        format!("{}:{}", self.server.bind, self.server.port)
    }

    /// Adresse der Verwaltungsoberfläche.
    pub fn verwaltung_adresse(&self) -> String {
        format!("{}:{}", self.verwaltung.bind, self.verwaltung.port)
    }

    /// Pfad der Datenbankdatei.
    pub fn datenbank(&self) -> PathBuf {
        self.server.daten.join("sprechwerk.sqlitedb")
    }

    /// Beispielkonfiguration zum Hineinkopieren.
    pub fn beispiel() -> String {
        r#"# sprechwerk — Konfiguration
# Jeder Wert lässt sich über eine Umgebungsvariable überschreiben
# (SPRECHWERK_PORT, SPRECHWERK_NAME, SPRECHWERK_PASSWORT, SPRECHWERK_DATEN, …).

[server]
name = "Sprechwerk"
bind = "0.0.0.0"
port = 7443
passwort = ""            # leer = offen, wie TeamSpeak ohne Serverpasswort
standard_rolle = "mitglied"   # administrator | moderator | mitglied | gast
# daten = "/data"         # ohne Angabe: /data, wenn vorhanden, sonst daten/ neben dem Programm
verbindungen_je_adresse = 8
rahmen_je_sekunde = 100  # Steuerrahmen je Verbindung (Sprache zählt nicht); 0 = aus
aufbewahrung_tage = 0    # 0 = unbegrenzt
staukontrolle = "bbr"    # bbr | cubic — bbr ist für Sprache gleichmäßiger
sprachpuffer_kib = 64    # klein halten: verwerfen statt anstauen

[sicherung]
aktiv = true
intervall_minuten = 1440  # täglich
behalten = 14
# verzeichnis = "/data/sicherungen"

[verwaltung]
# Verwaltung im Browser: Protokoll, Mitglieder, Rollen (http://127.0.0.1:7444).
aktiv = true
bind = "127.0.0.1"        # nur dieser Rechner; im Container 0.0.0.0 (siehe docker-compose.yml)
port = 7444               # TCP
code = ""                 # leer = bei jedem Start neu, steht im Protokoll; sonst mind. 16 Zeichen
browser = true            # beim Start öffnen, wenn ein Bildschirm da ist
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datei(inhalt: &str) -> PathBuf {
        let pfad = std::env::temp_dir().join(format!(
            "sprechwerk-konfig-{}-{}.toml",
            std::process::id(),
            inhalt.len()
        ));
        std::fs::write(&pfad, inhalt).unwrap();
        pfad
    }

    #[test]
    fn ohne_datei_gelten_vorgaben() {
        let k = Konfig::laden("/gibt/es/nicht.toml").unwrap();
        assert_eq!(k.server.port, 7443);
        assert_eq!(k.server.name, "Sprechwerk");
        assert_eq!(k.standard_rolle(), sprechwerk_domain::Role::Mitglied);
    }

    #[test]
    fn datei_wird_gelesen() {
        let pfad = datei("[server]\nname = \"Verein\"\nport = 9000\n[sicherung]\nbehalten = 3\n");
        let k = Konfig::laden(&pfad).unwrap();
        assert_eq!(k.server.name, "Verein");
        assert_eq!(k.server.port, 9000);
        assert_eq!(k.sicherung.behalten, 3);
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn umgebung_hat_vorrang() {
        let pfad = datei("[server]\nname = \"Aus Datei\"\n");
        std::env::set_var("SPRECHWERK_NAME", "Aus Umgebung");
        let k = Konfig::laden(&pfad).unwrap();
        std::env::remove_var("SPRECHWERK_NAME");
        assert_eq!(k.server.name, "Aus Umgebung");
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn unbekannte_rolle_wird_abgelehnt() {
        let pfad = datei("[server]\nstandard_rolle = \"chef\"\n");
        assert!(matches!(Konfig::laden(&pfad), Err(Fehler::Wert(_))));
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn tippfehler_in_der_datei_werden_gemeldet() {
        let pfad = datei("[server]\nprot = 9000\n");
        assert!(matches!(Konfig::laden(&pfad), Err(Fehler::Toml(_))));
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn verwaltung_ist_vorgabe_nur_lokal() {
        let k = Konfig::laden("/gibt/es/nicht.toml").unwrap();
        assert!(k.verwaltung.aktiv);
        assert_eq!(k.verwaltung_adresse(), "127.0.0.1:7444");
        assert!(k.verwaltung.code.is_empty());
    }

    #[test]
    fn kurzer_verwaltungscode_wird_abgelehnt() {
        let pfad = datei("[verwaltung]\ncode = \"kurz\"\n");
        assert!(matches!(Konfig::laden(&pfad), Err(Fehler::Wert(_))));
        let _ = std::fs::remove_file(&pfad);
        let pfad = datei("[verwaltung]\ncode = \"  ein-langer-code-1234  \"\nport = 9001\n");
        let k = Konfig::laden(&pfad).unwrap();
        assert_eq!(k.verwaltung.code, "ein-langer-code-1234");
        assert_eq!(k.verwaltung.port, 9001);
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn beispiel_ist_gueltig() {
        let pfad = datei(&Konfig::beispiel());
        let k = Konfig::laden(&pfad).unwrap();
        assert_eq!(k.verwaltung.port, 7444);
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn sicherungsverzeichnis_ist_unter_daten() {
        let pfad = datei("[server]\ndaten = \"/tmp/sprechwerk-x\"\n");
        let k = Konfig::laden(&pfad).unwrap();
        assert_eq!(
            k.sicherung.verzeichnis,
            PathBuf::from("/tmp/sprechwerk-x/sicherungen")
        );
        assert_eq!(
            k.datenbank(),
            PathBuf::from("/tmp/sprechwerk-x/sprechwerk.sqlitedb")
        );
        let _ = std::fs::remove_file(&pfad);
    }
}

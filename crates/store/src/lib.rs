//! Persistenz.
//!
//! **Festlegung (SPEC §2): die Datenbank ist Betriebsmittel, der Text ist
//! Wahrheit.** Konkret:
//!
//! * Der laufende Zustand liegt im Speicher (`sprechwerk_domain::State`).
//! * Nach jeder Änderung wird er in die Datenbank geschrieben (Write-Through),
//!   damit ein Absturz höchstens die letzte Änderung kostet.
//! * `exportiern` schreibt den **kompletten** Zustand als JSON, mit
//!   `format_version`. `importieren` baut daraus einen Zustand — unabhängig
//!   davon, welche Programmversion den Text geschrieben hat, solange die
//!   Version nicht größer ist als die eigene.
//! * Sicherungen sind solche Textdateien. Sie sind Menschenlesbar, mit `jq`
//!   filterbar und ohne dieses Programm prüfbar.

use rusqlite::{params, Connection, OptionalExtension};
use sprechwerk_domain::{
    AuditEntry, Ban, Member, MemberId, Message, MessageId, Role, Room, RoomId, State,
};
use std::path::{Path, PathBuf};

/// Fehler der Persistenz.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Datenbank: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unbekanntes Format {0} (dieses Programm kann bis {1})")]
    Format(u32, u32),
    #[error("ungültiger Wert in der Datenbank: {0}")]
    Wert(String),
    #[error("Datei: {0}")]
    Io(#[from] std::io::Error),
    #[error("Zustand: {0}")]
    Zustand(#[from] sprechwerk_domain::Error),
}

type Ergebnis<T> = Result<T, Error>;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  schluessel TEXT PRIMARY KEY,
  wert       TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS raeume (
  id           TEXT PRIMARY KEY,
  eltern       TEXT,
  name         TEXT NOT NULL,
  reihenfolge  INTEGER NOT NULL,
  angelegt_am  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mitglieder (
  id               TEXT PRIMARY KEY,
  schluessel       TEXT NOT NULL UNIQUE,
  name             TEXT NOT NULL,
  rolle            TEXT NOT NULL,
  raum             TEXT,
  raum_seit        INTEGER,
  angelegt_am      INTEGER NOT NULL,
  zuletzt_gesehen  INTEGER NOT NULL,
  stumm            INTEGER NOT NULL DEFAULT 0,
  taub             INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS nachrichten (
  id              TEXT PRIMARY KEY,
  raum            TEXT NOT NULL,
  verfasser       TEXT NOT NULL,
  verfasser_name  TEXT NOT NULL,
  text            TEXT NOT NULL,
  zeit            INTEGER NOT NULL,
  geloescht       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_nachrichten_raum_zeit ON nachrichten (raum, zeit);
CREATE TABLE IF NOT EXISTS protokoll (
  laufnummer       INTEGER PRIMARY KEY,
  zeit             INTEGER NOT NULL,
  handelnder       TEXT,
  handelnder_name  TEXT NOT NULL,
  aktion           TEXT NOT NULL,
  ziel             TEXT NOT NULL,
  vorher           TEXT,
  nachher          TEXT,
  ergebnis         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_protokoll_zeit ON protokoll (zeit);
CREATE TABLE IF NOT EXISTS sperren (
  laufnummer       INTEGER PRIMARY KEY AUTOINCREMENT,
  schluessel       TEXT,
  adresse          TEXT,
  grund            TEXT NOT NULL,
  zeit             INTEGER NOT NULL,
  handelnder_name  TEXT NOT NULL
);
"#;

/// Kennung der aktuellen Datenbankstruktur.
pub const SCHEMA_VERSION: u32 = 1;

/// Ein Store auf einer SQLite-Datei.
pub struct Store {
    conn: Connection,
    pfad: PathBuf,
}

impl Store {
    /// Öffnet (oder erzeugt) die Datenbank und legt das Schema an.
    pub fn oeffnen(pfad: impl AsRef<Path>) -> Ergebnis<Self> {
        let pfad = pfad.as_ref().to_path_buf();
        if let Some(eltern) = pfad.parent() {
            std::fs::create_dir_all(eltern)?;
        }
        let conn = Connection::open(&pfad)?;
        // WAL: Leser blockieren Schreiber nicht — und eine Sicherung im
        // laufenden Betrieb ist dadurch konsistent.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (schluessel, wert) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(Self { conn, pfad })
    }

    /// Pfad der Datenbankdatei.
    pub fn pfad(&self) -> &Path {
        &self.pfad
    }

    /// Version der Datenbankstruktur.
    pub fn schema_version(&self) -> Ergebnis<u32> {
        let wert: Option<String> = self
            .conn
            .query_row(
                "SELECT wert FROM meta WHERE schluessel = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(wert.and_then(|w| w.parse().ok()).unwrap_or(0))
    }

    /// Lädt den Zustand. Ist die Datenbank leer, wird `standard` geliefert.
    pub fn laden(&self, standard: impl FnOnce() -> State) -> Ergebnis<State> {
        let mut state = standard();

        let mut stmt = self.conn.prepare("SELECT schluessel, wert FROM meta")?;
        let mut zeilen = stmt.query([])?;
        while let Some(z) = zeilen.next()? {
            let schluessel: String = z.get(0)?;
            let wert: String = z.get(1)?;
            match schluessel.as_str() {
                "server_name" => state.server_name = wert,
                "standard_rolle" => {
                    if let Some(r) = Role::aus_text(&wert) {
                        state.standard_rolle = r;
                    }
                }
                "format_version" => {
                    let v: u32 = wert.parse().unwrap_or(0);
                    if v > sprechwerk_domain::state::FORMAT_VERSION {
                        return Err(Error::Format(v, sprechwerk_domain::state::FORMAT_VERSION));
                    }
                    state.format_version = v;
                }
                "naechste_laufnummer" => {
                    state.setze_naechste_laufnummer(wert.parse().unwrap_or(1));
                }
                _ => {}
            }
        }

        // Räume
        let mut stmt = self
            .conn
            .prepare("SELECT id, eltern, name, reihenfolge, angelegt_am FROM raeume")?;
        let mut zeilen = stmt.query([])?;
        let mut raeume = std::collections::BTreeMap::new();
        while let Some(z) = zeilen.next()? {
            let id = RoomId::from_text(&z.get::<_, String>(0)?)?;
            let _alte_eltern: Option<String> = z.get(1)?;
            raeume.insert(
                id,
                Room {
                    id,
                    name: z.get(2)?,
                    reihenfolge: z.get(3)?,
                    angelegt_am: z.get(4)?,
                },
            );
        }
        state.raeume = raeume;

        // Mitglieder
        let mut stmt = self.conn.prepare(
            "SELECT id, schluessel, name, rolle, raum, raum_seit, angelegt_am, zuletzt_gesehen, stumm, taub FROM mitglieder",
        )?;
        let mut zeilen = stmt.query([])?;
        let mut mitglieder = std::collections::BTreeMap::new();
        while let Some(z) = zeilen.next()? {
            let id = MemberId::from_text(&z.get::<_, String>(0)?)?;
            let rolle_text: String = z.get(3)?;
            let rolle = Role::aus_text(&rolle_text)
                .ok_or_else(|| Error::Wert(format!("unbekannte Rolle: {rolle_text}")))?;
            let raum = match z.get::<_, Option<String>>(4)? {
                Some(t) => Some(RoomId::from_text(&t)?),
                None => None,
            };
            mitglieder.insert(
                id,
                Member {
                    id,
                    schluessel: z.get(1)?,
                    name: z.get(2)?,
                    rolle,
                    raum,
                    raum_seit: z.get(5)?,
                    angelegt_am: z.get(6)?,
                    zuletzt_gesehen: z.get(7)?,
                    stumm: z.get::<_, i64>(8)? != 0,
                    taub: z.get::<_, i64>(9)? != 0,
                },
            );
        }
        state.mitglieder = mitglieder;

        // Nachrichten
        let mut stmt = self.conn.prepare(
            "SELECT id, raum, verfasser, verfasser_name, text, zeit, geloescht FROM nachrichten ORDER BY zeit, id",
        )?;
        let mut zeilen = stmt.query([])?;
        let mut nachrichten = Vec::new();
        while let Some(z) = zeilen.next()? {
            nachrichten.push(Message {
                id: MessageId::from_text(&z.get::<_, String>(0)?)?,
                raum: RoomId::from_text(&z.get::<_, String>(1)?)?,
                verfasser: MemberId::from_text(&z.get::<_, String>(2)?)?,
                verfasser_name: z.get(3)?,
                text: z.get(4)?,
                zeit: z.get(5)?,
                geloescht: z.get::<_, i64>(6)? != 0,
            });
        }
        state.nachrichten = nachrichten;

        // Protokoll
        let mut stmt = self.conn.prepare(
            "SELECT laufnummer, zeit, handelnder, handelnder_name, aktion, ziel, vorher, nachher, ergebnis \
             FROM protokoll ORDER BY laufnummer",
        )?;
        let mut zeilen = stmt.query([])?;
        let mut protokoll = Vec::new();
        while let Some(z) = zeilen.next()? {
            let handelnder = match z.get::<_, Option<String>>(2)? {
                Some(t) => Some(MemberId::from_text(&t)?),
                None => None,
            };
            protokoll.push(AuditEntry {
                laufnummer: z.get(0)?,
                zeit: z.get(1)?,
                handelnder,
                handelnder_name: z.get(3)?,
                aktion: z.get(4)?,
                ziel: z.get(5)?,
                vorher: z.get(6)?,
                nachher: z.get(7)?,
                ergebnis: z.get(8)?,
            });
        }
        state.protokoll = protokoll;

        // Sperren
        let mut stmt = self
            .conn
            .prepare("SELECT schluessel, adresse, grund, zeit, handelnder_name FROM sperren ORDER BY laufnummer")?;
        let mut zeilen = stmt.query([])?;
        let mut sperren = Vec::new();
        while let Some(z) = zeilen.next()? {
            sperren.push(Ban {
                schluessel: z.get(0)?,
                adresse: z.get(1)?,
                grund: z.get(2)?,
                zeit: z.get(3)?,
                handelnder_name: z.get(4)?,
            });
        }
        state.sperren = sperren;

        Ok(state)
    }

    /// Schreibt den Zustand vollständig in die Datenbank.
    ///
    /// **Einfachheit vor Feingefühl:** Räume, Mitglieder und Sperren werden
    /// ersetzt, Nachrichten und Protokoll nur ergänzt (sie wachsen und dürfen
    /// nicht neu geschrieben werden). Bei einigen hundert Mitgliedern und
    /// einigen zehntausend Nachrichten ist das schnell genug; wenn nicht, wird
    /// hier auf gezielte Schreibvorgänge umgestellt.
    pub fn speichern(&mut self, state: &State) -> Ergebnis<()> {
        let tx = self.conn.transaction()?;

        tx.execute(
            "INSERT INTO meta (schluessel, wert) VALUES ('server_name', ?1) \
             ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
            params![state.server_name],
        )?;
        tx.execute(
            "INSERT INTO meta (schluessel, wert) VALUES ('standard_rolle', ?1) \
             ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
            params![state.standard_rolle.text()],
        )?;
        tx.execute(
            "INSERT INTO meta (schluessel, wert) VALUES ('format_version', ?1) \
             ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
            params![state.format_version.to_string()],
        )?;
        tx.execute(
            "INSERT INTO meta (schluessel, wert) VALUES ('naechste_laufnummer', ?1) \
             ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
            params![state.naechste_laufnummer().to_string()],
        )?;

        // Räume: vorhandene Zeilen aktualisieren, fehlende löschen.
        {
            let mut vorhanden = std::collections::HashSet::new();
            for r in state.raeume.values() {
                vorhanden.insert(r.id.text());
                tx.execute(
                    "INSERT INTO raeume (id, eltern, name, reihenfolge, angelegt_am) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(id) DO UPDATE SET eltern = excluded.eltern, name = excluded.name, \
                     reihenfolge = excluded.reihenfolge, angelegt_am = excluded.angelegt_am",
                    params![
                        r.id.text(),
                        Option::<String>::None,
                        r.name,
                        r.reihenfolge,
                        r.angelegt_am
                    ],
                )?;
            }
            let mut stmt = tx.prepare("SELECT id FROM raeume")?;
            let ids: Vec<String> = stmt
                .query_map([], |z| z.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for id in ids {
                if !vorhanden.contains(&id) {
                    tx.execute("DELETE FROM raeume WHERE id = ?1", params![id])?;
                }
            }
        }

        // Mitglieder
        {
            let mut vorhanden = std::collections::HashSet::new();
            for m in state.mitglieder.values() {
                vorhanden.insert(m.id.text());
                tx.execute(
                    "INSERT INTO mitglieder (id, schluessel, name, rolle, raum, raum_seit, angelegt_am, zuletzt_gesehen, stumm, taub) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                     ON CONFLICT(id) DO UPDATE SET schluessel = excluded.schluessel, name = excluded.name, \
                     rolle = excluded.rolle, raum = excluded.raum, raum_seit = excluded.raum_seit, \
                     zuletzt_gesehen = excluded.zuletzt_gesehen, stumm = excluded.stumm, taub = excluded.taub",
                    params![
                        m.id.text(),
                        m.schluessel,
                        m.name,
                        m.rolle.text(),
                        m.raum.map(|r| r.text()),
                        m.raum_seit,
                        m.angelegt_am,
                        m.zuletzt_gesehen,
                        m.stumm as i64,
                        m.taub as i64
                    ],
                )?;
            }
            let mut stmt = tx.prepare("SELECT id FROM mitglieder")?;
            let ids: Vec<String> = stmt
                .query_map([], |z| z.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for id in ids {
                if !vorhanden.contains(&id) {
                    tx.execute("DELETE FROM mitglieder WHERE id = ?1", params![id])?;
                }
            }
        }

        // Nachrichten: nur ergänzen und den Löschzustand nachziehen.
        for n in &state.nachrichten {
            tx.execute(
                "INSERT INTO nachrichten (id, raum, verfasser, verfasser_name, text, zeit, geloescht) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(id) DO UPDATE SET text = excluded.text, geloescht = excluded.geloescht",
                params![
                    n.id.text(),
                    n.raum.text(),
                    n.verfasser.text(),
                    n.verfasser_name,
                    n.text,
                    n.zeit,
                    n.geloescht as i64
                ],
            )?;
        }
        {
            let vorhanden: std::collections::HashSet<String> =
                state.nachrichten.iter().map(|n| n.id.text()).collect();
            let mut stmt = tx.prepare("SELECT id FROM nachrichten")?;
            let ids: Vec<String> = stmt
                .query_map([], |z| z.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for id in ids {
                if !vorhanden.contains(&id) {
                    tx.execute("DELETE FROM nachrichten WHERE id = ?1", params![id])?;
                }
            }
        }

        // Protokoll: nur anhängen.
        for e in &state.protokoll {
            tx.execute(
                "INSERT OR IGNORE INTO protokoll (laufnummer, zeit, handelnder, handelnder_name, aktion, ziel, vorher, nachher, ergebnis) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    e.laufnummer,
                    e.zeit,
                    e.handelnder.map(|h| h.text()),
                    e.handelnder_name,
                    e.aktion,
                    e.ziel,
                    e.vorher,
                    e.nachher,
                    e.ergebnis
                ],
            )?;
        }

        // Sperren: ersetzen (es sind wenige).
        tx.execute("DELETE FROM sperren", [])?;
        for b in &state.sperren {
            tx.execute(
                "INSERT INTO sperren (schluessel, adresse, grund, zeit, handelnder_name) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![b.schluessel, b.adresse, b.grund, b.zeit, b.handelnder_name],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Schreibt den Zustand als Text (Exportformat, **Festlegung SPEC §2**).
    pub fn exportieren(state: &State, ziel: impl AsRef<Path>) -> Ergebnis<()> {
        let text = serde_json::to_string_pretty(state)?;
        let ziel = ziel.as_ref();
        if let Some(eltern) = ziel.parent() {
            std::fs::create_dir_all(eltern)?;
        }
        // Erst in eine Nebendatei, dann umbenennen: eine halb geschriebene
        // Sicherung darf nie als vollständig erscheinen.
        let zwischen = ziel.with_extension("tmp");
        std::fs::write(&zwischen, text)?;
        std::fs::rename(&zwischen, ziel)?;
        Ok(())
    }

    /// Liest einen Zustand aus dem Textformat.
    pub fn importieren(quelle: impl AsRef<Path>) -> Ergebnis<State> {
        let text = std::fs::read_to_string(quelle)?;
        let state: State = serde_json::from_str(&text)?;
        if state.format_version > sprechwerk_domain::state::FORMAT_VERSION {
            return Err(Error::Format(
                state.format_version,
                sprechwerk_domain::state::FORMAT_VERSION,
            ));
        }
        Ok(state)
    }

    /// Zeitgestempelte Sicherung im Verzeichnis, die letzten `behalten` bleiben.
    pub fn sichern(
        state: &State,
        verzeichnis: impl AsRef<Path>,
        behalten: usize,
        jetzt_ms: i64,
    ) -> Ergebnis<PathBuf> {
        let verzeichnis = verzeichnis.as_ref().to_path_buf();
        std::fs::create_dir_all(&verzeichnis)?;
        let name = format!("sprechwerk-{jetzt_ms}.json");
        let ziel = verzeichnis.join(&name);
        Self::exportieren(state, &ziel)?;

        // Aufräumen: nur Dateien dieses Musters, älteste zuerst.
        let mut vorhanden: Vec<PathBuf> = std::fs::read_dir(&verzeichnis)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("sprechwerk-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        vorhanden.sort();
        if behalten > 0 && vorhanden.len() > behalten {
            for alt in &vorhanden[..vorhanden.len() - behalten] {
                std::fs::remove_file(alt)?;
            }
        }
        Ok(ziel)
    }

    /// Konsistente Kopie der Datenbankdatei (SQLite-Sicherung).
    pub fn datenbank_sichern(&self, ziel: impl AsRef<Path>) -> Ergebnis<()> {
        let ziel = ziel.as_ref();
        if let Some(eltern) = ziel.parent() {
            std::fs::create_dir_all(eltern)?;
        }
        let zwischen = ziel.with_extension("tmp");
        self.conn
            .execute("VACUUM INTO ?1", params![zwischen.to_string_lossy()])?;
        std::fs::rename(&zwischen, ziel)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_domain::State;

    fn zustand() -> State {
        let mut s = State::neu("Testwerk", Role::Mitglied, 1_000);
        let a = s.anmelden("k-admin", "Ada", 1_000).unwrap();
        let raum = s.raum_anlegen(a, "Vorstand", 1_010).unwrap();
        s.betreten(a, raum, 1_011).unwrap();
        s.text_senden(a, "Protokoll", 1_012).unwrap();
        s.sperren(
            a,
            Some("k-boese".into()),
            Some("192.0.2.7".into()),
            "Spam",
            1_013,
        )
        .unwrap();
        s
    }

    #[test]
    fn speichern_und_laden_ist_verlustfrei() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::oeffnen(dir.join("test.sqlitedb")).unwrap();
        let s = zustand();
        store.speichern(&s).unwrap();

        let geladen = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        assert_eq!(geladen.server_name, s.server_name);
        assert_eq!(geladen.raeume.len(), s.raeume.len());
        assert_eq!(geladen.mitglieder.len(), s.mitglieder.len());
        assert_eq!(geladen.nachrichten, s.nachrichten);
        assert_eq!(geladen.protokoll, s.protokoll);
        assert_eq!(geladen.sperren, s.sperren);
        assert_eq!(geladen.naechste_laufnummer(), s.naechste_laufnummer());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zweimal_speichern_verdoppelt_nichts() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::oeffnen(dir.join("t.sqlitedb")).unwrap();
        let s = zustand();
        store.speichern(&s).unwrap();
        store.speichern(&s).unwrap();
        let geladen = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        assert_eq!(geladen.nachrichten.len(), s.nachrichten.len());
        assert_eq!(geladen.protokoll.len(), s.protokoll.len());
        assert_eq!(geladen.sperren.len(), s.sperren.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geloeschte_nachricht_verschwindet_aus_der_datenbank() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::oeffnen(dir.join("t.sqlitedb")).unwrap();
        let mut s = zustand();
        let a = *s.mitglieder.keys().next().unwrap();
        let id = s.nachrichten[0].id;
        s.nachricht_loeschen(a, id, 2_000).unwrap();
        store.speichern(&s).unwrap();
        let geladen = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        assert_eq!(geladen.nachrichten.len(), s.nachrichten.len());
        assert!(
            geladen
                .nachrichten
                .iter()
                .find(|n| n.id == id)
                .unwrap()
                .geloescht
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_und_import_ueber_datei() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = zustand();
        let datei = dir.join("export.json");
        Store::exportieren(&s, &datei).unwrap();
        let zurueck = Store::importieren(&datei).unwrap();
        assert_eq!(zurueck.server_name, s.server_name);
        assert_eq!(zurueck.nachrichten.len(), s.nachrichten.len());
        // Der Text ist lesbar — das ist der Punkt des Formats.
        let text = std::fs::read_to_string(&datei).unwrap();
        assert!(text.contains("\"format_version\": 1"));
        assert!(text.contains("Protokoll"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sicherung_haelt_nur_die_letzten_n() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = zustand();
        for i in 0..5 {
            Store::sichern(&s, &dir, 2, 1_000 + i).unwrap();
        }
        let anzahl = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .count();
        assert_eq!(anzahl, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zu_neues_format_wird_abgelehnt() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = zustand();
        s.format_version = 99;
        let datei = dir.join("neu.json");
        std::fs::write(&datei, serde_json::to_string(&s).unwrap()).unwrap();
        assert!(matches!(
            Store::importieren(&datei),
            Err(Error::Format(99, _))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn datenbank_kann_im_betrieb_gesichert_werden() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-test7-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::oeffnen(dir.join("t.sqlitedb")).unwrap();
        store.speichern(&zustand()).unwrap();
        let ziel = dir.join("sicherung.sqlitedb");
        store.datenbank_sichern(&ziel).unwrap();
        assert!(ziel.exists());
        let kopie = Store::oeffnen(&ziel).unwrap();
        assert_eq!(
            kopie
                .laden(|| State::neu("x", Role::Gast, 0))
                .unwrap()
                .mitglieder
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

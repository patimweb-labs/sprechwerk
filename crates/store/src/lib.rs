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
  angelegt_am  INTEGER NOT NULL,
  -- Sprache Ende zu Ende verschlüsselt (ADR 0006)
  verschluesselt INTEGER NOT NULL DEFAULT 0,
  verschluesselt_seit INTEGER NOT NULL DEFAULT 0
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
  geloescht       INTEGER NOT NULL DEFAULT 0,
  anhaenge        TEXT NOT NULL DEFAULT ''
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
  ergebnis         TEXT NOT NULL,
  grund            TEXT
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

/// Bringt eine vorhandene Datenbank auf den aktuellen Stand.
///
/// **Vorher wird kopiert** (SPEC §2): eine Migration, die schiefgeht, darf keine
/// Daten kosten. Die Kopie liegt neben der Datenbank und wird nie automatisch
/// gelöscht — sie ist die Rückfahrt.
///
/// Ein **neueres** Format wird abgelehnt: ein älteres Programm kann eine
/// jüngere Struktur nicht verstehen, und ein halbes Schreiben wäre schlimmer als
/// eine klare Absage.
fn migrieren(conn: &Connection, pfad: &Path) -> Ergebnis<()> {
    // Ganz frische Datei: es gibt noch keine `meta`-Tabelle.
    let hat_meta: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if !hat_meta {
        return Ok(());
    }

    let vorhanden: u32 = conn
        .query_row(
            "SELECT wert FROM meta WHERE schluessel = 'schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|w| w.parse().ok())
        .unwrap_or(0);

    if vorhanden == 0 {
        // Frische Datenbank: noch keine Version. Das Schema wird gleich
        // angelegt, die Version danach gesetzt.
        return Ok(());
    }
    if vorhanden > SCHEMA_VERSION {
        return Err(Error::Format(vorhanden, SCHEMA_VERSION));
    }
    if vorhanden == SCHEMA_VERSION {
        return Ok(());
    }

    // Kopie vor der Migration. WAL zuerst leeren, sonst fehlt der letzte Stand
    // in der Datei.
    let _: Option<(i64, i64, i64)> = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let kopie = pfad.with_extension(format!("vor-v{vorhanden}.sqlite"));
    if !kopie.exists() {
        std::fs::copy(pfad, &kopie)?;
    }

    // Schritt für Schritt. Jeder Schritt muss idempotent sein (er kann durch
    // einen Absturz zweimal laufen).
    let mut version = vorhanden;
    while version < SCHEMA_VERSION {
        match version {
            // v1 → v2: Grund im Prüfprotokoll.
            1 => {
                if spalte_fehlt(conn, "protokoll", "grund")? {
                    conn.execute("ALTER TABLE protokoll ADD COLUMN grund TEXT", [])?;
                }
                version = 2;
            }
            // v2 → v3: Anhänge an Nachrichten (ADR 0012 §3).
            2 => {
                anhaenge_nachruesten(conn)?;
                version = 3;
            }
            anderes => {
                return Err(Error::Format(anderes, SCHEMA_VERSION));
            }
        }
        conn.execute(
            "UPDATE meta SET wert = ?1 WHERE schluessel = 'schema_version'",
            params![version.to_string()],
        )?;
    }

    // Nach der Migration prüfen, dass die Version wirklich steht.
    let danach: u32 = conn
        .query_row(
            "SELECT wert FROM meta WHERE schluessel = 'schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|w| w.parse().ok())
        .unwrap_or(0);
    if danach != SCHEMA_VERSION {
        return Err(Error::Format(danach, SCHEMA_VERSION));
    }
    Ok(())
}

/// Fehlt diese Spalte in der Tabelle?
fn spalte_fehlt(conn: &Connection, tabelle: &str, spalte: &str) -> Ergebnis<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({tabelle})"))?;
    let mut zeilen = stmt.query([])?;
    while let Some(z) = zeilen.next()? {
        let name: String = z.get(1)?;
        if name == spalte {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Ergänzt Spalten, die in einer älteren Datenbank fehlen (idempotent).
fn nachruesten(conn: &Connection) -> Ergebnis<()> {
    if spalte_fehlt(conn, "protokoll", "grund")? {
        conn.execute("ALTER TABLE protokoll ADD COLUMN grund TEXT", [])?;
    }
    anhaenge_nachruesten(conn)
}

/// Spalte `anhaenge` an `nachrichten` — nur, wenn es die Tabelle schon gibt
/// (eine sehr alte Datei kennt sie noch nicht; dann legt das Schema sie an).
fn anhaenge_nachruesten(conn: &Connection) -> Ergebnis<()> {
    let tabelle: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'nachrichten'",
        [],
        |r| r.get(0),
    )?;
    if tabelle > 0 && spalte_fehlt(conn, "nachrichten", "anhaenge")? {
        conn.execute(
            "ALTER TABLE nachrichten ADD COLUMN anhaenge TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

/// Anhänge in einer Spalte: Kennungen durch Leerzeichen getrennt. Eine
/// Kennung ist Hex (`protocol::anhang::ist_id`), enthält also keins.
fn anhaenge_schreiben(ids: &[String]) -> String {
    ids.join(" ")
}

fn anhaenge_lesen(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// Kennung der aktuellen Datenbankstruktur.
///
/// 1 → 2: Spalte `grund` im Prüfprotokoll.
/// 2 → 3: Spalte `anhaenge` an den Nachrichten.
pub const SCHEMA_VERSION: u32 = 3;

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
        nur_eigentuemer(&pfad)?;
        let conn = Connection::open(&pfad)?;
        // Gelöschtes überschreiben, nicht nur als frei markieren: sonst läge
        // entfernter Inhalt weiter in der Datei (ADR 0012 §5).
        conn.pragma_update(None, "secure_delete", "ON")?;
        // WAL: Leser blockieren Schreiber nicht — und eine Sicherung im
        // laufenden Betrieb ist dadurch konsistent.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // **Erst die Struktur prüfen, dann anlegen.** `CREATE TABLE IF NOT
        // EXISTS` ändert eine vorhandene Tabelle nicht — eine bestehende
        // Installation bekäme neue Spalten sonst nie und würde beim Lesen
        // scheitern.
        migrieren(&conn, &pfad)?;
        conn.execute_batch(SCHEMA)?;
        // Sicherheitsnetz: einzelne Spalten nachrüsten, falls eine Migration
        // fehlte. Idempotent und billig.
        nachruesten(&conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (schluessel, wert) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(Self { conn, pfad })
    }

    /// Schreibt die Datei neu, ohne freie Seiten (nach dem Entfernen von
    /// Klartext: was nur als frei markiert war, ist danach weg).
    pub fn verdichten(&self) -> Ergebnis<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
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
                "serverschluessel" => {
                    state.serverschluessel = serde_json::from_str(&wert)
                        .map_err(|e| Error::Wert(format!("serverschluessel: {e}")))?;
                }
                _ => {}
            }
        }

        // Räume
        let mut stmt = self.conn.prepare(
            "SELECT id, eltern, name, reihenfolge, angelegt_am, \
                 COALESCE(verschluesselt, 0), COALESCE(verschluesselt_seit, 0) FROM raeume",
        )?;
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
                    verschluesselt: z.get::<_, i64>(5)? != 0,
                    verschluesselt_seit: z.get::<_, i64>(6)?,
                },
            );
        }
        state.raeume = raeume;

        // Mitglieder
        //
        // **`raum` und `raum_seit` werden nicht gelesen** (Spalten bleiben für
        // alte Datenbanken stehen): Anwesenheit ist flüchtig. Ein Neustart
        // zeigte sonst Leute in Räumen, die längst weg sind — und ein
        // Neuankömmling läse „2 anwesend", obwohl nur er da ist (gefunden
        // 2026-11-19). Dasselbe gilt für den Export (`domain::Member`, dort
        // `serde(skip)`).
        let mut stmt = self.conn.prepare(
            "SELECT id, schluessel, name, rolle, angelegt_am, zuletzt_gesehen, stumm, taub FROM mitglieder",
        )?;
        let mut zeilen = stmt.query([])?;
        let mut mitglieder = std::collections::BTreeMap::new();
        while let Some(z) = zeilen.next()? {
            let id = MemberId::from_text(&z.get::<_, String>(0)?)?;
            let rolle_text: String = z.get(3)?;
            let rolle = Role::aus_text(&rolle_text)
                .ok_or_else(|| Error::Wert(format!("unbekannte Rolle: {rolle_text}")))?;
            mitglieder.insert(
                id,
                Member {
                    id,
                    schluessel: z.get(1)?,
                    name: z.get(2)?,
                    rolle,
                    // Flüchtig: nach dem Laden sitzt niemand in einem Raum.
                    raum: None,
                    raum_seit: None,
                    angelegt_am: z.get(4)?,
                    zuletzt_gesehen: z.get(5)?,
                    stumm: z.get::<_, i64>(6)? != 0,
                    taub: z.get::<_, i64>(7)? != 0,
                },
            );
        }
        state.mitglieder = mitglieder;

        // Nachrichten
        let mut stmt = self.conn.prepare(
            "SELECT id, raum, verfasser, verfasser_name, text, zeit, geloescht, anhaenge FROM nachrichten ORDER BY zeit, id",
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
                anhaenge: anhaenge_lesen(&z.get::<_, String>(7)?),
            });
        }
        state.nachrichten = nachrichten;

        // Protokoll
        let mut stmt = self.conn.prepare(
            "SELECT laufnummer, zeit, handelnder, handelnder_name, aktion, ziel, vorher, nachher, ergebnis, grund \
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
                grund: z.get(9)?,
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
        tx.execute(
            "INSERT INTO meta (schluessel, wert) VALUES ('serverschluessel', ?1) \
             ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
            params![serde_json::to_string(&state.serverschluessel)
                .map_err(|e| Error::Wert(format!("serverschluessel: {e}")))?],
        )?;

        // Räume: vorhandene Zeilen aktualisieren, fehlende löschen.
        {
            let mut vorhanden = std::collections::HashSet::new();
            for r in state.raeume.values() {
                vorhanden.insert(r.id.text());
                tx.execute(
                    "INSERT INTO raeume (id, eltern, name, reihenfolge, angelegt_am, verschluesselt, \
                     verschluesselt_seit) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                     ON CONFLICT(id) DO UPDATE SET eltern = excluded.eltern, name = excluded.name, \
                     reihenfolge = excluded.reihenfolge, angelegt_am = excluded.angelegt_am, \
                     verschluesselt = excluded.verschluesselt, \
                     verschluesselt_seit = excluded.verschluesselt_seit",
                    params![
                        r.id.text(),
                        Option::<String>::None,
                        r.name,
                        r.reihenfolge,
                        r.angelegt_am,
                        if r.verschluesselt { 1 } else { 0 },
                        r.verschluesselt_seit
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
                        // Anwesenheit wird nicht gespeichert (flüchtig):
                        // sonst steht nach einem Neustart jemand im Raum,
                        // der längst weg ist.
                        Option::<String>::None,
                        Option::<i64>::None,
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
                "INSERT INTO nachrichten (id, raum, verfasser, verfasser_name, text, zeit, geloescht, anhaenge) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT(id) DO UPDATE SET text = excluded.text, geloescht = excluded.geloescht, \
                 anhaenge = excluded.anhaenge",
                params![
                    n.id.text(),
                    n.raum.text(),
                    n.verfasser.text(),
                    n.verfasser_name,
                    n.text,
                    n.zeit,
                    n.geloescht as i64,
                    anhaenge_schreiben(&n.anhaenge)
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
                "INSERT OR IGNORE INTO protokoll (laufnummer, zeit, handelnder, handelnder_name, aktion, ziel, vorher, nachher, ergebnis, grund) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    e.laufnummer,
                    e.zeit,
                    e.handelnder.map(|h| h.text()),
                    e.handelnder_name,
                    e.aktion,
                    e.ziel,
                    e.vorher,
                    e.nachher,
                    e.ergebnis,
                    e.grund
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
        // Nur für den Eigentümer, wie die Datenbank: Mitglieder, Kennungen,
        // Prüfprotokoll gehen niemanden sonst auf dem Rechner etwas an.
        nur_eigentuemer(&zwischen)?;
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
        nur_eigentuemer(&zwischen)?;
        std::fs::rename(&zwischen, ziel)?;
        Ok(())
    }
}

/// Legt die Datenbankdatei mit 0600 an bzw. bringt eine vorhandene darauf.
///
/// Vorher war sie 0644 — für jeden Benutzer des Rechners lesbar. `-wal` und
/// `-shm` legt SQLite mit denselben Rechten an wie die Datei selbst.
fn nur_eigentuemer(pfad: &Path) -> Ergebnis<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let datei = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(pfad)?;
        datei.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        for endung in ["-wal", "-shm"] {
            let neben = PathBuf::from(format!("{}{endung}", pfad.display()));
            if neben.exists() {
                std::fs::set_permissions(&neben, std::fs::Permissions::from_mode(0o600))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = pfad;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_domain::State;

    /// Gültige Kennung (32 Byte, Hex) — Sperren verlangen eine echte Kennung.
    fn kennung(name: &str) -> String {
        let mut roh = [0u8; 32];
        for (i, b) in name.as_bytes().iter().enumerate() {
            roh[i % 32] ^= b;
        }
        // Ohne `hex`-Abhängigkeit: 32 Byte als 64 Hex-Zeichen.
        roh.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn zustand() -> State {
        let mut s = State::neu("Testwerk", Role::Mitglied, 1_000);
        let a = s.anmelden("k-admin", "Ada", 1_000).unwrap();
        let raum = s.raum_anlegen(a, "Vorstand", 1_010).unwrap();
        s.betreten(a, raum, 1_011).unwrap();
        s.text_senden(a, "Protokoll", 1_012).unwrap();
        s.sperren(
            a,
            Some(kennung("k-boese")),
            Some("192.0.2.7".into()),
            "Spam",
            1_013,
        )
        .unwrap();
        // Eigene Zeit: bei gleicher Zeit entschiede die Kennung (Zufall) über
        // die Reihenfolge.
        s.beitrag_senden(a, "Fotos", vec!["0a".repeat(16), "0b".repeat(16)], 1_014)
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

    /// Anwesenheit ist flüchtig: sie steht nicht in der Datenbank, und ein
    /// alter Eintrag mit Raum weckt niemanden auf.
    ///
    /// **Gefunden 2026-11-19:** nach einem Neustart meldete der Raum „3
    /// anwesend", obwohl niemand verbunden war — wer dann eintrat, bekam
    /// keinen Serverschlüssel („kein Mitglied online").
    #[test]
    fn anwesenheit_ueberlebt_den_neustart_nicht() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-fluechtig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::oeffnen(dir.join("fluechtig.sqlitedb")).unwrap();
        let s = zustand();
        let raum = s
            .mitglieder
            .values()
            .find_map(|m| m.raum)
            .expect("das Mitglied sitzt im Vorstand");
        assert_eq!(s.anwesende(raum).len(), 1);
        store.speichern(&s).unwrap();

        let geladen = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        assert!(
            geladen.mitglieder.values().all(|m| m.raum.is_none()),
            "nach dem Laden sitzt niemand mehr in einem Raum"
        );
        assert!(geladen.mitglieder.values().all(|m| m.raum_seit.is_none()));
        assert!(geladen
            .raeume
            .keys()
            .all(|r| geladen.anwesende(*r).is_empty()));

        // Ein Alter Bestand (Datenbank von vor dem Fix) weckt niemanden auf.
        store
            .conn
            .execute(
                "UPDATE mitglieder SET raum = ?1, raum_seit = 1",
                params![raum.text()],
            )
            .unwrap();
        let geladen = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        assert!(geladen.mitglieder.values().all(|m| m.raum.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Serverschlüssel-Kennungen überleben den Neustart, entfernter Klartext
    /// steht danach nicht mehr in der Datei (ADR 0012).
    #[test]
    fn serverschluessel_und_klartext_entfernen() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let datei = dir.join("e2e.sqlitedb");
        let mut s = zustand();
        let ada = *s.mitglieder.keys().next().unwrap();
        s.serverschluessel_anlegen(ada, &"ab".repeat(16), 1_100)
            .unwrap();
        let geheimnis = "Kassenstand-streng-geheim";
        s.text_senden(ada, geheimnis, 1_101).unwrap();
        {
            let mut st = Store::oeffnen(&datei).unwrap();
            st.speichern(&s).unwrap();
            s.klartext_entfernen(|t| t.starts_with("e1."), 1_102);
            st.speichern(&s).unwrap();
            st.verdichten().unwrap();
        }
        let st = Store::oeffnen(&datei).unwrap();
        let geladen = st.laden(|| State::neu("x", Role::Gast, 0)).unwrap();
        assert_eq!(geladen.serverschluessel, s.serverschluessel);
        assert!(geladen.nachrichten.is_empty());
        drop(st);
        // Auch nicht in freien Seiten oder im WAL.
        for endung in ["", "-wal"] {
            let pfad = PathBuf::from(format!("{}{endung}", datei.display()));
            if let Ok(bytes) = std::fs::read(&pfad) {
                assert!(
                    !bytes
                        .windows(geheimnis.len())
                        .any(|w| w == geheimnis.as_bytes()),
                    "Klartext steht noch in {}",
                    pfad.display()
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn datenbank_ist_nur_fuer_den_eigentuemer() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sprechwerk-rechte-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let datei = dir.join("r.sqlitedb");
        // Eine alte Datei mit 0644 wird beim Öffnen zugemacht.
        std::fs::write(&datei, b"").unwrap();
        std::fs::set_permissions(&datei, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut st = Store::oeffnen(&datei).unwrap();
        st.speichern(&zustand()).unwrap();
        let sicherung = Store::sichern(&zustand(), dir.join("sicherungen"), 3, 1).unwrap();
        let kopie = dir.join("kopie.sqlitedb");
        st.datenbank_sichern(&kopie).unwrap();
        for pfad in [&sicherung, &kopie] {
            let rechte = std::fs::metadata(pfad).unwrap().permissions().mode() & 0o777;
            assert_eq!(rechte, 0o600, "{}", pfad.display());
        }
        for endung in ["", "-wal", "-shm"] {
            let pfad = PathBuf::from(format!("{}{endung}", datei.display()));
            if pfad.exists() {
                let rechte = std::fs::metadata(&pfad).unwrap().permissions().mode() & 0o777;
                assert_eq!(rechte, 0o600, "{}", pfad.display());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verschluesselung_ueberlebt_den_neustart() {
        let mut s = zustand();
        let raum = *s.raeume.keys().next().unwrap();
        s.raeume.get_mut(&raum).unwrap().verschluesselt = true;
        let datei = std::env::temp_dir().join("sprechwerk-ver-1.db");
        let _ = std::fs::remove_file(&datei);
        {
            let mut st = Store::oeffnen(&datei).unwrap();
            st.speichern(&s).unwrap();
        }
        let st = Store::oeffnen(&datei).unwrap();
        let geladen = st
            .laden(|| State::neu("Testwerk", Role::Mitglied, 2_000))
            .unwrap();
        assert!(
            geladen.raeume[&raum].verschluesselt,
            "das Flag muss den Neustart überleben"
        );
        let _ = std::fs::remove_file(datei);
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
    /// Eine Sperre behält ihren Grund — und eine **ältere** Datenbank bekommt
    /// die Spalte beim Öffnen nachgerüstet (sonst hätte sie sie nie).
    #[test]
    fn grund_ueberlebt_und_wird_nachgeruestet() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-grund-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let datei = dir.join("test.sqlitedb");

        // Eine Datenbank im alten Stand: protokoll ohne Spalte `grund`.
        {
            let conn = rusqlite::Connection::open(&datei).unwrap();
            conn.execute_batch(
                "CREATE TABLE protokoll (
                   laufnummer INTEGER PRIMARY KEY, zeit INTEGER NOT NULL, handelnder TEXT,
                   handelnder_name TEXT NOT NULL, aktion TEXT NOT NULL, ziel TEXT NOT NULL,
                   vorher TEXT, nachher TEXT, ergebnis TEXT NOT NULL);
                 CREATE TABLE meta (schluessel TEXT PRIMARY KEY, wert TEXT NOT NULL);
                 INSERT INTO meta (schluessel, wert) VALUES ('schema_version', '1');
                 INSERT INTO meta (schluessel, wert) VALUES ('naechste_laufnummer', '2');",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO protokoll VALUES (1, 1000, NULL, 'Ada', 'anmeldung', 'ab12', NULL, NULL, 'erlaubt')",
                [],
            )
            .unwrap();
        }

        // Öffnen rüstet nach, Laden liest die alte Zeile ohne Grund.
        let mut store = Store::oeffnen(&datei).unwrap();
        let geladen = store
            .laden(|| State::neu("Testwerk", Role::Mitglied, 0))
            .unwrap();
        assert_eq!(geladen.protokoll.len(), 1);
        assert_eq!(geladen.protokoll[0].ziel, "ab12");
        assert_eq!(
            geladen.protokoll[0].grund, None,
            "alte Zeilen haben keinen Grund"
        );

        // Und eine neue Zeile mit Grund überlebt Speichern und Laden.
        let mut s = geladen;
        let a = s.anmelden("k-admin", "Ada", 2_000).unwrap();
        let gesperrt = kennung("k-boese");
        s.sperren(a, Some(gesperrt.clone()), None, "Spam im Chat", 2_100)
            .unwrap();
        store.speichern(&s).unwrap();
        let wieder = store.laden(|| State::neu("egal", Role::Gast, 0)).unwrap();
        let zeile = wieder
            .protokoll
            .iter()
            .find(|e| e.aktion == "sperre_verhaengt")
            .expect("Sperrzeile");
        assert_eq!(zeile.grund.as_deref(), Some("Spam im Chat"));
        assert_eq!(
            zeile.ziel, gesperrt,
            "im Ziel muss stehen, wen es trifft — nicht der Grund"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// Migration: eine Datenbank im Stand 1 bekommt die Spalte `grund`, eine
    /// **Kopie** liegt daneben, und ein neueres Format wird abgelehnt.
    #[test]
    fn migration_von_stand_eins() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let datei = dir.join("alt.sqlitedb");

        // Stand 1: protokoll ohne `grund`, kein `grund` in der Struktur.
        {
            let conn = rusqlite::Connection::open(&datei).unwrap();
            conn.execute_batch(
                "CREATE TABLE protokoll (
                   laufnummer INTEGER PRIMARY KEY, zeit INTEGER NOT NULL, handelnder TEXT,
                   handelnder_name TEXT NOT NULL, aktion TEXT NOT NULL, ziel TEXT NOT NULL,
                   vorher TEXT, nachher TEXT, ergebnis TEXT NOT NULL);
                 CREATE TABLE meta (schluessel TEXT PRIMARY KEY, wert TEXT NOT NULL);
                 INSERT INTO meta VALUES ('schema_version', '1');
                 INSERT INTO meta VALUES ('server_name', 'Altwerk');
                 INSERT INTO protokoll VALUES (1, 1000, NULL, 'Ada', 'anmeldung', 'ab12', NULL, NULL, 'erlaubt');",
            )
            .unwrap();
        }

        let store = Store::oeffnen(&datei).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            SCHEMA_VERSION,
            "die Version muss nach der Migration stimmen"
        );
        // Daten sind noch da.
        let geladen = store
            .laden(|| State::neu("egal", Role::Mitglied, 0))
            .unwrap();
        assert_eq!(geladen.server_name, "Altwerk");
        assert_eq!(geladen.protokoll.len(), 1);

        // Kopie liegt daneben (die Rückfahrt).
        let kopie = datei.with_extension("vor-v1.sqlite");
        assert!(
            kopie.exists(),
            "vor der Migration muss eine Kopie entstehen"
        );

        // Ein **neueres** Format wird abgelehnt, nicht halb geschrieben.
        let neuer = dir.join("neu.sqlitedb");
        {
            let conn = rusqlite::Connection::open(&neuer).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (schluessel TEXT PRIMARY KEY, wert TEXT NOT NULL);
                 INSERT INTO meta VALUES ('schema_version', '99');",
            )
            .unwrap();
        }
        assert!(
            Store::oeffnen(&neuer).is_err(),
            "ein neueres Format darf nicht geöffnet werden"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// v2 → v3: die Spalte `anhaenge` kommt dazu, vorhandene Nachrichten
    /// bleiben und haben keine Anhänge.
    #[test]
    fn migration_von_stand_zwei() {
        let dir = std::env::temp_dir().join(format!("sprechwerk-mig2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let datei = dir.join("alt.sqlitedb");
        {
            let conn = rusqlite::Connection::open(&datei).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (schluessel TEXT PRIMARY KEY, wert TEXT NOT NULL);
                 INSERT INTO meta VALUES ('schema_version', '2');
                 CREATE TABLE nachrichten (
                   id TEXT PRIMARY KEY, raum TEXT NOT NULL, verfasser TEXT NOT NULL,
                   verfasser_name TEXT NOT NULL, text TEXT NOT NULL, zeit INTEGER NOT NULL,
                   geloescht INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
        }
        let store = Store::oeffnen(&datei).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(!spalte_fehlt(&store.conn, "nachrichten", "anhaenge").unwrap());
        assert!(datei.with_extension("vor-v2.sqlite").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn anhaenge_in_der_spalte() {
        let ids = vec!["0a".repeat(16), "0b".repeat(16)];
        assert_eq!(anhaenge_lesen(&anhaenge_schreiben(&ids)), ids);
        assert!(anhaenge_lesen("").is_empty());
    }
}

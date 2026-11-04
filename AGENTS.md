# AGENTS.md — Vorlage für alle Weppelmann-Projekte

> Diese Datei liegt in **jedem** Repo (UI-Bibliothek und Apps). Sie ist die verbindliche
> Arbeitsanweisung für Menschen und Agenten. Abweichungen nur mit Eintrag in
> `docs/adr/` und Vermerk hier. Der projektspezifische Teil ist §8.

## 1. Worum es geht

Sprach- und Texträume für Vereine: selbst gehostet, ein Container, ein UDP-Port
(QUIC). Sprache wird verteilt, nicht gemischt. Der Server ist in Rust, die
Fensteranwendung ist Tauri 2 mit Vue.

## 2. Nicht verhandelbar

1. **Datenorientiert.** Das Datenmodell ist die einzige Wahrheit, die Oberfläche ist
   `render(model) → Sicht`. Kein Zustand in der Darstellung, der nicht im Modell steht.
2. **Zustände als Summentypen.** `enum Zustand { Getrennt, Verbindet, Verbunden{..}, Fehler(Text) }`
   statt Fächer aus Booleans. Ein Rust-`enum` mit `serde` und ein TS-Diskriminator
   beschreiben denselben Vertrag.
3. **Sichtbarkeit folgt aus dem Modell.** „Einblenden" ist keine Anweisung, sondern eine
   Folge des Zustands. Kein `show()`, kein `display:none` als Logik, keine `v-if`-Kette
   für Geschäftslogik.
4. **Keine Logik in der Darstellung.** Rechnen, laden, filtern in `domain` (Rust) oder in
   reinen TS-Funktionen — beide mit Unit-Tests.
5. **SIMPLE** (Wrigstad): immer ein lauffähiges Programm, nach jeder Änderung bauen,
   Tests laufen lassen, Problem rekursiv verkleinern. Cheats sind erlaubt, aber
   ticketpflichtig, lokal begrenzt und werden abgebaut.
6. **Keine Farben, Größen oder Abstände im Code.** Nur semantische Tokens
   (`var(--wp-color-text-default)`). Hex-Werte, `px`-Magie und `--wp-gray-300` in
   Komponenten sind Fehler. Fehlt ein Token, kommt es in `weppelmann-ui/packages/tokens`,
   nicht als Zahl ins Projekt.
7. **Drei Themes, sonst nichts:** `dark`, `light`, `mono`. Keine Runtime-Themes,
   keine Nutzer-Themes, kein Theme-Editor.
8. **Tests sind Teil der Lieferung.** Unit, Integration, Vertrag, Rundlauf.
   Ein Fix ohne Test ist ein Fix auf Probe.
9. **Keine Secrets** in Code, Commits, Logs oder Testdaten. Konfiguration über
   Umgebung und Datei, Beispiel in `.env.example` / `sprechwerk.toml`.
10. **Plattformneutral.** Kein `cfg(target_os)` und keine Pfadannahmen im Frontend.
    Unterschiede kommen als Daten.
11. **Keine eigene Krypto.** Verschlüsselung kommt von QUIC (TLS 1.3) und Ed25519.
    Wer hier etwas selbst baut, hat den Auftrag falsch verstanden.

## 3. Struktur

```
crates/domain     Datenmodell, Regeln, keine I/O          → Unit-Tests
crates/protocol   Sprache auf der Leitung (Rahmen, Felder) → Unit-Tests
crates/adapters   Netz (QUIC), Geräte (Audio), Dateien     → Integrationstests
crates/ipc        Befehle/Ereignisse + Modell für die UI   → Vertragstests
crates/store      Datenbank des Servers, Export, Sicherung → Integrationstests
crates/server     QUIC-Dienst, Konfiguration, Betrieb      → Rundlauf
crates/cli        Referenzclient auf der Kommandozeile     → Rundlauf
crates/desktop    Tauri-Fenster (eigene [workspace])       → Vertragstests
ui/               Vue 3 + TS, dünn; nutzt @weppelmann/components
docs/             architektur.md, datenfluss.md, adr/, QUALITAET.md
```

Richtung der Abhängigkeiten: `domain ← protocol ← adapters ← ipc ← desktop`.
Nie zurück. `ui/` kennt keine Domäne, nur Befehle, Ereignisse und Zustände.

## 4. Befehle

```bash
# alles prüfen (das ist der Befehl vor jedem Commit)
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace

# Fensteranwendung (eigene [workspace], deshalb eigener Aufruf)
(cd crates/desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo check)

npm --prefix ui run typecheck && npm --prefix ui run build
./scripts/latenz.sh        # Latenz messen (p50/p95/p99)
./scripts/rundlauf.sh      # Server + zwei Clienten von Hand
```

`cargo test` **mit** `--workspace`: ohne das prüft Cargo nur das Wurzelpaket und
meldet „0 passed". `crates/desktop` steht in einer eigenen `[workspace]`-Tabelle
(Tauri braucht das) und bekommt seinen eigenen Aufruf.

Das Frontend lässt sich ohne Backend entwickeln: `npm --prefix ui run dev`
(Port 1423). Wer für eine UI-Aufgabe einen echten Server braucht, hat die
Grenze falsch gezogen.

## 5. Definition of Done

- [ ] Tests grün (Unit, Integration, Vertrag, Rundlauf)
- [ ] `clippy -D warnings`, `vue-tsc --noEmit` sauber
- [ ] alle drei Themes geprüft (dark, light, mono)
- [ ] Tastaturbedienung geprüft, Fokus sichtbar, Rollen und Namen gesetzt
- [ ] Screenshot im PR (vorher/nachher bei UI-Änderungen)
- [ ] keine Tokens umgangen, keine Hex-Werte, keine neuen Abhängigkeiten ohne Begründung
- [ ] Dokumentation angepasst: `README.md`, `docs/`, `SPEC.md` (Festlegungen)
- [ ] Latenz nicht schlechter: `scripts/latenz.sh` vor und nach der Änderung

## 6. Commits und Releases

- Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `build:`, `ci:`).
- Kleine Commits, ein Gedanke pro Commit, deutschsprachige Betreffzeile erlaubt.
- **Immer über einen Branch und einen Pull Request.** `main`/`master` ist geschützt:
  kein direkter Push, kein Force-Push. `git switch -c fix/kurzbeschreibung`,
  committen, `git push -u origin HEAD`, PR öffnen, CI abwarten, mergen.
- Kein „WIP"-Merge; der Hauptzweig ist immer lauffähig.

## 7. Dokumentation

- `README.md`: was das Programm tut, für wen, Installation, ein Absatz zu Daten
  und Datenschutz.
- `docs/architektur.md`: Schichten, Grenzen, Datenfluss als Mermaid-Diagramm.
- `docs/datenfluss.md`: was ein Klick auslöst, bis das Bild steht.
- `docs/QUALITAET.md`: Latenz- und Qualitätskriterien mit Messwerten.
- `docs/adr/NNNN-titel.md`: getroffene Entscheidung, Alternativen, Kosten.
- `SPEC.md`: jede Festlegung mit Begründung und Preis.

## 8. Projektspezifisch

- **Name, Zweck, Zielgruppe:** sprechwerk (Arbeitstitel) — Sprach- und Texträume
  für Vereine; Leute, die heute TeamSpeak oder Discord nutzen, und ein Betreiber,
  der selbst hostet.
- **Abweichende Befehle:** Oberfläche ist Tauri 2 + Vue (`ui/`) im Crate
  `crates/desktop` (eigene `[workspace]`, Port 1423).
- **Besonderheiten:**
  * Sprache läuft als QUIC-Datagramm, Steuerung und Text als Strom — **nie**
    Dateien über die Sprachverbindung.
  * Der Sprachkopf ist 4 Byte; der Raum steht **nicht** darin, sondern kommt aus
    dem Sitzungszustand des Servers.
  * Keine Konten: die Kennung ist ein Ed25519-Schlüsselpaar, der Server fordert
    bei jeder Verbindung einen Beweis an. Export/Import der Schlüsseldatei ist
    der Gerätewechsel.
  * Das Zertifikat ist selbst signiert; der Fingerabdruck wird beim ersten
    Verbinden bestätigt (TOFU) — siehe `docs/adr/0002-verschluesselung.md`.
- **Kontakt/Issues:** https://gitea.weppelmann.ddnss.de/patrickweppelmann/sprechwerk

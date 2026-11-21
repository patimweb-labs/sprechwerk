# AGENTS.md — Vorlage für alle Weppelmann-Projekte

> Diese Datei liegt in **jedem** Repo (UI-Bibliothek und Apps). Sie ist die verbindliche
> Arbeitsanweisung für Menschen und Agenten. Abweichungen nur mit Eintrag in
> `docs/adr/` und Vermerk hier. Der projektspezifische Teil ist §9.
>
> **Vermerk (ADR 0013, 2026-11-18):** sprechwerk ist reines Rust — Oberfläche
> mit egui statt Vue/Tauri. Wo diese Vorlage Vue, CSS-Variablen, `vue-tsc`,
> `npm` oder Playwright nennt, gilt hier das Rust-Gegenstück aus §3, §4 und §9.

## 1. Worum es geht

Sprach- und Texträume für Vereine: selbst gehostet, ein Container, ein UDP-Port
(QUIC). Sprache wird verteilt, nicht gemischt. Server, Fenster und
Server-Fenster in reinem Rust, Oberfläche mit egui (ADR 0013).

## 2. Nicht verhandelbar

1. **Datenorientiert.** Das Datenmodell ist die einzige Wahrheit, die Oberfläche ist
   `render(model) → Sicht`. Kein Zustand in der Darstellung, der nicht im Modell steht.
2. **Zustände als Summentypen.** `enum Zustand { Leer, Laedt, Fertig(Daten), Fehler(Text) }`
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
   (`var(--wp-color-text-default)`; hier: `f.text_default`, `thema::SPACE_4`,
   erzeugt von `crates/oberflaeche/build.rs`). Hex-Werte, Pixel-Magie und
   Grundwerte wie `--wp-gray-300` in Ansichten sind Fehler.
7. **Drei Themes, sonst nichts:** `dark`, `light`, `mono`. Keine Runtime-Themes,
   keine Nutzer-Themes, kein Theme-Editor.
8. **Tests sind Teil der Lieferung.** Unit, Integration, Vertrag, Rundlauf.
   Ein Fix ohne Test ist ein Fix auf Probe.
9. **Keine Secrets** in Code, Commits, Logs oder Testdaten. Konfiguration über
   Umgebung, Beispiel in `.env.example`.
10. **Plattformneutral.** Kein `cfg(target_os)` und keine Pfadannahmen im Frontend.
    Unterschiede kommen als Daten (`platform` im Modell → `data-platform` am Root).
11. **Icons entstehen aus Text.** Jedes App-Zeichen liegt als ASCII-Raster
    (`assets/<app>.pm`) im Projekt und wird von `apps/markenwerk` erzeugt — nicht
    als fertiges PNG abgelegt, nicht von Hand nachgezeichnet. Siehe §9.
    **Ausnahme:** sprechwerk hat eine Vorlage des Nutzers übernommen, Quelle sind
    die SVGs in `assets/` ([ADR 0015](docs/adr/0015-logo-und-farben.md)).
    Tastenkürzel über `Mod`, nie fest `Ctrl`. Siehe PLAN.md §10.

## 3. Struktur

```
crates/domain          Datenmodell, Regeln, keine I/O              → Unit-Tests
crates/protocol        Rahmen, Felder, Sprachkopf, Geheimtext      → Unit-Tests
crates/store           SQLite, Export/Import, Sicherung            → Integrationstests
crates/adapters        Netz, Schlüssel, Klang                      → Integrationstests
crates/audio           Geräte, Opus, Verlustausgleich              → Unit-Tests
crates/ipc             Befehle/Antworten + Modell                  → Vertragstests
crates/client          Client-Kern ohne Oberfläche (Fassade)       → Unit-Tests
crates/oberflaeche     Bausatz: Themes aus Tokens, Texte, Schriften
crates/fenster         Fenster (egui), dünn: modell → Bild → Aktion → Oberflächentests
crates/server          Dienst als Bibliothek + `sprechwerk`         → Integration, Rundlauf
crates/server-fenster  Server mit Fenster (egui)                   → Oberflächentests
crates/cli             Referenzclient
packaging/             Paketskripte je System
docs/                  architektur.md, datenfluss.md, adr/
```

Richtung der Abhängigkeiten: `domain ← adapters ← ipc ← client ← fenster`.
Nie zurück. `fenster` kennt keine Domäne, nur Aktionen, Meldungen und Zustände;
`oberflaeche` kennt weder Client noch Server.

## 4. Befehle

```bash
# alles prüfen (das ist der Befehl vor jedem Commit)
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
./scripts/pruefen.sh                   # dasselbe + Bilder + Sprachproben
./scripts/latenz.sh                    # Latenz messen (p50/p95/p99)
./scripts/rundlauf.sh                  # Server + zwei Clienten von Hand
./scripts/sprechprobe.sh               # Sprachpfad ohne Gerät (Netz, Codec)
./scripts/audio-ende-zu-ende.sh        # Sprachpfad mit Gerät (virtuelle Geräte)

cargo run -p sprechwerk-fenster                      # Fenster
SPRECHWERK_PROFIL=/tmp/b cargo run -p sprechwerk-fenster   # zweite Kennung
cargo run -p sprechwerk-server-fenster               # Server mit Fenster
cargo run -p sprechwerk-server                       # Server (Konsole)
SPRECHWERK_BILDER=docs/bilder cargo test -p sprechwerk-fenster \
  -p sprechwerk-server-fenster --test oberflaeche    # Bildschirmfotos erzeugen
packaging/linux/paket.sh                             # Pakete wie im Release
```

`cargo test` **mit** `--workspace`: ohne das prüft Cargo nur das Wurzelpaket und
meldet „0 passed".

Oberflächenentwicklung läuft **ohne** das echte Backend: die Fenster hängen
über das Trait `Kern` am Client; die Oberflächentests setzen ein Beispielmodell
ein und bedienen das Fenster über AccessKit (`egui_kittest`). Wer für eine
UI-Aufgabe einen echten Server oder eine Soundkarte braucht, hat die Grenze
falsch gezogen.

## 5. Definition of Done

- [ ] Tests grün (Unit, Integration, Vertrag, Rundlauf)
- [ ] `clippy -D warnings` sauber (hier gibt es kein `vue-tsc`, ADR 0013)
- [ ] alle drei Themes geprüft (dark, light, mono)
- [ ] Tastaturbedienung geprüft, Fokus sichtbar, Rollen und Namen gesetzt
- [ ] Screenshot im PR (vorher/nachher bei UI-Änderungen)
- [ ] keine Tokens umgangen, keine Hex-Werte, keine neuen Abhängigkeiten ohne Begründung
- [ ] Dokumentation angepasst: `README.md`, `docs/`, `SPEC.md`
- [ ] Latenz nicht schlechter: `scripts/latenz.sh` vor und nach der Änderung
- [ ] Cheats dieses PRs als Ticket vermerkt

## 6. Commits und Releases

- Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `build:`, `ci:`).
- `release-please` mit `bump-minor-pre-major: true` (0.x bleibt unter 1.0, auch bei
  Breaking Changes) und `extra-files` für die `.metainfo.xml`.
- Kleine Commits, ein Gedanke pro Commit, deutschsprachige Betreffzeile erlaubt.
- **Immer über einen Branch und einen Pull Request.** `main` ist geschützt: kein
  direkter Push, kein Force-Push, kein Löschen. Ablauf:
  `git switch -c fix/kurzbeschreibung`, committen, `git push -u origin HEAD`,
  PR öffnen, CI abwarten, mergen. Reviews sind nicht erzwungen
  (`required_approving_review_count: 0`, Solo-Betrieb), der PR ist es.
- Kein „WIP"-Merge nach `main`; `main` ist immer lauffähig.

## 7. Dokumentation

- `README.md`: was die App tut, für wen, Screenshots (dark + light), Installation je
  Plattform, ein Absatz zu Daten und Datenschutz. Das ist die Visitenkarte, nicht das
  Handbuch.
- `docs/architektur.md`: Schichten, Grenzen, Datenfluss als Mermaid-Diagramm.
- `docs/datenfluss.md`: was ein Befehl auslöst, bis das Bild steht.
- `docs/adr/NNNN-titel.md`: getroffene Entscheidung, Alternativen, Kosten.
- `docs/QUALITAET.md`: Latenz- und Qualitätskriterien mit Messwerten.
- `SPEC.md`: jede Festlegung mit Begründung und Preis.
- Screenshots werden **erzeugt** (hier: `egui_kittest`, siehe §4), nicht von
  Hand abgelegt.

## 8. Marke und Icons

Der Stil steht in `docs/marke/icons.md`; hier nur die Regeln, an die sich Code hält.

1. **Quelle ist das Raster.** `assets/<name>.pm`: Kopf (`name`, `surface`, `ink`,
   optional `shade`), dann `---`, dann das Raster. `.` Fläche, `#` Zeichnung,
   `+` Schatten. Quadratisch, höchstens 64 Zeilen, 32 × 32 ist die gute Größe.
2. **Kachelform: Achteck.** An jeder Ecke 22 % der Kante als 45-Grad-Fase. Das
   macht der Generator (`inKachel`), nicht das Zeichen. `form = voll` schaltet es ab.
3. **Zeichnung mit Abstand.** Mindestens ein Pixel zur Kante, auch an der Fase.
4. **Zwei Strichstärken.** 2 px für die Form, 1–2 px für Binnenlinien.
5. **Zwei Farben.** Fläche je Anwendung, Zeichnung immer `#E9E5DC`. Werte aus
   `primitives.tokens.json`.
   **Abweichung für sprechwerk (ADR 0015, 2026-11-21):** Keine Achteck-Kachel und
   kein Raster mehr. Zeichen und Wortmarke sind aus der Vorlage des Nutzers
   übernommen (`docs/marke/entwurf.png`), also abgerundete Kachel, Pixelblasen
   und gezeichnete Buchstabenpfade. Zwei überlappende Sprechblasen brauchen zwei
   Farben, sonst verschmelzen sie bei 16 px zu einem Fleck: Türkis `#2EE6C8`
   (Akzent) und Orange `#F7931E` (Sprechsignal), Kachel `#131A22` → `#0E1319`.
   Die 2-px-Strichsprache und der Mindestabstand gelten unverändert.
6. **Erzeugen, nicht ablegen.** In allen anderen Apps: `node src/cli.ts render
   <app>` (und `logo <app>` für Zeichen plus Wortmarke) in `apps/markenwerk`.
   **Für sprechwerk** sind die SVGs in `assets/` die Quelle, die Icons entstehen mit
   `packaging/icons/erzeugen.sh`; `scripts/marke-bauen.sh` und die Kopplung an
   `apps/markenwerk` gibt es nicht mehr (ADR 0015).
7. **Nachweis bei Änderungen:** frischer Lauf in einem leeren Verzeichnis,
   `sha256sum` gegen die Dateien im Projekt — für sprechwerk: `erzeugen.sh`
   zweimal laufen lassen und die PNGs vergleichen.

## 9. Projektspezifisch

- **Name, Zweck, Zielgruppe:** sprechwerk (Arbeitstitel) — Sprach- und
  Texträume für Vereine; Leute, die heute TeamSpeak oder Discord nutzen, und ein
  Betreiber, der selbst hostet (Docker, ein Volume, ein UDP-Port).
- **Abweichende Befehle:** Oberfläche ist egui (`crates/fenster`,
  `crates/server-fenster`), ein Arbeitsbereich, kein Node (ADR 0013).
  Server: `crates/server` (`sprechwerk`, Verwaltung mit `sprechwerk stand |
  rolle | protokoll`), Referenzclient: `crates/cli`.
- **Besonderheiten:**
  * Sprache läuft als QUIC-Datagramm (6 Byte Kopf: Version, Codec, Folge,
    Sprecher),
    Steuerung und Text als Strom — **nie** Dateien über die Sprachverbindung.
  * Der Raum steht **nicht** im Sprachkopf, sondern im Sitzungszustand des
    Servers. Damit ist Umleiten in fremde Räume unmöglich.
  * Keine Konten: die Kennung ist ein Ed25519-Schlüsselpaar; der Server fordert
    je Verbindung einen Beweis an. Gerätewechsel = Schlüsseldatei mitnehmen.
  * Zertifikat selbst signiert, Fingerabdruck beim ersten Verbinden bestätigen
    (TOFU) — `docs/adr/0002-verschluesselung.md`.
  * Audio braucht **keine** Entwicklungspakete: Opus wird zur Laufzeit geladen,
    Linux-Geräte laufen über PipeWire/PulseAudio; ALSA direkt nur mit
    `--features cpal` (dann `alsa-lib-devel`). Siehe `docs/AUDIO.md`.
- **Kontakt/Issues:** https://gitea.weppelmann.ddnss.de/patrickweppelmann/sprechwerk

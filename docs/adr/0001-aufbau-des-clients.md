# ADR 0001 — Aufbau des Clients: Tauri 2 + Vue, Schichten wie im Hexwerk-Repo

**Status:** entschieden (2026-11-04)
**Betrifft:** `crates/desktop`, `crates/ipc`, `ui/`

## Zusammenhang

Der Client soll denselben Aufbau haben wie die übrigen Projekte (hexwerk,
crosspoint): eine Fensteranwendung mit Web-Oberfläche, ein Rust-Kern mit
strengen Schichtgrenzen, Design-Tokens aus `weppelmann-ui`, drei Themes,
Dokumentation in `docs/` mit ADRs.

## Entscheidung

1. **Fenster:** Tauri 2. Das Frontend ist Vue 3 + TypeScript in `ui/`.
   `crates/desktop` ist ein eigenes Cargo-Workspace (Tauri verlangt das) und
   übersetzt nur zwischen Fenster und Logik.
2. **Schichten:** `domain ← protocol ← adapters ← ipc ← desktop`. `ui/` kennt
   keine Domäne. Die Oberfläche ist `render(modell)`: ein einziger Modellstand
   (`ipc::Zustand`) kommt als Ereignis `zustand` aus dem Rust-Teil.
3. **Darstellung:** nur semantische Tokens (`--wp-*`) aus `weppelmann-ui`.
   Fehlt ein Token (es gibt zum Beispiel **keine** Radius-Tokens), wird nicht
   improvisiert: Kanten bleiben eckig, bis ein Token existiert.
4. **Port 1423** für die Entwicklung (1420 WLED, 1421 Hexwerk, 1422 Crosspoint).
5. **Verbindungsthread:** die QUIC-Verbindung lebt in einem eigenen Thread mit
   eigener Tokio-Laufzeit. Das Fenster blockiert nie, ein Netzfehler kann die
   Oberfläche nicht anhalten.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| GTK4 wie im früheren hexwerk | Plattformabhängig, keine gemeinsamen Tokens/Komponenten, kein Bau für Windows/macOS ohne Zusatzarbeit |
| egui / iced (reines Rust) | Schnell, aber ohne die vorhandene Komponenten- und Tokenbasis; jede Oberfläche wäre neu |
| Electron | Größe, zwei Laufzeiten, kein Rust-Kern in einem Prozess |
| Web-Client im Browser | Später möglich (WebTransport), aber für Sprache in Version 1 nicht nötig |

## Kosten

* Zwei Laufzeiten im Prozess (WebView + Rust) — akzeptiert, weil die Trennung
  die Oberfläche dünn hält.
* WebView-Größe je Plattform (WebKitGTK, WKWebView, WebView2).
* Ein zusätzliches Cargo-Workspace, das eigene Befehle braucht (AGENTS.md §4).

## Nachtrag: der Chat lässt sich einklappen

Der Verlauf nimmt den größten Teil der Breite ein — beim Durchsehen der Räume
und Leute stört er. Deshalb lässt er sich **nach rechts einklappen**: ein Knopf
in der Kopfzeile schiebt ihn weg, rechts bleibt eine schmale Schiene mit
Chat-Zeichen, Name des Raums (senkrecht) und der Zahl ungelesener Nachrichten.
Die Schiene ist der einzige Weg zurück — niemand soll den Chat „verlieren".

Das ist bewusst **keine Einstellung**: man klappt zu, wenn man die Liste sehen
will, und wieder auf, wenn man schreiben will. Der Zustand wird gemerkt (damit
der nächste Start nicht überrascht), steht aber nicht in den Einstellungen.

# ADR 0013 — Alles in Rust: Fenster mit egui, Verwaltung ohne Browser

**Status:** entschieden und umgesetzt (2026-11-18)
**Ersetzt:** ADR 0001 (Tauri 2 + Vue), ADR 0011 (Verwaltung im Browser),
den Paketteil von ADR 0007
**Betrifft:** `crates/client`, `crates/oberflaeche`, `crates/fenster`,
`crates/server` (Bibliothek + `verwaltung.rs`), `crates/server-fenster`,
`packaging/`, `.github/workflows/`

## Kontext

Bis 1.3 bestand der Client aus drei Sprachen: Rust (Logik), TypeScript und
CSS (Vue 3 in `ui/`), verbunden über Tauri 2. Dazu kam eine zweite Vue-Seite
für die Verwaltung, die der Server über HTTP auslieferte (ADR 0011).

Der Preis war hoch für ein Vereinsprogramm:

* **Zwei Werkzeugketten.** Node 22 plus `weppelmann-ui` neben dem Repository
  (in der CI mit eigenem Zugangstoken), npm-Abhängigkeiten, `vue-tsc`, Vitest,
  Playwright — für eine Oberfläche, deren Logik ohnehin in Rust lag.
* **WebKitGTK unter Linux.** Sieben `-devel`-Pakete zum Bauen, eine
  GNOME-Laufzeit im Flatpak, Eigenheiten der Webview je System.
* **Eine zweite Sperrdatei** (`crates/desktop` als eigener Arbeitsbereich):
  `cargo test --workspace` sah die Fenster-Tests nicht.
* **Ein HTTP-Dienst im Server** (axum) samt Zugangscode, Keks und Port 7444 —
  nur damit der Betreiber Rollen setzen kann.

Der Anstoß des Nutzers: „Was würde es kosten, alles in Rust zu machen — es
soll weiterhin minimalistisch gut aussehen. Der Server braucht eine reine
Konsolenvariante für Docker und eine, die eine reine Rust-Anwendung ist." Als
Vorbild dienten ArtCraft und FigCraft (storytold), die neben bzw. statt einer
Tauri-App ein egui-Fenster über denselben Crates bauen.

## Entscheidung

1. **Fenster mit egui** (`eframe` 0.37, Renderer `glow`, AccessKit an).
   Die Oberfläche ist weiter `render(modell)`: `crates/fenster` hält ein
   `Oberflaeche`-Modell (Ansicht und Dialog als Summentypen), zeichnet es in
   jedem Bild neu und gibt Aktionen ab; gerechnet wird in reinen Funktionen
   (`logik.rs`) mit Tests.
2. **Client-Kern ohne Oberfläche** (`crates/client`): Verbindung, Sprechpfad,
   Schlüssel, Einstellungen und die Fassade `Client` mit `Aktion`, `Ergebnis`
   und `Meldung` als Summentypen. Was vorher in Tauri-Befehlen stand (TOFU beim
   Verbinden, Audio starten), liegt hier.
3. **Bausatz** (`crates/oberflaeche`): Themes werden in `build.rs` aus den
   Token-Dateien von `weppelmann-ui` erzeugt (dark, light, mono; Abstände,
   Größen, Schriftgrade) — im Code stehen nur Namen. Texte in vier Sprachen
   (`texte.json`, unverändert übernommen), IBM Plex eingebettet (OFL),
   Bausteine (Karte, Seitenkopf, Pegelbalken …) für beide Fenster.
4. **Server als Bibliothek plus zwei Programme.** `dienst::laufen` startet den
   Dienst; `sprechwerk` (Konsole, Container) und `sprechwerk-server-fenster`
   (egui) benutzen ihn.
5. **Verwaltung ohne Browser.**
   * Konsole: `sprechwerk stand | rolle <wer> <rolle> | protokoll [ab]`. Ein
     Steuerkanal auf `127.0.0.1` (eine JSON-Zeile hin, eine zurück); Zugang
     über `<daten>/verwaltung.schluessel` (bei jedem Start neu, `0600`). Wer das
     Datenverzeichnis lesen kann, darf verwalten — im Container per
     `docker exec`.
   * Fenster: das Server-Fenster ruft `Verwaltung` direkt auf, ohne Kanal.
6. **Pakete wie FigCraft:** Skripte unter `packaging/`, die lokal und in der
   CI laufen — nfpm (deb, rpm), appimagetool, Flatpak aus dem tar.gz, WiX v5
   (msi), `lipo` + `hdiutil` (dmg, universal).
7. **Oberflächentests mit `egui_kittest`:** Beispielmodell hinein, über
   AccessKit bedienen, Aktionen prüfen; dieselben Tests erzeugen die Bilder in
   `docs/bilder/` (`SPRECHWERK_BILDER`). Ein Widget ohne zugänglichen Namen und
   ein Zeichen ohne Schrift lassen sie scheitern.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Tauri behalten, nur die Verwaltung in Rust | Node, WebKit und zweite Sperrdatei blieben; das Ziel war ein Werkzeug |
| iced (Elm-Architektur) | passt zur Architektur am besten, aber Barrierefreiheit (AccessKit) war in egui fertig, in iced nicht; ArtCraft/FigCraft zeigen egui im Einsatz |
| GPUI (Zed) | schönste Darstellung, aber Doku dünn und API an Zeds Bedarf gebunden |
| Slint | eigene Auszeichnungssprache — wieder kein reines Rust |
| Dioxus/Leptos nativ | intern weiter HTML und CSS |
| Verwaltung über eine Unix-Domain-Socket | eleganter Zugang über Dateirechte, aber unter Windows nicht gleich; TCP auf 127.0.0.1 plus Schlüsseldatei gilt überall |
| Konsole schreibt direkt in die Datenbank | der laufende Server hält den Zustand im Speicher und überschreibt die Änderung |

## Folgen und Preis

* **Ein Arbeitsbereich, eine Sprache.** `cargo test --workspace` prüft alles
  (360 Tests beim Umstieg), CI ohne Node, ohne Token für `weppelmann-ui`,
  ohne WebKit-Pakete. Zum Bauen genügen Rust und ein C-Compiler.
* **Kein Port 7444 nach außen**, kein HTTP im Server (axum fällt weg).
* **Der gemeinsame Design-Bausatz ist geteilt:** die Vue-Komponenten von
  `weppelmann-ui` gelten hier nicht mehr; geteilt werden nur noch die
  Token-Dateien (Kopie in `crates/oberflaeche/tokens/`). Ändern sich dort
  Werte, muss die Kopie nachgezogen werden.
* **Texteingabe ist egui, nicht der Browser.** Markieren und Kopieren im
  Verlauf gibt es über das Kontextmenü („Text kopieren"), nicht durch Ziehen
  über mehrere Beiträge. IME und Farb-Emoji sind schwächer als in einer
  Webview (Emoji einfarbig, NotoEmoji).
* **Effekte entfallen** (Konfetti, Herzen, Beben): in egui Handarbeit, und
  der Nutzer braucht sie nicht. Alte Nachrichten mit Effekt bleiben lesbar.
* **Icons** sind dieselbe Lucide-Auswahl wie `@weppelmann/icons`, als SVG
  eingebettet (`scripts/icons-holen.py`) und in der Theme-Farbe gezeichnet.
* **Alte Konfiguration bleibt lesbar:** `verwaltung.bind/code/browser` werden
  angenommen und beim Start als wirkungslos gemeldet.
* **Die Kennung bleibt:** das Fenster liest Schlüssel und Einstellungen am
  selben Ort wie die Tauri-App (`<Konfiguration>/de.weppelmann.Sprechwerk`).
* **Datei-Anhänge** (Branch `feat/chat-anhaenge`, nicht gemergt) waren für Vue
  geschrieben und müssen für das egui-Fenster neu gebaut werden.
* **Ungeprüft auf echten Geräten:** Windows- und macOS-Pakete entstehen in der
  CI; ob sie auf echten Rechnern laufen, steht aus.

# ADR 0007 — Pakete je System und Betrieb

**Status:** entworfen (2026-11-04)
**Betrifft:** `crates/desktop`, `flatpak/`, `packaging/`, OCI-Image

## Fenster

| System | Format | Anmerkung |
|---|---|---|
| Linux | `.deb`, `.rpm`, AppImage | Tauri-Bundler; AppImage für „einfach herunterladen" |
| Linux (immutabel) | Flatpak | braucht Mikrofon über das Portal (`--socket=pulseaudio` oder PipeWire-Portal) |
| Windows | MSI/NSIS | WebView2 ist ab Windows 10 vorhanden, sonst mitinstallieren |
| macOS | DMG | Signierung/Notarisierung kostet Geld; ohne sie warnt Gatekeeper |

**Automatische Updates: aus.** Ein Verein kontrolliert seine Versionen; stilles
Nachladen von Code widerspricht dem Selbsthost-Gedanken. Stattdessen: die
Anwendung meldet „Version X ist verfügbar" (Abfrage der Instanz), der Betreiber
entscheidet.

## Server

* **OCI-Image, multi-arch** (`linux/amd64`, `linux/arm64`) — Raspberry-tauglich.
* Nicht als root, `/data` als Volume, `HEALTHCHECK` über eine Sprachverbindung
  (nicht über einen TCP-Port — den gibt es nicht).
* Nur `7443/udp` nach außen; keine eingehenden TCP-Ports.
* **Betriebswerkzeuge im Image**: `export`, `import`, `sicherung` — ein
  Rettungsweg, der auch dann funktioniert, wenn der Dienst nicht startet.
* **Kennzahlen** (Prometheus-Format, optional): verbundene Clients,
  Sprachrahmen/s, verworfene Rahmen, Sicherungsalter.
* **Protokolle** nach stdout (Container-Regel), strukturiert (`tracing`).

## Betriebsabläufe

| Vorgang | Ablauf |
|---|---|
| Sicherung | nach Plan (JSON im Volume, letzte N), zusätzlich `datenbank_sichern` (VACUUM INTO) |
| Wiederherstellung | Container stoppen, `import <datei>`, starten — vorher sichert der Import den alten Stand |
| Update | Image tauschen, Container starten: Migration beim Start, vorher automatische Sicherung |
| Rückweg | Sicherung einspielen; kein Rückwärts-Migrationsweg (SPEC §2) |
| Umzug auf andere Maschine | Volume kopieren **oder** Export/Import — beides getestet |
| Aufräumen | `aufbewahrung_tage` für Text, Sicherungsrotation, keine verwaisten Räume |

## Kosten

* Vier Paketformate = vier Baustellen; Flatpak und macOS sind die teuersten.
* Multi-Arch baut doppelt (oder über Emulation, langsam).
* Ohne automatische Updates muss der Betreiber selbst dran denken — dafür
  überrascht ihn nichts.

## Anwendungssymbol

Das Symbol wird **nicht gezeichnet, sondern erzeugt**: `markenwerk` in
`weppelmann-ui` liest ein ASCII-Raster (`marken/sprechwerk.pm`) und schreibt
SVG, PNG in allen Desktop-Größen, ein Tray-Bild und eine monochrome Fassung
(`symbolic.svg`). Dieselbe Kachelform (Achteck) und derselbe Strich wie
Hexwerk und WLED Ambilight — die Apps gehören sichtbar zusammen.

Damit das Symbol auch in der **Taskleiste** erscheint, reicht die Datei in der
Anwendung nicht: unter Linux muss eine `.desktop`-Datei die Fensterklasse
nennen. Deshalb liegen beide im Baum:

```
crates/desktop/icons/                     # was Tauri einbettet (.png/.ico/.icns)
crates/desktop/packaging/
  de.weppelmann.Sprechwerk.desktop        # StartupWMClass=sprechwerk-desktop
  icons/hicolor/<größe>/apps/sprechwerk.png
```

Installation im Benutzerbereich (ohne root):

```bash
cp -r crates/desktop/packaging/icons/hicolor ~/.local/share/icons/
cp crates/desktop/packaging/de.weppelmann.Sprechwerk.desktop ~/.local/share/applications/
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
```

Im Fenster selbst steht dieselbe Zeichnung links neben dem Namen — als Maske
(`mask-image` auf `symbolic.svg`), damit sie die Textfarbe annimmt und nicht als
zweite Bilddatei gepflegt werden muss.

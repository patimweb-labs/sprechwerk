# ADR 0007 — Pakete je System und Betrieb

**Status:** entworfen (2026-11-04), Pakete und Releases umgesetzt (2026-11-16)
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
* Nur `7443/udp` nach außen; keine eingehenden TCP-Ports. (Die Verwaltung im
  Browser, ADR 0011, lauscht auf `7444/tcp` — im Container nur an
  `127.0.0.1` des Hosts weitergereicht, also nicht „nach außen".)
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

## Umgesetzt (2026-11-16): Releases

`.github/workflows/release.yml` mit release-please (AGENTS.md §6). Ein Merge
des Release-PRs erzeugt Tag, Änderungsliste und hängt an das GitHub-Release:

| Teil | Form | Gebaut auf |
|---|---|---|
| Fenster Linux | `.deb`, `.rpm`, AppImage (x86_64) | `ubuntu-24.04` |
| Fenster Flatpak | `.flatpak` (x86_64), GNOME-Laufzeit 51 | aus dem `.deb` umgepackt |
| Fenster Windows | NSIS-`.exe` (ohne Adminrechte), `.msi` | `windows-2025` |
| Fenster macOS | `.dmg`, universal (arm64 + x86_64), ab macOS 11 | `macos-15` |
| Server + CLI | Linux `.tar.gz` je x86_64 und aarch64, Windows `.zip`, macOS `.tar.gz` (universal) — mit eingebauter Verwaltung (ADR 0011) | `server.yml`, eigener Läufer je System |
| Container | `ghcr.io/patimweb-labs/sprechwerk`, amd64 + arm64 | eigener Läufer je Architektur, kein QEMU; Verwaltung aus `server.yml` |
| Prüfsummen | `SHA256SUMS` | — |

Die Fenster-Pakete baut `.github/workflows/pakete.yml`; denselben Baustein ruft
`pruefen.yml` bei **jedem PR** auf. Was im Release landet, ist also vorher schon
einmal gebaut worden — mit Clippy auf Windows und macOS, wo andere Teile
übersetzt werden (cpal statt PipeWire).

Entscheidungen dabei:

* **Flatpak umgepackt statt aus den Quellen gebaut.** Die GNOME-Laufzeit 51
  bringt WebKitGTK 4.1, `libopus.so.0` und `pactl`/`parec`/`pacat`/`paplay`
  mit; im Flatpak läuft damit dieselbe Binärdatei wie im `.deb`. Geprüft
  2026-11-16: alle 133 Bibliotheken werden gefunden, das Fenster startet,
  `pactl` erreicht den Tonserver über `--socket=pulseaudio`. **Preis:** so
  nicht Flathub-tauglich (dort wird ohne Netz aus den Quellen gebaut); das ist
  ein eigener Schritt.
* **Opus selbst gebaut** (1.5.2, Prüfsumme von xiph.org) und ins Paket gelegt:
  Windows `opus.dll` mit statischer C-Laufzeit neben der `.exe`, macOS
  `libopus.0.dylib` in `Contents/Frameworks`. Homebrew-Bibliotheken liefen nur
  ab der macOS-Version des Bau-Rechners, vcpkg-DLLs brauchten das „Visual C++
  Redistributable". `opus.rs` sucht deshalb zuerst neben dem Programm.
* **Unsigniert.** Kein Apple-Entwicklerkonto (99 €/Jahr), kein
  Windows-Zertifikat. macOS wird ad hoc signiert (Apple Silicon startet sonst
  gar nichts), Gatekeeper und SmartScreen warnen beim ersten Start; die README
  sagt, was dann zu tun ist. **Preis:** jeder Nutzer sieht eine Warnung.
* **macOS-Mikrofon:** `crates/desktop/Info.plist` setzt
  `NSMicrophoneUsageDescription` — ohne den Eintrag beendet macOS die App beim
  ersten Mikrofonzugriff.

`.deb`/`.rpm` nennen Opus und `parec`/`pacat` als Abhängigkeit
(`tauri.linux.conf.json`); das AppImage kann das nicht und setzt beides voraus.

**Noch nie auf echter Hardware gelaufen:** Windows und macOS. Gebaut wird in
der CI, ausprobiert hat es noch niemand.

`scripts/versionen.py` (in `pruefen.sh` und der CI) prüft vorher, dass
release-please jede Version anheben kann, ohne dass `cargo build --locked`
danach scheitert.

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

# ADR 0007 — Pakete je System und Betrieb

**Status:** entworfen (2026-11-04), Pakete und Releases umgesetzt (2026-11-16); **Paketteil überholt durch [ADR 0013](0013-alles-in-rust.md)** (2026-11-18): Skripte unter `packaging/` statt Tauri-Bundler, Container und Betrieb gelten weiter.
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

Client und Server sind zwei Anwendungen; jede bekommt eigene Dateien.

| Anwendung | Teil | Form | Gebaut auf |
|---|---|---|---|
| Client | Linux | `.deb`, `.rpm`, `.tar.gz`, AppImage (x86_64) | `ubuntu-24.04` |
| Client | Flatpak | `.flatpak` (x86_64), `de.weppelmann.Sprechwerk` | aus dem Client-Tar |
| Client | Windows | `.msi` (ohne Adminrechte), `.zip` | `windows-2025` |
| Client | macOS | `.dmg` (universal, ab macOS 11), `.zip` für die Konsole | `macos-15` |
| Server | Linux | `.deb`, `.rpm`, `.tar.gz`, AppImage (x86_64 und aarch64) | `ubuntu-24.04` |
| Server | Flatpak | `.flatpak` (x86_64), `de.weppelmann.Sprechwerk.Server` | aus dem Server-Tar |
| Server | Windows | `.zip` | `windows-2025` |
| Server | macOS | `.dmg` (universal), `.tar.gz` für die Konsole | `macos-15` |
| Server | Container | `docker.io/smotherer/sprechwerk`, amd64 + arm64 | eigener Läufer je Architektur, kein QEMU |
| beide | Prüfsummen | `SHA256SUMS` | — |

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
nennen. Deshalb liegen Zeichnung und Eintrag im Baum und werden von den
Paketen mitinstalliert:

```
packaging/icons/                          # erzeugt von markenwerk (AGENTS.md §8)
packaging/linux/de.weppelmann.Sprechwerk.desktop        # StartupWMClass=de.weppelmann.Sprechwerk
packaging/linux/de.weppelmann.Sprechwerk.Server.desktop # dieselbe für den Server
```

Eingerichtet wird das vom Paket — `.deb`, `.rpm`, AppImage und Flatpak bringen
Eintrag und Symbole mit. **Nicht** von Hand nach `~/.local/share/applications`
kopieren: ein Eintrag dort hat denselben Dateinamen wie der aus dem Paket,
liegt aber früher im Suchpfad und verdeckt ihn. Genau das ist passiert, als
das Programm noch `sprechwerk-desktop` hieß — der alte Eintrag blieb liegen,
zeigte auf ein Programm, das es nicht mehr gab, und das Menü startete nichts.
Wer so etwas vorfindet:

```bash
ls ~/.local/share/applications/*[Ss]prechwerk*
rm ~/.local/share/applications/de.weppelmann.Sprechwerk.desktop
kbuildsycoca6 --noincremental      # Plasma; sonst: neu anmelden
```

Im Fenster selbst steht dieselbe Zeichnung links neben dem Namen — als Maske
(`mask-image` auf `symbolic.svg`), damit sie die Textfarbe annimmt und nicht als
zweite Bilddatei gepflegt werden muss.

## Nachtrag (2026-11-19): Container bei Docker Hub

Das Abbild liegt bei **Docker Hub** statt bei GitHub (`ghcr.io`). Grund ist
kein Geschmack: das Repository wurde neu aufgesetzt, und das alte GHCR-Paket
hing weiter am gelöschten Repository — jeder Push scheiterte mit
`denied: permission_denied: read_package`. Ein GHCR-Paket lässt sich nur von
der Organisation umhängen; Docker Hub ist davon unabhängig und für einen
Betreiber außerdem der Ort, an dem er sucht.

Angemeldet wird sich mit zwei Repo-Secrets, `DOCKERHUB_USERNAME` und
`DOCKERHUB_TOKEN` (Access Token mit *Read & Write*, kein Kontopasswort).
`packages: write` braucht der Lauf nicht mehr.

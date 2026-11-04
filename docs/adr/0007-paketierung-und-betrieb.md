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

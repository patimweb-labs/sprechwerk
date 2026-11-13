# sprechwerk

Sprach- und Texträume für Vereine — selbst gehostet, ein Container, ein
Port. Der Name ist ein Arbeitstitel und änderbar.

Entstanden aus der Frage, warum TeamSpeak in zwei Jahren unordentlich wird und
sich nicht beherrschbar betreiben lässt. Die Antwort ist nicht „mehr
Funktionen", sondern **Struktur, die nicht verrottet** und **Betrieb, der keine
Überraschungen hat**. Jede Festlegung steht in [`SPEC.md`](SPEC.md) — mit
Begründung und, wo nötig, mit dem Preis.

Nachschlagewerk für Protokollfragen: das Handbuch zu TeamSpeak 3 in
[`../fernwerk/docs/handbuch/`](../fernwerk/docs/handbuch/) (zehn Kapitel,
Protokollebene).

## Was es kann (Stand)

* **QUIC** auf einem UDP-Port, TLS 1.3, Ströme für Text und Steuerung,
  Datagramme für Sprache — keine eigene Krypto, keine handgebauten Fragmente.
* **Anmeldung ohne Konten**: die Kennung ist ein Ed25519-Schlüsselpaar, der
  Server fordert bei jeder Verbindung einen Beweis an. Mitnehmbar per Export.
* **Vier feste Rollen** (Administrator, Moderator, Mitglied, Gast) mit einer
  Rechttabelle im Programm — keine 489 Rechte, keine Vererbung.
* **Raumbaum** mit Unterräumen, Text und Sprache im selben Raum.
* **Dauerhafter Text** mit Verlauf, Löschen (Verfasser weich, Moderator hart).
* **Prüfprotokoll**: wer, wann, was, vorher/nachher — auch abgelehnte
  Versuche.
* **Ende-zu-Ende-Verschlüsselung je Raum** (ADR 0006): X25519-Umschläge,
  ChaCha20-Poly1305, Gruppenschlüssel mit Epoche. Der Server verteilt nur
  Geheimtext; Text bleibt lesbar, weil er gespeichert wird.
* **Sprache, hörbar und gemessen**: Aufnahme (PipeWire/PulseAudio), Hochpass,
  Rauschunterdrückung, Noisegate, Opus 32 kbit/s, 60-ms-Vorlauf statt
  PTT-Wartezeit, Jitter-Puffer je Sprecher, Mischen im Client. Der Server
  verteilt nur und prüft das Rederecht — er mischt nichts. Laufzeit durch den
  Server: **p95 1,7 ms** (`scripts/sprechprobe.sh`).
* **Echounterdrückung** (AEC3 als reines Rust): über Lautsprecher sprechen,
  ohne dass die anderen sich selbst hören — 39 dB Echo-Dämpfung bei 0,7 %
  Rechenlast in Release.
* **Fenster**: Tauri 2 mit Vue, drei Farbschemata, vier Sprachen, Pegelanzeige,
  Push-to-Talk, Gerätewahl, Klickdummy für die Oberfläche ohne Server.
* **Betrieb**: Konfiguration über Datei und Umgebung, Sicherung nach Plan im
  Textformat, Aufbewahrungsfrist, ein Container.

Noch nicht umgesetzt (bewusst später): Dateien, Status, Video,
Bildschirmfreigabe, Emojis, Suche, Sperrliste im Fenster.

## Bauen

```bash
# Linux: für cpal (ALSA) — ohne diese Pakete nimmt der Pfad den
# PipeWire-Rückfallweg (parec/pacat), der ebenfalls funktioniert.
sudo dnf install alsa-lib-devel pkgconf-pkg-config

# Windows/macOS: nichts zu installieren, cpal ist fest eingebunden.
cargo build --release
```

Opus wird **zur Laufzeit** geladen (`libopus.so.0` / `libopus.dylib` /
`opus.dll`) und darf neben der Anwendung liegen.

## Ton prüfen (ohne Mikrofon, ohne zweite Person)

```bash
scripts/sprechprobe.sh          # Netz und Codec: Ton durch den Server
scripts/lastprobe.sh            # vier Sprecher gleichzeitig, drei Hörer
scripts/verlustprobe.sh         # 5 % Verlust und Jitter auf einer Störstrecke
scripts/audio-ende-zu-ende.sh   # der echte Pfad: Geräte, RNNoise, Gate, Opus
```

`sprechprobe.sh` startet einen eigenen Server, schickt einen 440-Hz-Ton durch
den Sprachpfad, schneidet ihn mit und prüft Frequenz, Raumtreue, Rückkanal und
Laufzeit.

`audio-ende-zu-ende.sh` legt vier virtuelle PipeWire-Geräte an und verbindet
zwei **echte** Endpunkte über den echten Server. Damit läuft die ganze Kette
wie im Fenster — Aufnahme, Rauschunterdrückung, Noisegate, Opus, Vorlauf,
Server, Jitter-Puffer, Wiedergabe —, und der Mitschnitt wird ausgewertet:
kommt der Ton in der richtigen Frequenz an, und kommt auch ein stimmähnliches
Signal durch die Entrauschung? Alles auf einem Rechner.

## Prüfen

```bash
scripts/pruefen.sh              # alles: Form, Werkzeuge, Tests, Ansichten, Sprachproben
scripts/pruefen.sh --nur-tests  # schnell: nur kompilieren und testen
```

Dieselben Schritte laufen bei jedem Push auf GitHub (`.github/workflows/pruefen.yml`)
— inklusive der Tests von `crates/desktop`, die `cargo test` im Wurzelverzeichnis
sonst nicht einsammelt. Für die Oberfläche wird die Komponentenbibliothek
`weppelmann-ui` neben das Repository geklont; ist sie nicht öffentlich, braucht
der Job das Secret `WEPPELMANN_UI_TOKEN` (nur Leserechte).

**Vor dem Push prüfen** (einmal einschalten):

```bash
git config core.hooksPath .githooks
```

Der Haken `.githooks/pre-push` läuft dann vor jedem Push durch Form, Werkzeuge
und Tests von Arbeitsbereich und Fenster und hält bei einem Befund an. Bewusst
umgehen: `git push --no-verify` oder `SKIP_PREPUSH=1 git push`.

Branch-Schutz („master nur mit grüner Prüfung") ist auf GitHub für **private**
Repositories im kostenlosen Tarif nicht verfügbar — deshalb dieser Haken. Sobald
das Repository öffentlich ist oder ein Pro-Tarif vorliegt, lässt sich derselbe
Schutz serverseitig erzwingen (die vier Aufgabennamen sind die Prüfungen).

Die Laufumgebung ist auf `ubuntu-24.04` festgelegt, nicht `ubuntu-latest`: GitHub
verschiebt `latest` im Oktober 2026 auf Ubuntu 26, und damit ändern sich
Paketnamen unter der Hand.

## Was das Fenster zeigt (und TeamSpeak nicht)

* **Prüfprotokoll** — wer hat was getan, und was wurde **abgelehnt**. Für
  Administrator und Moderator, weil sie das Recht dazu haben.
* **Sperrliste** — wer gesperrt ist, warum, von wem, mit einem Knopf zum
  Aufheben.
* **Qualität in der Statuszeile** — „keine Rahmen verloren" (oder die Zahl) und
  die Umlaufzeit zum Server, statt eines Symbols ohne Zahl.
* **Schlüsselstand je Raum** — ist der Ende-zu-Ende-Schlüssel da? Wenn nicht,
  steht dort „du hörst nichts" mit einem Knopf zum Nachfordern.
* **Eigene Kennung** — der öffentliche Schlüssel in Vierergruppen, mit dem
  Hinweis, was man weitergeben darf und was nicht.
* **Zuletzt verbunden je Server** — nicht nur beim letzten Eintrag.

## Schnellstart

```bash
cargo build --release

# Server (legt beim ersten Start Datenbank und Zertifikat an)
SPRECHWERK_DATEN=/tmp/sprechwerk ./target/release/sprechwerk

# Client in einem zweiten Terminal
./target/release/sprechwerk-cli --server 127.0.0.1:7443 --name Ada --raum 1

# Fenster (braucht die Oberfläche: ui/) — baut eigenständig
cd ui && npm install && npm run build && cd ..
cargo build --manifest-path crates/desktop/Cargo.toml --features custom-protocol
./crates/desktop/target/debug/sprechwerk-desktop
```

Der Server zeigt beim Start seinen Zertifikat-Fingerabdruck. Der **erste**
Client, der sich verbindet, wird Administrator.

### Mit Docker

```bash
docker compose up -d
docker compose logs -f       # Fingerabdruck und Betriebsmeldungen
```

Ein Volume (`/data`), ein Port (`7443/udp`), keine weiteren Dienste.

## Konfiguration

Datei `sprechwerk.toml` (Pfad über `SPRECHWERK_KONFIG`), jeder Wert auch als
Umgebungsvariable. Beispiel ausgeben:

```bash
./target/release/sprechwerk beispiel > sprechwerk.toml
```

| Datei | Umgebung | Bedeutung |
|---|---|---|
| `server.name` | `SPRECHWERK_NAME` | Anzeigename |
| `server.port` | `SPRECHWERK_PORT` | UDP-Port (Vorgabe 7443) |
| `server.bind` | `SPRECHWERK_BIND` | Adresse (Vorgabe `0.0.0.0`) |
| `server.passwort` | `SPRECHWERK_PASSWORT` | leer = offen |
| `server.standard_rolle` | `SPRECHWERK_STANDARD_ROLLE` | Rolle neuer Mitglieder |
| `server.daten` | `SPRECHWERK_DATEN` | Datenverzeichnis (Vorgabe `/data`) |
| `server.aufbewahrung_tage` | `SPRECHWERK_AUFBEWAHRUNG_TAGE` | 0 = unbegrenzt |
| `sicherung.aktiv` | `SPRECHWERK_SICHERUNG_AKTIV` | Sicherung nach Plan |
| `sicherung.intervall_minuten` | `SPRECHWERK_SICHERUNG_INTERVALL_MINUTEN` | Abstand |
| `sicherung.behalten` | `SPRECHWERK_SICHERUNG_BEHALTEN` | Anzahl der Sicherungen |

Die Sicherungen liegen als lesbares JSON unter `<daten>/sicherungen`. Sie
tragen `format_version` und sind mit `jq` prüfbar; eingespielt wird über den
Store (`Store::importieren`).

## Aufbau

```
crates/domain    Zustand: Räume, Rollen, Mitglieder, Nachrichten, Protokoll
crates/protocol   Sprache auf der Leitung: Rahmen, Nachrichten, Sprachkopf
crates/store   SQLite, Export/Import, Sicherungen
crates/adapters  Verbindung, Anmeldung, Beweis (Ed25519)
crates/server  QUIC-Dienst, Konfiguration, Sicherung, Verteilung
crates/cli     Referenzclient auf der Kommandozeile
```

`core` kennt kein Netz — deshalb sind Rechte, Räume und Verläufe ohne Sockets
prüfbar.

## Prüfen

```bash
cargo test --workspace     # 148 Tests: Zustand, Rahmen, Vertrag, Datenbank, Rundlauf
npm --prefix ui run typecheck
scripts/rundlauf.sh        # Server + zwei Clients von Hand
scripts/latenz.sh          # Latenz messen (p50/p95/p99)
scripts/audio-ende-zu-ende.sh   # Sprache über den echten Pfad, ein Rechner
```

Der Rundlauf startet das **echte Programm** und verbindet zwei echte Clients
über QUIC: Anmeldung, Rollen, Raum betreten, Text, Protokoll, Sprache nur im
eigenen Raum, Serverpasswort, Kennung über Verbindungen hinweg.

## Protokoll in einem Blick

```text
Client                                  Server
  ── QUIC (TLS 1.3, ALPN sprechwerk/1) ──►
  ◄── Anforderung { nonce }               Beweisaufforderung
  ── Hallo { kennung, beweis, name } ──►  Ed25519-Signatur über den Nonce
  ◄── Willkommen { Räume, Mitglieder }    oder Fehler
  ◄──► Rahmen (4 Byte Länge + JSON)       Text, Befehle, Antworten
  ◄──► Datagramme (Kopf 6 Byte + Opus)    Sprache, unzuverlässig
```

Der Server öffnet den Steuerstrom, weil ein QUIC-Strom der Gegenstelle erst
mit den ersten Bytes sichtbar wird.

## Lizenz

MIT — siehe [`LICENSE`](LICENSE).

# sprechwerk

**Sprach- und Texträume für Vereine.** Selbst gehostet, ein Container, ein
UDP-Port. Für Leute, die heute TeamSpeak oder Discord benutzen — und für einen
Betreiber, der die Daten selbst in der Hand haben will.

![Raumansicht, dunkel: links die Räume mit den Anwesenden, rechts der Chat mit Vorschaukarte](docs/bilder/raeume-dark.png)

> Arbeitstitel. Der Name ist austauschbar; Verzeichnis und Crates heißen so,
> bis ein besserer gefunden ist.

**Inhalt** ·
[Was es ist](#was-es-ist) ·
[Was es kann](#was-es-kann) ·
[Für wen](#für-wen) ·
[So sieht es aus](#so-sieht-es-aus) ·
[Im Vergleich](#im-vergleich) ·
[Stand heute](#stand-heute) ·
[Installation](#installation) ·
[Das erste Mal verbinden](#das-erste-mal-verbinden) ·
[Verwaltung im Browser](#verwaltung-im-browser) ·
[Sicherheit und Daten](#sicherheit-und-daten) ·
[Betrieb](#betrieb) ·
[Selbst prüfen](#selbst-prüfen) ·
[Mitentwickeln](#mitentwickeln) ·
[Aufbau](#aufbau) ·
[Protokoll in einem Blick](#protokoll-in-einem-blick)

## Was es ist

Ein Ort zum Reden und Schreiben für eine geschlossene Gruppe: Vorstandssitzung,
Trainingsabend, Spieleabend, Elternabend. Man tritt einem Server bei, betritt
einen Raum, spricht — und schreibt daneben Text, der bleibt.

Der Unterschied zu den üblichen Werkzeugen ist nicht der Funktionsumfang,
sondern die Bauweise:

* **Ein Datenmodell, aus dem alles folgt.** Räume, Rollen, Rechte und Anzeige
  kommen aus einem Zustand; die Oberfläche ist `render(modell)`. Was im Modell
  sichtbar ist, ist auch zu sehen — „ausblenden" gibt es nicht als Anweisung.
* **Zustände sind Summentypen**, keine Fächer aus Booleans:
  `enum Zustand { Leer, Laedt, Fertig(Daten), Fehler(Text) }`.
* **Ein Port** statt dreier: Steuerung und Text als Strom, Sprache als
  Datagramm, alles auf einem UDP-Port über QUIC.
* **Keine Konten**, sondern ein Schlüsselpaar: die Kennung ist eine Datei, die
  man mitnimmt.
* **Alles Ende zu Ende** — Text, Sprache, Reaktionen, Vorschauen. Der Betreiber
  sieht keinen Inhalt, nur den Betrieb.
* **Jede Festlegung mit Begründung und Preis** in [`SPEC.md`](SPEC.md) und
  [`docs/adr/`](docs/adr/).

## Was es kann

**Sprache.** Räume betreten, reden, hören. Aufnahme über PipeWire/PulseAudio
(Linux), CoreAudio (macOS), WASAPI (Windows). Hochpass, Entrauschung,
Noisegate und Echounterdrückung (AEC3, reines Rust) laufen vor der Kodierung;
Opus mit 32 kbit/s, 60 ms Vorlauf gegen abgeschnittene erste Wörter — die
Sprechtaste schneidet also nichts ab. Je Sprecher ein eigener Jitter-Puffer,
gemischt wird im Client. Der Server verteilt nur, mischt nichts und prüft das
Rederecht. Wer stumm oder taub schaltet, sieht das an sich selbst und an anderen.

**Text.** Ein Chat je Raum, Verlauf dauerhaft. Löschen kann der Verfasser weich
(„Nachricht gelöscht" bleibt stehen), ein Moderator hart; eine Aufbewahrungsfrist
räumt nach Plan auf. Dazu:

* **Reaktionen** — sechs Zeichen (👍 ❤️ 😂 😮 😢 🎉), zusammengefasst angezeigt;
  wer dabei ist, sieht man am Zeichen.
* **Effekte** — Konfetti, Herzen, Beben; spielen einmal beim Empfang und lassen
  sich noch einmal abspielen.
* **Links** — anklickbar und mit **Vorschau**: Titel, Beschreibung und ein
  kleines Bild. Die Seite ruft der **Absender** ab, nie der Empfänger und nie
  der Server; abschaltbar in den Einstellungen. Anklicken öffnet den Browser
  des Systems, nicht das Fenster.

**Rollen und Rechte.** Administrator, Moderator, Mitglied, Gast — vier Rollen
und eine Tabelle statt hunderter Einzelrechte. Kontextmenüs für Eingriffe:
stumm schalten, taub schalten, in einen anderen Raum verschieben, aus dem Raum
werfen, vom Server werfen, sperren. Jede Änderung steht mit vorher/nachher im
**Prüfprotokoll**, auch abgelehnte Versuche; die **Sperrliste** nennt Grund und
Urheber und lässt sich aufheben.

**Verwaltung im Browser.** Jeder Server bringt eine Verwaltungsseite mit, die er
selbst ausliefert: Serverzustand, Mitglieder samt Rollen, Live-Protokoll. Hier
wird man Administrator, ohne einen Schlüssel aus der Hand zu geben — siehe
[Verwaltung im Browser](#verwaltung-im-browser).

**Betrieb.** Ein Container mit einem Volume und einem UDP-Port — kein
Reverse-Proxy, kein Zertifikatsantrag. Alternativ der Server als Programm unter
Linux, Windows und macOS. Sicherung nach Plan, Export/Import als lesbares JSON,
Healthcheck über ein Lebenszeichen, Unterbefehle für alles, was man in einem
Wartungsfenster braucht.

## Für wen

| Lage | Was sprechwerk dafür tut |
|---|---|
| Verein mit 20–200 Leuten, ein Server beim Betreiber oder auf einem gemieteten Root-Server | ein Container, ein Volume, ein UDP-Port — kein Reverse-Proxy, kein Zertifikatsantrag |
| Verein mit Datenschutzauflage (Mitgliederdaten, Protokolle) | Text und Sprache Ende zu Ende verschlüsselt; der Betreiber kann weder mithören noch mitlesen |
| Vorstand, der nachweisen muss, wer was getan hat | Prüfprotokoll: wer, wann, was, vorher/nachher — auch abgelehnte Versuche |
| Leute, die kein Konto anlegen wollen | Kennung ist eine Schlüsseldatei; mitnehmen = derselbe Mensch auf dem neuen Gerät |

Nicht gedacht für: Firmen mit zehntausenden Gleichzeitigen, offene Communitys
mit Fremden, Sofortnutzung im Browser ohne Installation.

## So sieht es aus

Drei Farbschemata — **dark**, **light**, **mono** —, vier Sprachen (de, en, es,
fr), vollständig per Tastatur bedienbar, Fokus sichtbar, Rollen und Namen
gesetzt.

| dark | light |
|---|---|
| ![Räume mit Chat, dunkel](docs/bilder/raeume-dark.png) | ![Räume mit Chat, hell](docs/bilder/raeume-light.png) |
| ![Prüfprotokoll, dunkel](docs/bilder/protokoll-dark.png) | ![Prüfprotokoll, hell](docs/bilder/protokoll-light.png) |
| ![Serverliste mit Zertifikat, dunkel](docs/bilder/server-dark.png) | ![Serverliste mit Zertifikat, hell](docs/bilder/server-light.png) |

Sperrliste, Einstellungen und alle Bilder in **mono** liegen in
[`docs/bilder/`](docs/bilder/).

Die Bilder sind **erzeugt**, nicht abgelegt: `npm --prefix ui run bilder` startet
die Oberfläche mit dem eingebauten Klickdummy (kein Server, keine Soundkarte),
klickt sich durch alle Ansichten und nimmt jede in allen drei Schemata auf — mit
fester Uhrzeit, deshalb liefert jeder Lauf auf derselben Maschine dieselben
Bytes. Dabei prüft es im echten Browser, was ein Test ohne Browser nicht sieht
(etwa, dass jeder Name in der Raumliste sichtbar ist).

Was das Fenster anders macht als TeamSpeak:

* **Prüfprotokoll** mit vorher/nachher und **abgelehnten** Versuchen.
* **Sperrliste** — wer, warum, von wem, mit Knopf zum Aufheben.
* **Schlüsselstand** — fehlt im eigenen Raum der Sprachschlüssel, steht dort
  „du hörst nichts" mit einem Knopf zum Nachfordern, statt Stille ohne Grund;
  fehlt der Schlüssel für den Chat, sagt das Eingabefeld, worauf es wartet.
* **Qualität in der Statuszeile** — verlorene Rahmen als Zahl, Umlaufzeit in
  Millisekunden, nicht als Symbol.
* **Eigene Kennung** in Vierergruppen, mit Hinweis, was man weitergeben darf.
* **Zertifikat des Servers** sichtbar, Zeichen für Zeichen vergleichbar mit dem,
  was der Server beim Start anzeigt.

## Im Vergleich

TeamSpeak 3 tut seit 2006, was es soll. Es wird aber nicht dadurch unordentlich,
dass Funktionen fehlen, sondern durch das, was drumherum gewachsen ist.

| | TeamSpeak 3 | sprechwerk |
|---|---|---|
| Lizenz | proprietär; kostenlose Lizenz mit begrenzter Platzanzahl, Lizenzdatei nötig | MIT, keine Platzgrenze, keine Lizenzdatei |
| Quellcode | geschlossen (Client und Server) | offen: Server, Client, Protokoll, Oberfläche |
| Ports | UDP 9987 Sprache, TCP 10011 Abfrage, TCP 30033 Dateien | **ein** UDP-Port (QUIC) plus TCP 7444 für die Verwaltung auf dem Server selbst |
| Verschlüsselung | eigene Krypto zwischen Client und Server; der Server kann Sprache entschlüsseln | TLS 1.3 mit angeheftetem Zertifikat, hybrid quantensicher im Transport; Text und Sprache **Ende zu Ende** |
| Kennung | dauerhafte UID, kopierbar | Ed25519-Besitznachweis je Verbindung, mitnehmbare Schlüsseldatei |
| Rechte | einige hundert Einzelrechte, Vererbung | vier Rollen, feste Tabelle |
| Protokoll | binär, nicht dokumentiert (nachgebaut) | JSON-Rahmen, längenpräfixiert, versioniert ([`SPEC.md`](SPEC.md) §7, [`docs/datenfluss.md`](docs/datenfluss.md)) |
| Oberfläche | Windows/macOS/Linux; Mobil-App kostenpflichtig | Fenster (Tauri 2, Linux/Windows/macOS), drei Farbschemata, vier Sprachen; Browser später über WebTransport |
| Text | dauerhaft, serverseitig gespeichert und lesbar | dauerhaft, aber der Server speichert nur Geheimtext — lesen können die Mitglieder |
| Reaktionen, Effekte, Link-Vorschau | nicht vorhanden (nur Text) | Reaktionen, Effekte, Vorschau; die Seite ruft der Absender ab |
| Betriebswerkzeug | Abfrage-Schnittstelle, Hoster-Panels | Datei + Umgebungsvariablen + Unterbefehle (`beispiel`, `export`, `import`, `sicherung`, `--gesundheit`), dazu eine Verwaltung im Browser |
| Prüfprotokoll im Client | nein | ja, für Administrator und Moderator |
| Dateien, Status, Video, Plugins | vorhanden (teils seit Jahren) | **nicht vorhanden** — bewusst „später", nicht „gestrichen" |
| Reife | 20 Jahre, großes Ökosystem | 1.2, ein Projekt, ein Maintainer |

**Was TeamSpeak besser kann** — und was wir nicht kleinreden: Mobile Apps für
iOS und Android, Dateitransfer im Client, eine Plugin-Schnittstelle, etabliertes
Hosting bei vielen Anbietern, und zwei Jahrzehnte Fehler, die schon gefunden
sind. Wer das braucht, ist mit TeamSpeak heute besser dran. Ein Latenzvergleich
mit demselben Ton steht noch aus ([`docs/QUALITAET.md`](docs/QUALITAET.md) §1).

Angaben zu TeamSpeak stammen aus dessen öffentlicher Dokumentation und
Lizenzbedingungen (Stand 2026) und können sich ändern.

## Stand heute

Lauffähig und geprüft: **284 Tests** im Arbeitsbereich, **43** in der
Fensteranwendung, **84** in der Oberfläche, dazu die Ansichten im echten Browser
und Messskripte, die einen echten Server starten.

* **Durchsatz und Antwortzeit** — Durchlauf einer Nachricht durch den Server
  p99 **21–29 µs**, Beitritt p95 **1,8–2,8 ms**, Aufpreis der Quantensicherheit
  **+50 µs** je Verbindungsaufbau (`scripts/latenz.sh`, `scripts/lastprobe.sh`,
  [`docs/QUALITAET.md`](docs/QUALITAET.md) §2).
* **Ende zu Ende** — Sprache mit einem Schlüssel je Raum und Epoche
  (ChaCha20-Poly1305), Text mit einem Serverschlüssel, den nur die Mitglieder
  haben. In Datenbank und Sicherung steht ausschließlich Geheimtext, Klartext
  nimmt der Server nicht an — auch Reaktionen und Vorschauen nicht.
* **Verwaltung** — im Browser, mitgeliefert, nur auf dem Rechner des Servers
  erreichbar.

Noch nicht gebaut (bewusst): Dateien und Bilder im Chat (geplant,
[ADR 0012](docs/adr/0012-alles-ende-zu-ende.md) Stufe 3), GIF-Suche,
Status/Abwesenheit, Video, Bildschirmfreigabe, Suche. Was als Nächstes kommt,
steht in [`SPEC.md`](SPEC.md) §11 und
[`docs/adr/0010-quantensicherheit.md`](docs/adr/0010-quantensicherheit.md)
(Schritt 2: Raumschlüssel hybrid und mit Forward Secrecy).

## Installation

Zwei Teile: der **Server** (einmal, beim Betreiber) und das **Fenster** (bei
jedem, der mitredet).

### Fertige Pakete

Jedes [Release](https://github.com/patimweb-labs/sprechwerk/releases) bringt mit:

| Datei | Für |
|---|---|
| `Sprechwerk_<version>_amd64.deb` | Fenster — Debian, Ubuntu (ab 24.04) |
| `Sprechwerk-<version>-1.x86_64.rpm` | Fenster — Fedora, openSUSE |
| `Sprechwerk_<version>_amd64.AppImage` | Fenster — jedes andere Linux, ohne Installation |
| `Sprechwerk_<version>_x86_64.flatpak` | Fenster — als Flatpak (Silverblue, Steam Deck, …) |
| `Sprechwerk_<version>_x64-setup.exe` | Fenster — Windows 10/11, Installation ohne Administratorrechte |
| `Sprechwerk_<version>_x64_en-US.msi` | Fenster — Windows, für die Verteilung über Gruppenrichtlinien |
| `Sprechwerk_<version>_universal.dmg` | Fenster — macOS 11 oder neuer, Apple Silicon und Intel |
| `sprechwerk-server-<version>-linux-x86_64.tar.gz` | Server + `sprechwerk-cli`, PC/Server |
| `sprechwerk-server-<version>-linux-aarch64.tar.gz` | dasselbe für ARM (Raspberry Pi 4/5, 64 Bit) |
| `sprechwerk-server-<version>-windows-x86_64.zip` | Server für Windows 10/11 — entpacken, `sprechwerk.exe` starten |
| `sprechwerk-server-<version>-macos-universal.tar.gz` | Server für macOS 11 oder neuer, Apple Silicon und Intel |
| `SHA256SUMS` | Prüfsummen aller Dateien (`sha256sum -c SHA256SUMS`) |

Jeder Server bringt seine [Verwaltung im Browser](#verwaltung-im-browser) mit.

`.deb` und `.rpm` ziehen Opus und `parec`/`pacat` als Abhängigkeit nach; beim
AppImage müssen beide vorhanden sein (auf fast jedem Linux-Schreibtisch sind
sie es). Die Server-Programme brauchen glibc 2.39 oder neuer (Debian 13,
Ubuntu 24.04, aktuelle Fedora) — für ältere Systeme den Container nehmen.

**Flatpak:** `flatpak install --user Sprechwerk_<version>_x86_64.flatpak` —
die GNOME-Laufzeit kommt dabei von Flathub mit. Schlüssel und Einstellungen
liegen dann unter `~/.var/app/de.weppelmann.Sprechwerk/`.

**Windows und macOS sind nicht signiert** (dafür bräuchte es ein
Code-Signing-Zertifikat bzw. ein Apple-Entwicklerkonto). Das System warnt
deshalb beim ersten Start:

* **Windows:** SmartScreen meldet „Der Computer wurde durch Windows geschützt" —
  „Weitere Informationen", dann „Trotzdem ausführen".
* **macOS:** Die App aus dem `.dmg` nach „Programme" ziehen. Beim ersten Start
  Rechtsklick auf die App → „Öffnen" → „Öffnen". Meldet macOS, die App sei
  „beschädigt", hat es nur die Download-Markierung nicht gemocht:
  `xattr -dr com.apple.quarantine /Applications/Sprechwerk.app`.
  Beim ersten Sprechen fragt macOS nach dem Mikrofon.

Opus liegt bei Windows und macOS im Paket; nichts weiter zu installieren.

### Server im Container (der übliche Weg)

```bash
npm --prefix ui run verwaltung  # Verwaltung bauen (wird ins Programm eingebettet)
docker compose up -d --build
docker compose logs -f          # Fingerabdruck und Link zur Verwaltung
```

Ohne den ersten Schritt läuft alles, nur zeigt die Verwaltung statt der
Oberfläche eine Seite mit der Bauanleitung; das fertige Abbild aus dem Release
hat sie eingebaut.

Ohne eigenen Bau: `docker run` mit dem Abbild aus dem Release —

```bash
docker run -d --name sprechwerk -p 7443:7443/udp -p 127.0.0.1:7444:7444/tcp \
  -v sprechwerk-daten:/data ghcr.io/patimweb-labs/sprechwerk:1.2
```

Fest auf eine Minor-Version statt `latest` setzen, wenn der Betrieb ruhig
bleiben soll: ein Update migriert die Daten ohne Rückweg
([`SPEC.md`](SPEC.md) §2) — das soll ein bewusster Schritt sein.

Ein Volume (`/data`), ein Port nach außen (`7443/udp`), sonst nichts. Weitere
Einstellungen in `docker-compose.yml` oder als Umgebungsvariablen (siehe
[Betrieb](#betrieb)). In der Firewall nur **UDP** 7443 freigeben; TCP braucht
QUIC nicht. Die Verwaltung (7444/tcp) ist nur auf dem Host selbst erreichbar.

Mit **Podman**: `podman build --format docker -t sprechwerk .` — im
Standardformat (OCI) verwirft Podman die `HEALTHCHECK`-Anweisung.

Der Container prüft sich über ein Lebenszeichen (`<daten>/lebt`, alle 10 s neu
geschrieben, `sprechwerk --gesundheit` wertet es aus) — **nicht** über eine
Testverbindung: der erste Client wird Administrator, ein Healthcheck darf diesen
Platz nicht belegen.

### Server ohne Container

```bash
npm --prefix ui run verwaltung  # optional: Verwaltung im Browser einbetten
cargo build --release -p sprechwerk-server
SPRECHWERK_DATEN=./data ./target/release/sprechwerk
```

Braucht nur Rust (die Fassung legt `rust-toolchain.toml` fest, `rustup` holt sie
selbst) und einen C-Compiler für den TLS-Anbieter (`aws-lc-rs`); `cmake` ist
nicht nötig. `sprechwerk beispiel > sprechwerk.toml` erzeugt eine vollständige
Konfiguration zum Anpassen.

**Unter Windows und macOS** das Archiv aus dem Release entpacken und
`sprechwerk.exe` (Windows) bzw. `sprechwerk` (macOS) starten: ein Konsolenfenster
zeigt das Protokoll, der Browser öffnet die Verwaltung. Daten, Zertifikat und
Sicherungen landen in `daten/` neben dem Programm; Schließen des Fensters beendet
den Server. Auch hier warnen SmartScreen bzw. Gatekeeper beim ersten Start (unter
macOS: `xattr -d com.apple.quarantine sprechwerk sprechwerk-cli`). Die Firewall
fragt nach UDP 7443; für Mitglieder von außen muss der Router diesen Port
weiterleiten.

### Fenster aus dem Quellbaum

Voraussetzungen auf jeder Plattform: Rust (über `rustup`), Node.js 22 und die
Komponentenbibliothek [`weppelmann-ui`](https://github.com/Smotherer007/weppelmann-ui)
**neben** diesem Verzeichnis (`../weppelmann-ui`, dort einmal
`npm ci && npm run tokens`).

```bash
cd ui && npm ci && cd ..
cargo build --release --manifest-path crates/desktop/Cargo.toml --features custom-protocol
# Programm: crates/desktop/target/release/sprechwerk-desktop
```

`--features custom-protocol` bettet die Oberfläche ein; ohne das Merkmal lädt
das Fenster die Entwicklungsadresse (Port 1423).

Pakete wie im Release baut `npm --prefix ui run pakete` — die Tauri-CLI kommt
dabei aus `ui/node_modules`, festgelegt auf eine Version; nichts weiter zu
installieren. Ergebnis unter `crates/desktop/target/release/bundle/`. Welche
Pakete auf welchem System entstehen, steht in
`crates/desktop/tauri.<system>.conf.json`; das Flatpak entsteht aus dem `.deb`
(Anleitung im Kopf von
`crates/desktop/packaging/flatpak/de.weppelmann.Sprechwerk.yml`).

**Linux — Fedora** (so geprüft):

```bash
sudo dnf install gcc pkgconf-pkg-config webkit2gtk4.1-devel gtk3-devel \
  libsoup3-devel javascriptcoregtk4.1-devel librsvg2-devel opus pulseaudio-utils
```

**Linux — Debian/Ubuntu** (so in der CI, Ubuntu 24.04):

```bash
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev patchelf \
  libopus0 pulseaudio-utils
```

Ton läuft unter Linux über das vorhandene PipeWire/PulseAudio (`parec`/`pacat`);
ALSA-Entwicklungspakete braucht es nur für `--features cpal`
([`docs/AUDIO.md`](docs/AUDIO.md)).

**Windows 10/11:** Rust mit der MSVC-Werkzeugkette (Visual Studio Build Tools),
CMake, WebView2 (in Windows 11 enthalten). Geräte über WASAPI, ohne
Zusatzpakete. `crates/desktop/opus/opus.dll` muss vor dem Bau da sein — Tauri
legt sie neben die `.exe` und prüft schon beim `cargo build`, dass es sie gibt.
Wie sie entsteht (Opus aus den Quellen, statische C-Laufzeit), steht im Schritt
„Opus bauen" in `.github/workflows/pakete.yml`.

**macOS:** Xcode-Befehlszeilenwerkzeuge (`xcode-select --install`), CMake.
Geräte über CoreAudio. Für den Bau gehört `libopus.0.dylib` nach
`crates/desktop/opus/` (Tauri packt sie in `Contents/Frameworks`); zum
Ausprobieren ohne Paket genügt `brew install opus`.

Windows und macOS bauen **in der CI bei jedem PR** (`pakete.yml`, mit Clippy
auf der jeweiligen Plattform), der Server ebenso (`server.yml`). Auf echten
Geräten ist dort noch niemand gewesen — Rückmeldungen von Windows und macOS sind
willkommen.

## Das erste Mal verbinden

1. Server starten. Er zeigt:

   ```
   Zertifikat-Fingerabdruck (SHA-256):
     51a2:1556:20e3:3bd9:…:eeba:49fe
   ```

2. Im Fenster die Adresse eintragen, verbinden. Der Fingerabdruck wird
   **angeheftet** (wie bei SSH) und ab dann bei jeder Verbindung geprüft. Weicht
   er ab, bricht die Verbindung ab, **bevor** Kennung oder Serverpasswort über
   die Leitung gehen.
3. Der **erste** Client wird Administrator. Wer zuerst kommt, verwaltet — also
   zuerst du, dann die anderen. War jemand schneller, oder soll noch jemand
   Administrator werden: in der [Verwaltung](#verwaltung-im-browser) beim Namen
   die Rolle wählen.

Wurde der Server neu aufgesetzt (neues Zertifikat), muss der gemerkte Wert
einmal ausdrücklich vergessen werden: Serverliste → „Zertifikat vergessen" →
neu verbinden.

## Verwaltung im Browser

Jeder Server bringt eine Verwaltungsseite mit — dieselben Bausteine und Themes
wie das Fenster:

* **Server:** Name, Port, Laufzeit, Verbundene, letzte Sicherung und der
  Fingerabdruck des Zertifikats zum Weitergeben an die Mitglieder.
* **Mitglieder und Rollen:** wer da ist und war, mit Rolle. Hier wird man
  **Administrator**, ohne einen Schlüssel aus der Hand zu geben: mit der App
  verbinden, dann hier beim eigenen Namen „Administrator" wählen. Die Änderung
  gilt sofort, auch für Verbundene. Der letzte Administrator bleibt es.
* **Protokoll:** live, dieselben Zeilen wie auf der Konsole bzw. in
  `docker compose logs`.

Erreichbar unter `http://127.0.0.1:7444` — **nur auf dem Rechner des Servers**.
Den Zugangscode schreibt der Server beim Start ins Protokoll, als fertigen Link:

```
INFO Verwaltung bereit: http://127.0.0.1:7444/anmelden?code=3f9c…
```

Als Programm unter Windows/macOS (und Linux mit Bildschirm) öffnet sich dieser
Link von selbst. Auf einem Server ohne Bildschirm von einem anderen Rechner aus:
`ssh -L 7444:127.0.0.1:7444 <server>`, dann den Link im eigenen Browser öffnen.

Der Code ist bei jedem Start neu (128 Bit Zufall); ein fester lässt sich setzen
(`verwaltung.code`, mindestens 16 Zeichen). Räume, Sperren und Konfiguration
ändert die Verwaltung nicht — das bleibt bei Fenster und Datei
([`docs/adr/0011-verwaltung-im-browser.md`](docs/adr/0011-verwaltung-im-browser.md)).

## Sicherheit und Daten

**Auf der Leitung:** QUIC = TLS 1.3 mit AEAD, Schlüsselwechsel und Forward
Secrecy. Schlüsselvereinbarung hybrid `X25519MLKEM768` — klassisch *und*
quantensicher in einem Handschlag ([`docs/adr/0010-quantensicherheit.md`](docs/adr/0010-quantensicherheit.md)).

**Anmeldung:** kein Passwortkonto, sondern ein Ed25519-Schlüsselpaar; der Server
fordert je Verbindung einen Beweis über eine Zufallszahl an. Die Kennung liegt
als Datei auf dem Rechner — wer sie kopiert, ist derselbe Mensch.

**Alles Ende zu Ende** ([ADR 0012](docs/adr/0012-alles-ende-zu-ende.md)):
lesen dürfen die **Mitglieder**, nicht der Betreiber. Wer als Betreiber mitlesen
will, verbindet sich mit dem Client — dann ist er Mitglied wie jeder andere und
steht in der Mitgliederliste.

* **Text:** ein Serverschlüssel für alle Mitglieder. Der erste Client erzeugt
  ihn, danach gibt ihn ein Mitglied dem nächsten weiter — verpackt für dessen
  Kennung und unterschrieben; der Server vermittelt nur und kennt den Schlüssel
  nie. Nach einer Sperre gilt ein neuer Schlüssel. Reaktionen und Vorschauen
  reisen im selben Umschlag wie der Text.
* **Sprache:** jeder Raum, ohne Schalter. Ein Gruppenschlüssel pro Raum und
  Epoche, einzeln per X25519-Umschlag verteilt. Beim Verlassen dreht die
  Epoche — wer geht, hört nichts mehr.
* **Link-Vorschau:** die Seite ruft **der Absender** ab (8 s Frist, höchstens
  512 KiB HTML, 8 MiB Bild, neutrale Kennung, keine Cookies). Er schickt Titel,
  gekürzte Beschreibung und ein auf 480 px verkleinertes, neu geschriebenes JPEG
  — nicht das Originalbild. Wer das nicht will, schaltet es in den Einstellungen
  ab; Links bleiben anklickbar.

| Sieht der Server | Sieht er nicht |
|---|---|
| wer wann in welchem Raum ist und spricht | was gesagt wird |
| dass jemand schreibt, wie lang (ungefähr), wann | was geschrieben wird |
| Namen, Rollen, Kennungen der Mitglieder | Reaktionen, Effekte, Vorschauinhalte |

**Der Preis:** Wer neu ist (oder auf einem neuen Gerät ohne die Schlüsseldatei)
und allein online, kann erst lesen und schreiben, wenn ein Mitglied online
kommt, das den Schlüssel hat. Gehen alle Geräte mit Schlüssel verloren, ist der
Verlauf für niemanden mehr lesbar — auch nicht für den Betreiber. Der Server
kann nicht mehr nach Inhalt suchen.

**Was das nicht leistet:** Der Betreiber sieht Metadaten. Ein Server, den der
Betreiber **umbaut**, kann ein erfundenes Mitglied anmelden und bekäme dann den
Schlüssel wie jedes Mitglied — es stünde aber in der Mitgliederliste. Ein
Abgleich der Kennungen außerhalb des Servers (wie Sicherheitsnummern) ist noch
nicht gebaut. Ein Mitglied, das mitschneidet, hat den Schlüssel. Ein
kompromittiertes Gerät ist kompromittiert. Offen und dokumentiert: die Umlage
des Raumschlüssels hat noch keine Forward Secrecy, und Anmeldung wie Umlage sind
klassisch. Die hybride Umlage mit Wegwerfschlüsseln ist gebaut und geprüft, aber
noch nicht verdrahtet ([`docs/adr/0010-quantensicherheit.md`](docs/adr/0010-quantensicherheit.md),
Schritt 2).

**Datenschutz:** Alles liegt beim Betreiber — eine SQLite-Datei und Sicherungen
als JSON unter `<daten>`, beides nur für den Eigentümer lesbar (0600), sonst
nichts. Keine Telemetrie, keine Cloud, kein Konto bei uns, keine Verbindung zu
Dritten. Gespeichert werden je Mitglied Anzeigename, öffentlicher Schlüssel,
Rolle und „zuletzt gesehen", dazu die Texte als Geheimtext (gelöscht nach der
Aufbewahrungsfrist, wenn eine gesetzt ist), das Prüfprotokoll und die
Sperrliste — bei einer Adresssperre mit IP-Adresse. Sprache wird **nicht**
gespeichert. Das Betriebsprotokoll auf der Konsole (`docker compose logs`) nennt
bei jeder Anmeldung die Adresse; wie lange es aufbewahrt wird, entscheidet der
Betreiber.

Im Fenster liegen nur die eigene Schlüsseldatei, die Serverschlüssel für den
Chat (`serverschluessel.json`), die gemerkten Server mit ihren Fingerabdrücken
und die Einstellungen — darin, wenn man es so will, das Serverpasswort.
Schlüsseldateien und Einstellungen sind unter Linux und macOS nur für den
eigenen Benutzer lesbar (0600, schon beim Anlegen); unter Windows schützt sie
das Benutzerprofil. Keine Secrets im Code; die Konfiguration kommt aus Datei und
Umgebung (Beispiel in `.env.example`).

## Betrieb

Konfiguration über `sprechwerk.toml` (Pfad: `SPRECHWERK_KONFIG`), jeder Wert auch
als Umgebungsvariable — `sprechwerk beispiel` gibt eine vollständige Vorlage aus.

| Datei | Umgebung | Bedeutung |
|---|---|---|
| `server.name` | `SPRECHWERK_NAME` | Anzeigename |
| `server.port` | `SPRECHWERK_PORT` | UDP-Port (Vorgabe 7443) |
| `server.bind` | `SPRECHWERK_BIND` | Adresse (Vorgabe `0.0.0.0`) |
| `server.passwort` | `SPRECHWERK_PASSWORT` | leer = offen |
| `server.standard_rolle` | `SPRECHWERK_STANDARD_ROLLE` | Rolle neuer Mitglieder (`mitglied`, oder `gast` für zu) |
| `server.daten` | `SPRECHWERK_DATEN` | Datenverzeichnis (Vorgabe `/data`, wenn vorhanden, sonst `daten/` neben dem Programm) |
| `server.aufbewahrung_tage` | `SPRECHWERK_AUFBEWAHRUNG_TAGE` | Texte nach so vielen Tagen löschen; 0 = unbegrenzt |
| `server.rahmen_je_sekunde` | `SPRECHWERK_RAHMEN_JE_SEKUNDE` | Steuerrahmen je Verbindung und Sekunde, darüber wird getrennt (Vorgabe 100; Sprache zählt nicht) |
| `server.staukontrolle` | `SPRECHWERK_STAUKONTROLLE` | `bbr` (Vorgabe, gleichmäßiger für Sprache) oder `cubic` |
| `server.sprachpuffer_kib` | `SPRECHWERK_SPRACHPUFFER_KIB` | Sendepuffer für Sprache — klein halten: verwerfen statt anstauen (Vorgabe 64) |
| `server.verbindungen_je_adresse` | — | gleichzeitige Verbindungen je IP-Adresse (Vorgabe 8) |
| `sicherung.aktiv` | `SPRECHWERK_SICHERUNG_AKTIV` | Sicherung nach Plan |
| `sicherung.intervall_minuten` | `SPRECHWERK_SICHERUNG_INTERVALL_MINUTEN` | Abstand (Vorgabe 1440 = täglich) |
| `sicherung.behalten` | `SPRECHWERK_SICHERUNG_BEHALTEN` | Anzahl der Sicherungen (Vorgabe 14) |
| `sicherung.verzeichnis` | `SPRECHWERK_SICHERUNG_VERZEICHNIS` | Ziel der Sicherungen (Vorgabe `<daten>/sicherungen`) |
| `verwaltung.aktiv` | `SPRECHWERK_VERWALTUNG_AKTIV` | Verwaltung im Browser anbieten (Vorgabe ja) |
| `verwaltung.bind` | `SPRECHWERK_VERWALTUNG_BIND` | Adresse (Vorgabe `127.0.0.1`; im Container `0.0.0.0`, auf dem Host nur `127.0.0.1`) |
| `verwaltung.port` | `SPRECHWERK_VERWALTUNG_PORT` | TCP-Port (Vorgabe 7444) |
| `verwaltung.code` | `SPRECHWERK_VERWALTUNG_CODE` | fester Zugangscode, mind. 16 Zeichen; leer = bei jedem Start neu |
| `verwaltung.browser` | `SPRECHWERK_VERWALTUNG_BROWSER` | beim Start den Browser öffnen, wenn es einen Bildschirm gibt (Vorgabe ja) |
| — | `SPRECHWERK_LOG` | Protokollstufe, z. B. `sprechwerk=debug` (Vorgabe `sprechwerk=info`) |

Betriebswerkzeuge:

```bash
sprechwerk beispiel             # vollständige Konfiguration ausgeben
sprechwerk export <datei>       # Zustand als lesbares JSON schreiben
sprechwerk import <datei>       # Zustand einspielen (sichert vorher automatisch)
sprechwerk sicherung            # Sofortsicherung
sprechwerk --gesundheit         # Lebenszeichen prüfen (für den Healthcheck)
```

Im Container: `docker compose exec sprechwerk sprechwerk sicherung`. Vor einer
Migration kopiert der Server die Datenbank; ein neueres Format wird abgelehnt
statt halb gelesen.

## Selbst prüfen

```bash
scripts/pruefen.sh              # alles: Form, Werkzeuge, Tests, Ansichten im Browser, Sprachproben
scripts/pruefen.sh --nur-tests  # schnell
scripts/latenz.sh               # p50/p95/p99 durch den Server
scripts/lastprobe.sh            # Antwort und Rundruf unter Last
scripts/rundlauf.sh             # Server + zwei Clients von Hand
scripts/sprechprobe.sh          # Ton durch den Server, ohne Mikrofon
scripts/verlustprobe.sh         # Verlust und Jitter, über einen Proxy
scripts/audio-ende-zu-ende.sh   # echter Gerätepfad mit virtuellen Geräten
npm --prefix ui run bilder      # alle Ansichten in drei Schemata → docs/bilder/
```

Dieselben Schritte laufen bei jedem Push (`.github/workflows/pruefen.yml`; die
Bilder legt die CI als Artefakt ab), und ein Haken hält vor dem Push an
(`git config core.hooksPath .githooks`).

## Mitentwickeln

Die verbindlichen Regeln stehen in [`AGENTS.md`](AGENTS.md) — für Menschen wie
für Werkzeuge. Das Wichtigste in Kürze:

* **Oberfläche ohne Backend:** `npm --prefix ui run dev` startet sie allein
  (Port 1423, Klickdummy über `?mock=1`). Wer für eine Oberflächenaufgabe einen
  Server oder eine Soundkarte braucht, hat die Grenze falsch gezogen.
* **Keine Farben, Größen oder Abstände im Code**, nur Tokens aus
  `@weppelmann/tokens`; ein Test prüft, dass jedes benutzte Token existiert und
  kein `px` in den Stilen steht.
* **Tests sind Teil der Lieferung** — ein Fix ohne Test ist ein Fix auf Probe.
* **Immer über Branch und Pull Request**, Conventional Commits.
* **Releases macht niemand von Hand.** Nach jedem Merge auf `master` hält
  release-please einen PR „chore: Release x.y.z" aktuell (Versionen,
  `CHANGELOG.md`). Wer ihn mergt, löst das Release aus: Tag, Pakete, Abbild,
  Prüfsummen (`.github/workflows/release.yml`). Ging ein Bau schief: „Run
  workflow" mit dem Tag baut die Dateien neu.

## Aufbau

```
crates/domain        Zustand und Regeln: Räume, Rollen, Rechte, Protokoll — kein Netz
crates/protocol      Sprache auf der Leitung: Rahmen, Nachrichten, Sprachkopf, Geheimtext
crates/store         SQLite, Export/Import, Migrationen, Sicherungen
crates/adapters      Netz: QUIC, Anmeldung, Zertifikatsprüfung, Ende-zu-Ende, Klang
crates/audio         Aufnahme/Wiedergabe, Opus, Verlustausgleich
crates/linkvorschau  Link-Vorschau: Seite abrufen, Bild verkleinern (nur im Client)
crates/ipc           Vertrag zum Fenster: Befehle, Ereignisse, Modellstand
crates/server        Dienst: Listener, Befehlsausführung, Mitschrift, Sicherung, Verwaltung
crates/cli           Referenzclient auf der Kommandozeile
crates/desktop       Fensteranwendung (Tauri 2, eigene Sperrdatei) + ui/ (Vue 3)
```

Richtung der Abhängigkeiten: `domain ← adapters ← ipc ← ui`. `domain` kennt kein
Netz — deshalb sind Rechte, Räume und Verläufe ohne Sockets prüfbar. Der Server
baut weder HTTP-Client noch Bilddecoder mit; die Link-Vorschau liegt deshalb in
einem eigenen Crate, das nur die Clients ziehen. Mehr in
[`docs/architektur.md`](docs/architektur.md) und
[`docs/datenfluss.md`](docs/datenfluss.md).

## Protokoll in einem Blick

```text
Client                                    Server
  ── QUIC (TLS 1.3 hybrid, ALPN sprechwerk/1) ──►
  ◄── Anforderung { nonce }                 Beweisaufforderung
  ── Hallo { kennung, beweis, name } ──►    Ed25519-Signatur über den Nonce
  ◄── Willkommen { Räume, Mitglieder }      oder Fehler
  ◄──► Rahmen (4 Byte Länge + JSON)         Text als Geheimtext, Befehle, Antworten
  ◄──► Datagramme (6 Byte Kopf + Opus)      Sprache, unzuverlässig
```

Der Server öffnet den Steuerstrom, weil ein QUIC-Strom der Gegenstelle erst mit
den ersten Bytes sichtbar wird. Je Verbindung kommt die Antwort auf einen Befehl
vor jeder Meldung, die ein späterer Befehl eines anderen auslöst — der Client
darf sich darauf verlassen ([`docs/datenfluss.md`](docs/datenfluss.md) §2). Der
Raum steht **nicht** im Sprachkopf, sondern im Sitzungszustand des Servers;
damit ist Umleiten in fremde Räume unmöglich. Begleitend: das Handbuch zu
TeamSpeak 3 auf Protokollebene liegt als Nachbardokumentation
(`../fernwerk/docs/handbuch/`), nicht in diesem Repository.

## Lizenz

MIT — siehe [`LICENSE`](LICENSE).

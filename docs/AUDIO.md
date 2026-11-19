# Audiopfad: was gebaut ist, was fehlt, was zu installieren ist

## Aufteilung

| Ort | Inhalt | prüfbar ohne Soundkarte |
|---|---|---|
| `crates/adapters/src/sprechpfad.rs` | Vorlauf, Jitter-Puffer, Sprechtaste, Schwelle | **ja** (13 Tests) |
| `crates/audio/` | Geräte (ALSA/PipeWire), Opus-Kodierung | nein — braucht Systempakete |

Die Trennung ist Absicht: die Logik, in der die Fehler sitzen (Reihenfolge,
Lücken, doppelte Rahmen, Vorlauf), liegt im Arbeitsbereich und ist geprüft. Das
Geräte-Zeug steckt in einem eigenen Crate, das nur gebaut wird, wenn die
Voraussetzungen da sind.

## Bauvoraussetzungen

**Keine Entwicklungspakete nötig.** `crates/audio` steht im Arbeitsbereich und
baut überall ohne `cmake` und ohne ALSA-Header:

* **Opus** wird zur **Laufzeit** geladen (`libloading`): `libopus.so.0` unter
  Linux, `libopus.dylib` unter macOS, `opus.dll` unter Windows. Gesucht wird
  zuerst neben dem Programm (Windows: neben der `.exe`; macOS:
  `Sprechwerk.app/Contents/Frameworks`, dort legen die Pakete sie ab), dann im
  System, zuletzt unter Homebrew. Fehlt sie, sagt der Client das, statt zu
  stürzen.
* **Geräte unter Linux** über das laufende PipeWire/PulseAudio (`parec`/`pacat`
  aus `pulseaudio-utils`). Wer lieber ALSA direkt will, baut mit
  `--features cpal` und braucht dann `alsa-lib-devel` (Fedora) bzw.
  `libasound2-dev` (Debian/Ubuntu) und `pkg-config`.
* **Geräte unter Windows und macOS** über `cpal` (WASAPI bzw. CoreAudio) — dort
  fest eingebunden, ohne Zusatzpakete. **Ohne Rückfall:** `parec` ist ein
  PipeWire-Programm und gibt es dort nicht. Scheitert cpal — unter macOS etwa,
  weil der Mikrofonzugriff verweigert wurde —, wird sein Fehler
  durchgereicht und im Fenster gemeldet, statt hinter einem zweiten Versuch zu
  verschwinden, der ohnehin scheitern muss. Auch Geräteliste, Pegelanzeige und
  Prüfton in den Einstellungen gehen dort über cpal
  (`sprechwerk_audio::geraet::systemgeraete`); unter Linux über
  `pactl`/`parec`/`paplay`. Das Fenster entscheidet danach, welche Werkzeuge
  im `PATH` liegen, nicht nach dem Betriebssystem.

```bash
# Laufzeit unter Linux (meist schon vorhanden)
sudo dnf install opus pulseaudio-utils            # Fedora
sudo apt install libopus0 pulseaudio-utils        # Debian/Ubuntu

cargo test -p sprechwerk-audio
```

Die Tests von `crates/audio` brauchen **keine** Soundkarte: Geräte und Codec
stecken hinter Schnittstellen, und der echte Gerätepfad hat ein eigenes
Werkzeug (`scripts/audio-ende-zu-ende.sh`, virtuelle Geräte).

## Kette

```text
Mikrofon ──► Vorlauf (60 ms Ring) ──► Opus 20 ms/32 kbit/s ──► QUIC-Datagramm
                ▲
        Push-to-Talk: beim Drücken geht der Vorlauf **vor** dem Live-Ton raus

QUIC-Datagramm ──► Ordnung + Verlustausgleich (80 ms) ──► Opus-Decoder ──► Lautsprecher
```

## Zahlen

| Größe | Wert | Herkunft |
|---|---|---|
| Rahmen | 20 ms | Protokoll (Sprachkopf, Folge je Sprecher) |
| Bitrate | 32 kbit/s mono | Vorgabe; TeamSpeak nutzt je Kanalqualität 6–32 kbit/s |
| Vorlauf | 60 ms (einstellbar 0–200) | ersetzt TeamSpecks `delay_ptt_msecs=300` |
| Jitter-Puffer | 4 Rahmen = 80 ms (eine Paritätsgruppe) | ersetzt den 60-ms-Puffer: Verlustausgleich braucht eine Gruppe Aufschub |
| Kopfaufschlag | 6 von 86 Byte = 7,0 % | Sprachkopf Version/Codec/Folge/Sprecher |

## Rauschunterdrückung und Noisegate

Zwei verschiedene Dinge, die oft verwechselt werden:

| | Wirkung | Wo | Einstellung |
|---|---|---|---|
| **Rauschunterdrückung** | **RNNoise** (neuronales Netz) filtert Störgeräusche im Signal und liefert eine Sprachwahrscheinlichkeit | Aufnahmepfad, vor der Kodierung | `rauschunterdrueckung` (Vorgabe: **an**) |
| **Noisegate** | unterhalb der Sprechschwelle wird **gar nichts** gesendet | Aufnahmepfad, vor der Kodierung | `noisegate` (Vorgabe: **an**), Schwelle über `schwelle` |
| **Echounterdrückung** | rechnet die eigene Wiedergabe aus der Aufnahme (AEC3, reines Rust) | Aufnahmepfad, **vor** dem Entrauschen | `echounterdrueckung` (Vorgabe: **an**) |

Umsetzung im Sprachpfad (noch zu bauen, Einstellungen liegen schon):

1. Aufnahme → **RNNoise** (`nnnoiseless`, reines Rust) auf 10-ms-Rahmen; es
   liefert den entrauschten Rahmen **und** die Sprachwahrscheinlichkeit.
2. Aufnahme → **Gate**: unter der Schwelle wird nichts gesendet — **außer**
   RNNoise erkennt klar Sprache (≥ 0,6). Der Pegel allein ist ein schlechter
   Richter: ein Rascheln ist laut, ein leises „ja" nicht.
3. TeamSpeak macht dasselbe (`denoise=true`, `denoiser_level=1`, `vad=true`,
   `voiceactivation_level=-40` in dessen Clientprofil).

Gemessen an weißem Rauschen (Tests in `crates/adapters/src/klang.rs`):
**98 % des Rauschens verschwinden**, Sprache bleibt zu ~66 % erhalten, die
Sprachwahrscheinlichkeit trennt beides (0,02 gegen 0,75). Der frühere
Pegel-Expander ist ersetzt — er war ein Notbehelf.

## Echounterdrückung (AEC)

Wer über **Lautsprecher** spricht, nimmt die anderen mit auf — das Mikrofon
hört die Boxen. Erst klingt es hohl, dann pfeift es. Kopfhörer lösen das, aber
ein Verein kann das nicht voraussetzen.

Deshalb wird der bekannte Anteil **herausgerechnet**: was wir selbst abspielen,
liegt in diesem Prozess als Zahlen vor. Ein adaptiver Filter (AEC3 aus WebRTC,
als reines Rust im `aec3`-Crate) schätzt den Weg „Wiedergabe → Raum → Mikrofon"
und zieht ihn ab.

```
Wiedergabe ──┬──► Lautsprecher ──► Raum ──► Mikrofon ──┐
             │                                          ├─► AEC3 ──► RNNoise ──► Gate ──► Opus
             └────────────── Referenz ─────────────────┘
```

**Reihenfolge:** AEC **zuerst** (es braucht das unverfälschte Signal), dann
RNNoise, dann das Gate. Die Kette aus dem Crate wird ausdrücklich **nur** als
AEC betrieben (`enable_noise_suppression(false)`, `enable_gain_controller2(false)`)
— zweimal entrauschen klingt dumpf, und eine automatische Verstärkung würde die
Sprechschwelle unbrauchbar machen.

### Gemessen (`cargo test --release -p sprechwerk-adapters echo`)

| Größe | Wert |
|---|---|
| Echo wird gesenkt um | **39,4 dB** (Echo 40 ms verzögert, 30 % Pegel) |
| Rechenzeit | **0,7 %** eines Kerns in Release (20 % im Debug-Bau) |
| Doppelsprechen | eigene Stimme wird **nicht** lauter und nicht mitgelöscht |

Grenzen, ehrlich: bei gleichzeitigem Sprechen (Doppelsprechen) wird das Echo
schlechter unterdrückt — das ist bei jedem AEC so; und ändert sich der Raum
(jemand läuft herum), braucht der Filter einen Moment. Kopfhörer bleiben die
bessere Wahl, wenn man sie hat.

## Niemals die eigene Wiedergabe aufnehmen

Drei Regeln, damit niemand sich selbst oder die anderen doppelt hört:

1. **Monitore sind keine Mikrofone.** PipeWire/PulseAudio bietet zu jedem
   Ausgang eine `.monitor`-Quelle an — sie liefert genau das, was der Rechner
   *abspielt*. Als Mikrofon gewählt entsteht eine Rückkopplungsschleife
   (Hall, Pfeifen, doppelte Stimmen).
   * Umgesetzt: `audio.rs` filtert `.monitor` aus der Mikrofonliste **und**
   lehnt es ab, wenn es von außen hereingereicht wird (Test
   `monitor_als_mikrofon_wird_abgelehnt`).
2. **Der Server schickt Sprache nicht an den Sprecher zurück.** Der Sprachpfad
   verteilt an alle im Raum **außer** dem Absender. Wer das ändert, baut sich
   ein Echo.
3. **Kein „Mitschnitt der eigenen Ausgabe" als Funktion.** Ein „Hörtest" spielt
   einen kurzen Ton — er läuft nicht über den Sprachpfad und wird nicht
   weitergegeben.

Was das **nicht** leistet: Echounterdrückung (AEC). Wer über Lautsprecher
spricht, nimmt die anderen trotzdem mit auf. Dagegen hilft nur Kopfhörer — oder
später ein AEC im Client (Kosten: eigener Baustein, hohe Rechenlast, nur mit
Referenzsignal sinnvoll).

## Stand: gebaut und gemessen

Der Pfad ist **gebaut und läuft** — im Fenster und als Messwerkzeug.

| Stück | Wo | Geprüft durch |
|---|---|---|
| Hochpass, Rauschunterdrückung (RNNoise), Noisegate | `adapters/src/klang.rs` | 8 Einzeltests (Rauschen −98 %, Sprache bleibt, VAD trennt, Gate mit Hysterese) |
| Echounterdrückung (AEC3) | `adapters/src/echo.rs` | 4 Einzeltests (39 dB, Rechenzeit, Doppelsprechen, Rate) |
| Vorlauf (60 ms), Jitter-Puffer, Sprechsteuerung | `adapters/src/sprechpfad.rs` | 13 Einzeltests |
| Empfangsordnung + Verlustausgleich (80 ms) | `audio/src/verlust.rs`, `audio/src/empfang.rs` | 9 Einzeltests |
| Opus, **zur Laufzeit geladen** (`libopus.so.0` / `libopus.dylib` / `opus.dll`) | `audio/src/opus.rs` | 6 Einzeltests (Ton kommt zurück, FEC liefert Ton statt Stille) |
| Verlustausgleich (Paritätsrahmen, Verlusttest) | `audio/src/verlust.rs` | 5 Einzeltests (1 Verlust exakt ersetzt, 5 % Verlust vollständig) |
| Sendekette (Block → Datagramm) | `audio/src/senden.rs` | 5 Einzeltests (Vorlauf beim Öffnen, Folgenummern lückenlos, Gate) |
| Empfang (je Sprecher Dekodierer + Jitter, Mischen) | `audio/src/empfang.rs` | 5 Einzeltests (zwei Sprecher gemischt, Doppelte verworfen) |
| Geräte: cpal (WASAPI/CoreAudio/ALSA); Rückfall auf `parec`/`pacat` **nur unter Linux** | `audio/src/geraet.rs` | 2 Einzeltests, Rückfall im Betrieb geprüft |
| Pfad (Takt, Threads, Stumm/Taub) | `audio/src/lib.rs` | 3 Einzeltests + Messung |
| Fensteranbindung | `desktop/src/sprech.rs` | 5 Einzeltests |

### Messung, 4. November 2026

`scripts/sprechprobe.sh` (zwei Proben am selben Server, 440-Hz-Ton, 3 s):

```
Tondauer im Mitschnitt: 2960 ms (gesendet: 3000 ms)
Frequenzanteile: 220 Hz: 21, 330 Hz: 95, 440 Hz: 7172, 660 Hz: 13, 880 Hz: 7
Spitze: 16114 / 32767          (kein Anschlag, kein Verlust)
Raum 9: laute Blöcke: 0        (Sprache verlässt den Raum nicht)
Sprecher: angenommen 0         (kein Rückkanal an den Sprecher)
Laufzeit durch den Server: p50 1,1 ms · p95 1,7 ms · p99 1,8 ms (n = 150)
```

Die Laufzeit ist der Weg **Client → Server → Client** auf demselben Rechner
(Ankunftszeit minus Sendezeitpunkt, gemeinsame Uhr). Sie enthält QUIC, die
Verteilung im Server und die Entschlüsselung, aber **nicht** die Puffer der
Soundkarte: die kommen im Betrieb dazu — 20 ms Aufnahmeblock, 80 ms
Empfangspuffer (eine Paritätsgruppe) und die Gerätelatenz (40 ms bei `pacat`,
bei cpal je nach Gerät 10–40 ms). Zusammen rund **140 ms** bis zum Ohr, davon
gehen 80 ms auf das Konto des Verlustausgleichs. Wer diese 80 ms nicht will,
schaltet die Paritätsrahmen ab — dann sind es 60 ms, aber ein Einzelverlust
ist hörbar.

Was der Pfad ausdrücklich **nicht** kann (bewusst, nicht vergessen):

1. **Keine Echounterdrückung (AEC).** Wer über Lautsprecher spricht, nimmt die
   anderen mit auf. Kopfhörer oder ein eigener AEC-Baustein.
2. **Kein Spektral-Denoiser.** Die Rauschunterdrückung ist ein Pegel-Expander
   über dem verfolgten Rauschboden (siehe oben) — Lüfter und Grundrauschen ja,
   einzelne Geräusche nur teilweise.
3. **Kein Verlusttest über echtes Netz** (`tc netem`): gemessen wurde auf dem
   gleichen Rechner. Steht als nächster Schritt in `docs/QUALITAET.md`.
4. **Keine Ende-zu-Ende-Verschlüsselung** (ADR 0002/0006) — offen.

## Werkzeug: `scripts/sprechprobe.sh` (Netz und Codec)

Prüft den Weg **ohne Gerät**: startet einen eigenen Server, lässt eine Probe
einen Ton senden, eine zweite mitschneiden und eine dritte in einem anderen
Raum sitzen. Geprüft werden Ankunft, Frequenz, Raumtreue, Rückkanal und
Laufzeit. Rückgabewert 0 = bestanden.

```bash
scripts/sprechprobe.sh            # eigener Port 17443
scripts/sprechprobe.sh 18000      # anderer Port
```

## Werkzeug: `scripts/audio-ende-zu-ende.sh` (der echte Pfad)

Prüft den Weg **mit Geräten**, aber trotzdem auf einem Rechner, ohne Mikrofon
und ohne weitere Personen. PipeWire bekommt vier virtuelle Geräte; dazwischen
liegen zwei echte Endpunkte (`sprechprobe endpunkt`) und der echte Server:

```text
Ton (pacat) ──► sw_a_senke ──(remap)──► sw_a_mikrofon
                                            │  Endpunkt A:
                                            │  parec → RNNoise → Gate
                                            │  → Opus → Vorlauf
                                            ▼
                                    Server (QUIC, Raum)
                                            ▼
                             sw_b_hoerer ◄── pacat ← Jitter-Puffer
                                            │  ◄── Opus ← Endpunkt B
                                  (parec mitschneiden)
```

Kein Teil ist nachgebaut: Aufnahme, Rauschunterdrückung, Noisegate, Opus,
Paritätsrahmen, Server, Jitter-Puffer und Wiedergabe laufen wie im Fenster.
Zwei Läufe:

1. **Reiner Ton 440 Hz** (Entrauschung und Gate aus) — kommt er an, ist es
   derselbe Ton (Goertzel gegen 220/880/1000 Hz), und war vorher Stille?
2. **Stimmähnliches Signal** (120 Hz mit Oberwellen, 4 Hz-Hüllkurve, also
   Silben) **mit** RNNoise und Gate — kommt Sprache durch die Entrauschung?

```bash
scripts/audio-ende-zu-ende.sh        # eigener Port 17444
scripts/audio-ende-zu-ende.sh 18000  # anderer Port
```

Rückgabewert 0 = bestanden. Voraussetzung: PipeWire oder PulseAudio mit
`pactl`, `parec`, `pacat` — also das, was für Ton ohnehin läuft.

Was der Lauf **nicht** sagt: nichts über ein echtes Netz (kein Verlust, kein
Jitter, keine MTU), nichts über Lautsprecher und Mikrofon (die Geräte sind
virtuell) und nichts über das Ohr. Er sagt, dass die Kette als Ganzes Sprache
überträgt und dabei weder verstummt noch den falschen Ton erzeugt.

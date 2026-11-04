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

```bash
# Fedora
sudo dnf install alsa-lib-devel pkgconf-pkg-config cmake
# Debian/Ubuntu
sudo apt install libasound2-dev pkg-config cmake

cargo build --manifest-path crates/audio/Cargo.toml
```

`crates/audio` steht **nicht** im Arbeitsbereich (`crates/audio/Cargo.toml` hat
eine eigene `[workspace]`), damit `cargo test --workspace` ohne Soundkarte
durchläuft.

## Kette

```text
Mikrofon ──► Vorlauf (60 ms Ring) ──► Opus 20 ms/32 kbit/s ──► QUIC-Datagramm
                ▲
        Push-to-Talk: beim Drücken geht der Vorlauf **vor** dem Live-Ton raus

QUIC-Datagramm ──► Jitter-Puffer (60 ms) ──► Opus-Decoder ──► Lautsprecher
```

## Zahlen

| Größe | Wert | Herkunft |
|---|---|---|
| Rahmen | 20 ms | Protokoll (Sprachkopf, Folge je Sprecher) |
| Bitrate | 32 kbit/s mono | Vorgabe; TeamSpeak nutzt je Kanalqualität 6–32 kbit/s |
| Vorlauf | 60 ms (einstellbar 0–200) | ersetzt TeamSpecks `delay_ptt_msecs=300` |
| Jitter-Puffer | 3 Rahmen = 60 ms | Startwert; bei 5 % Verlust messen und anpassen |
| Kopfaufschlag | 4 von 84 Byte = 4,8 % | Sprachkopf Version/Codec/Folge |

## Rauschunterdrückung und Noisegate

Zwei verschiedene Dinge, die oft verwechselt werden:

| | Wirkung | Wo | Einstellung |
|---|---|---|---|
| **Rauschunterdrückung** | filtert Störgeräusche *innerhalb* des Signals (Lüfter, Tastatur, Brummen) | Aufnahmepfad, vor der Kodierung | `rauschunterdrueckung` (Vorgabe: **an**) |
| **Noisegate** | unterhalb der Sprechschwelle wird **gar nichts** gesendet | Aufnahmepfad, vor der Kodierung | `noisegate` (Vorgabe: **an**), Schwelle über `schwelle` |

Umsetzung im Sprachpfad (noch zu bauen, Einstellungen liegen schon):

1. Aufnahme → **Gate**: Pegel unter `schwelle` ⇒ Rahmen verwerfen (nicht leise
   senden, sondern *nicht* senden). Das spart Bandbreite und hält den Raum ruhig.
2. Aufnahme → **Denoiser** (RNNoise o. ä.) auf den 20-ms-Rahmen, dann Opus.
   Reihenfolge ist wichtig: erst entrauschen, dann kodieren — sonst kodiert Opus
   das Rauschen mit.
3. TeamSpeak macht dasselbe (`denoise=true`, `denoiser_level=1`, `vad=true`,
   `voiceactivation_level=-40` in dessen Clientprofil).

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
| Hochpass, Rauschunterdrückung, Noisegate | `adapters/src/klang.rs` | 7 Einzeltests (Rauschen sinkt, Sprache bleibt, Gate mit Hysterese) |
| Vorlauf (60 ms), Jitter-Puffer, Sprechsteuerung | `adapters/src/sprechpfad.rs` | 13 Einzeltests |
| Opus-Kodierung (Bindung an `libopus.so.0`) | `audio/src/opus.rs` | 4 Einzeltests (Ton kommt mit Energie zurück, Rahmen < 1200 Byte) |
| Sendekette (Block → Datagramm) | `audio/src/senden.rs` | 5 Einzeltests (Vorlauf beim Öffnen, Folgenummern lückenlos, Gate) |
| Empfang (je Sprecher Dekodierer + Jitter, Mischen) | `audio/src/empfang.rs` | 5 Einzeltests (zwei Sprecher gemischt, Doppelte verworfen) |
| Gerät (Aufnahme/Wiedergabe über `parec`/`pacat`) | `audio/src/lib.rs` | 3 Einzeltests + Messung |
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
Soundkarte: die kommen im Betrieb dazu (20 ms Aufnahmeblock, 60 ms
Jitter-Puffer, 40 ms `pacat`-Latenz) — zusammen rund 120 ms bis zum Ohr, davon
gehen 60 ms auf das Konto des Jitter-Puffers.

Was der Pfad ausdrücklich **nicht** kann (bewusst, nicht vergessen):

1. **Keine Echounterdrückung (AEC).** Wer über Lautsprecher spricht, nimmt die
   anderen mit auf. Kopfhörer oder ein eigener AEC-Baustein.
2. **Kein Spektral-Denoiser.** Die Rauschunterdrückung ist ein Pegel-Expander
   über dem verfolgten Rauschboden (siehe oben) — Lüfter und Grundrauschen ja,
   einzelne Geräusche nur teilweise.
3. **Kein Verlusttest über echtes Netz** (`tc netem`): gemessen wurde auf dem
   gleichen Rechner. Steht als nächster Schritt in `docs/QUALITAET.md`.
4. **Keine Ende-zu-Ende-Verschlüsselung** (ADR 0002/0006) — offen.

## Werkzeug: `scripts/sprechprobe.sh`

Prüft den ganzen Weg ohne Mikrofon und ohne Kopfhörer: startet einen eigenen
Server, lässt eine Probe einen Ton senden, eine zweite mitschneiden und eine
dritte in einem anderen Raum sitzen. Geprüft werden Ankunft, Frequenz,
Raumtreue, Rückkanal und Laufzeit. Rückgabewert 0 = bestanden.

```bash
scripts/sprechprobe.sh            # eigener Port 17443
scripts/sprechprobe.sh 18000      # anderer Port
```

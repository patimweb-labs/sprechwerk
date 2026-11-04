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

## Was noch fehlt

1. **Gerät und Codec bauen** (siehe oben) und den Pfad in `crates/desktop`
   anschließen: `Audiopfad::starten`, `gesendet` → `Verbindung::sprache_senden`,
   eingehende Datagramme → `zu_spielen`, Pegel → `Sprechsteuerung`.
2. **Pegelanzeige** im Fenster (WpSlider zeigt die Schwelle, ein Balken den Pegel).
3. **Messung**: Ende-zu-Ende-Latenz mit zwei Rechnern, Verlusttest mit
   `tc netem loss 2%`, Vergleich gegen TeamSpeak mit demselben Ton.
4. **Endezuende-Verschlüsselung** (ADR 0002) — erst nach der Messung entscheiden.

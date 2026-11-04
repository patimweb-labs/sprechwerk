# ADR 0003 — QUIC als Transport

**Status:** entschieden (2026-11-04)
**Betrifft:** `crates/adapters`, `crates/server`

## Zusammenhang

Sprache braucht kurze Wege und Verlusttoleranz; Text braucht Zuverlässigkeit.
TeamSpeak löst beides auf einem handgebauten UDP-Protokoll mit eigenen
Fragmenten, Quittungen und eigener Verschlüsselung.

## Entscheidung

QUIC (RFC 9000) mit der Datagramm-Erweiterung (RFC 9221):

* **Steuerung und Text** als langer, bidirektionaler Strom, Rahmen mit 4-Byte-Länge
  und JSON-Nutzlast.
* **Sprache** als Datagramm mit 4-Byte-Kopf (Version, Codec, Folge) und
  Opus-Rahmen. Der Raum steht nicht im Paket, sondern im Sitzungszustand des
  Servers.
* **Staukontrolle:** BBR (Vorgabe), umstellbar auf Cubic.
* **Sendepuffer für Sprache:** klein (64 KiB). Verwerfen statt anstauen.

## Gemessen (2026-11-04, gleiche Maschine, Loopback)

```text
Sprachrahmen durch den Server:  p50 16 µs · p95 21 µs · p99 30 µs
Beitritt (QUIC + Anmeldung):    p50 1 072 µs · p95 1 289 µs
```

Ein Opus-Rahmen kommt alle 20 000 µs. Der Server braucht ein Zweitausendstel
davon. Siehe `docs/QUALITAET.md`.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| TCP/WebSocket | Staukontrolle staut Sprache; ein verlorenes Paket blockiert alles dahinter |
| Roh-UDP mit eigener Krypto | genau der TeamSpeak-Weg: Handshake, Schlüsselwechsel, Wiederholungen selbst bauen |
| WebRTC/SRTP | für Browser richtig, für einen Verteiler ein Vielfaches an Arbeit; bleibt Rückfalllinie |

## Kosten

* QUIC rechnet in Userland: CPU wächst mit dem Fanout (50 Sprecher × 50
  Empfänger ≈ 2 500 Pakete/s, unkritisch).
* UDP kann in manchen Netzen blockiert sein — dann braucht es später einen
  TCP-Weg (WebTransport ist QUIC, hilft dort nicht).
* Die Staukontrolle ist generisch (Cubic/BBR), nicht medientauglich wie bei
  WebRTC. Deshalb: Sprache nie über dieselbe Verbindung wie Dateien.

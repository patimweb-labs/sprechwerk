# ADR 0004 — Audiopfad: Geräte, Opus, Vorlauf, Puffer

**Status:** Logik gebaut und geprüft; Geräteanbindung entworfen (2026-11-04)
**Betrifft:** `crates/adapters/src/sprechpfad.rs`, `crates/audio`, `crates/desktop`

## Zusammenhang

Sprache ist die erste Säule. Der Server verteilt nur — die Qualität entsteht im
Client. TeamSpeak wartet bei Push-to-Talk 300 ms, damit das erste Wort nicht
abgeschnitten wird; das hört man. Wir wollen dieselbe Sicherheit ohne die
Wartezeit.

## Entscheidung

### Kette

```text
Gerät (beliebige Rate, n Kanäle)
  → Heruntermischen auf mono, Umrechnung auf 48 kHz
  → 20-ms-Rahmen (960 Abtastwerte)
  → Tor: Push-to-Talk gedrückt ODER Pegel ≥ Schwelle
  → Vorlauf: beim Drücken die letzten 60 ms **vor** dem Live-Ton
  → Opus (32 kbit/s, DTX, In-Band-FEC)
  → QUIC-Datagramm, 4 Byte Kopf

umgekehrt: Datagramm → Jitter-Puffer (3 Rahmen = 60 ms) → Opus (PLC bei
Verlust) → Umrechnung auf Geräterate → Ausgabe
```

### Trennung von Logik und Gerät

* `crates/adapters/src/sprechpfad.rs` — **Vorlauf, Jitter-Puffer,
  Sprechtaste, Schwelle.** Kein Gerät, keine Uhr, kein Netz → 13 Tests, laufen
  in `cargo test --workspace`.
* `crates/audio` — ALSA/PipeWire über `cpal`, Opus über `audiopus`. Eigenes
  Crate, eigene `[workspace]`, weil es Systempakete braucht (`alsa-lib-devel`,
  `cmake`). Wird getrennt gebaut.

Begründung: die Fehler sitzen in der Reihenfolge (zu spät, doppelt, Lücke,
Vorlauf zweimal), nicht im `write()` ans Gerät. Diese Fehler sollen ohne
Soundkarte auffallen.

### Regeln

1. **Vorlauf einmalig.** Nach dem Abholen ist der Ring leer; er füllt sich erst
   wieder, wenn die Taste losgelassen wurde — sonst ginge derselbe Ton doppelt
   hinaus.
2. **Erwartete Folgenummer = niedrigster anliegender Rahmen**, nicht der erste
   ankommende. (Gefunden durch Test: sonst gilt ein früh angekommener höherer
   Rahmen als Anfang und alles Niedrigere wäre „zu spät".)
3. **Doppelte Rahmen verwerfen, ohne den Inhalt zu ersetzen.** (`BTreeMap::insert`
   überschreibt — gefunden durch Test.)
4. **Aufhören spült den Puffer.** Die letzten Rahmen eines Wortbeitrags dürfen
   nicht verfallen, nur weil die Fülltiefe nicht mehr erreicht wird.
5. **Kein Blockieren im Geräte-Callback.** `cpal` ruft aus einem eigenen Thread
   mit harter Frist; der Callback schiebt nur in Kanäle.

### Latenzbudget

| Abschnitt | Wert |
|---|---|
| Rahmenbildung | 20 ms |
| Kodieren | 1–3 ms |
| Server | 16 µs (p50) / 30 µs (p99), gemessen |
| Jitter-Puffer | 60 ms (Startwert) |
| Dekodieren | 1–3 ms |
| Ausgabe | 20 ms |
| **Summe (LAN)** | **≈ 110 ms** |

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Kein Vorlauf, sofort senden | das erste Wort wird abgeschnitten — der Grund für TeamSpecks 300 ms |
| 300 ms Verzögerung wie TeamSpeak | hörbar, ohne Gewinn |
| Größerer Jitter-Puffer (200 ms) | sicherer, aber träge; 60 ms Startwert, bei Verlustmessung anpassen |
| Eigener Codec | Opus ist frei, gut und überall hardwarebeschleunigt |

## Kosten und offene Punkte

* `crates/audio` ist hier **nicht gebaut** (Systempakete fehlen) — ehrlich
  benannt, nicht als fertig ausgegeben.
* Anbindung ans Fenster (Pegel → `Sprechsteuerung`, `gesendet` →
  `sprache_senden`, Empfang → `zu_spielen`) fehlt noch.
* Der Callback allokiert derzeit (`vec![0u8; 4000]` je Rahmen) — für 50
  Rahmen/s unkritisch, für 48 kHz-Kanäle mit vielen Sprechern nicht. Beim
  Anschließen durch einen vorab reservierten Puffer ersetzen.
* Uhrdrift zwischen Gerät und 20-ms-Takt: heute über Rahmenzählung gelöst;
  bei Dauerbetrieb adaptive Umrechnung nötig.

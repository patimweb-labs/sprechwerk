#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Sprache Ende zu Ende prüfen — auf **einem** Rechner, ohne Mikrofon, ohne
# Kopfhörer und ohne weitere Personen.
#
# # Der Aufbau
#
# PipeWire bekommt vier virtuelle Geräte; dazwischen liegen zwei echte
# Endpunkte (`sprechprobe endpunkt`) und der echte Server:
#
# ```text
#   Ton (pacat) ──► sw_a_senke ──(remap)──► sw_a_mikrofon
#                                               │  echter Pfad A:
#                                               │  parec → RNNoise → Gate
#                                               │  → Opus → Vorlauf
#                                               ▼
#                                       Server (QUIC, Raum)
#                                               ▼
#                                sw_b_hoerer ◄── pacat ← Jitter-Puffer
#                                               │  ← Opus ← echter Pfad B
#                                     (parec mitschneiden)
# ```
#
# Geprüft wird also **der ganze Weg**: Geräte, Entrauschung, Gate, Opus,
# Paritätsrahmen, Server, Jitter-Puffer, Wiedergabe. Kein Teil ist nachgebaut.
#
# # Zwei Läufe
#
# 1. **Reiner Ton** (440 Hz, Entrauschung und Gate aus): kommt der Ton an, ist
#    er derselbe, und wie viel Stille steht davor?
# 2. **Stimme** (Grundton mit Obertönen, silbenweise moduliert, Entrauschung
#    und Gate **an**): kommt Sprache durch RNNoise und das Gate?
#
# Aufruf:  scripts/audio-ende-zu-ende.sh [port]
# Ergebnis: exit 0 = bestanden, 1 = Befund (die Zeilen nennen ihn).

set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${PORT:-17444}"
DATEN="$(mktemp -d)"
AUSWERTUNG="$WURZEL/scripts/audio-ende-zu-ende.py"
PROBE="$WURZEL/target/debug/sprechprobe"
SERVER="$WURZEL/target/debug/sprechwerk"
# Prüfserver ohne Verwaltung im Browser (kein Port 7444, kein Browserfenster).
export SPRECHWERK_VERWALTUNG_AKTIV=0
DAUER=9

MODULE=()
PIDS=()

aufraeumen() {
  for pid in "${PIDS[@]:-}"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
  done
  for modul in "${MODULE[@]:-}"; do
    [[ -n "$modul" ]] && pactl unload-module "$modul" 2>/dev/null || true
  done
  rm -rf "$DATEN"
}
trap aufraeumen EXIT

senke() {
  local id
  id=$(pactl load-module module-null-sink sink_name="$1" \
    sink_properties=device.description="$1") || {
    echo "FEHLER: PipeWire/PulseAudio nicht erreichbar (module-null-sink)" >&2
    exit 1
  }
  MODULE+=("$id")
}

mikrofon() {
  local id
  id=$(pactl load-module module-remap-source master="$1.monitor" source_name="$2" \
    source_properties=device.description="$2")
  MODULE+=("$id")
}

echo "== Bauen"
cargo build -q --manifest-path "$WURZEL/Cargo.toml" \
  -p sprechwerk-server -p sprechwerk-audio 2>&1 | tail -2

echo "== Virtuelle Geräte"
senke sw_a_senke
mikrofon sw_a_senke sw_a_mikrofon
senke sw_a_ausgang
senke sw_b_senke
mikrofon sw_b_senke sw_b_mikrofon
senke sw_b_hoerer
sleep 0.5

echo "== Server (Port $PORT)"
SPRECHWERK_DATEN="$DATEN" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  "$SERVER" >"$DATEN/server.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 1 50); do
  grep -q lauscht "$DATEN/server.log" 2>/dev/null && break
  sleep 0.1
done
ADR="127.0.0.1:$PORT"

# Ein Lauf: beide Endpunkte verbinden, ein Signal in A's Mikrofon, Mitschnitt
# an B's Wiedergabe.
#
# $1 Name des Laufs, $2 Signal (ton|stimme), $3 Entrauschen 0|1, $4 Gate 0|1
ein_lauf() {
  local name="$1" signal="$2" entrauschen="$3" gate="$4"
  local a b rec

  parec --device=sw_b_hoerer.monitor --format=s16le --rate=48000 --channels=1 \
    --raw --latency-msec=20 >"$DATEN/$name.pcm" 2>/dev/null &
  rec=$!
  PIDS+=($rec)

  "$PROBE" endpunkt --server "$ADR" --name "A-$name" --raum 1 --dauer "$DAUER" \
    --mikrofon sw_a_mikrofon --lautsprecher sw_a_ausgang \
    --entrauschen "$entrauschen" --gate "$gate" \
    --schluessel "$DATEN/ka-$name" >"$DATEN/$name-a.log" 2>&1 &
  a=$!
  PIDS+=($a)

  "$PROBE" endpunkt --server "$ADR" --name "B-$name" --raum 1 --dauer "$DAUER" \
    --mikrofon sw_b_mikrofon --lautsprecher sw_b_hoerer \
    --entrauschen "$entrauschen" --gate "$gate" \
    --schluessel "$DATEN/kb-$name" >"$DATEN/$name-b.log" 2>&1 &
  b=$!
  PIDS+=($b)

  # Ankommen lassen: Verbindung, Raum betreten, Geräte offen.
  sleep 2.5

  "$AUSWERTUNG" signal --art "$signal" --sekunden 3 |
    pacat --playback --device=sw_a_senke --format=s16le --rate=48000 --channels=1 \
      --raw 2>/dev/null || true

  wait "$a" "$b" 2>/dev/null || true
  sleep 0.3
  kill "$rec" 2>/dev/null || true
  wait "$rec" 2>/dev/null || true
}

echo "== Lauf 1: reiner Ton 440 Hz (ohne Entrauschung, ohne Gate)"
ein_lauf ton ton 0 0
echo "== Lauf 2: Stimme mit Rauschunterdrückung und Gate"
ein_lauf stimme stimme 1 1

echo
echo "== Auswertung"
"$AUSWERTUNG" ergebnis \
  --ton "$DATEN/ton.pcm" \
  --stimme "$DATEN/stimme.pcm" \
  --hz 440

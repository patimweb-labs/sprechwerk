#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Prüft den Sprachpfad durch den Server — ohne Mikrofon und ohne Kopfhörer.
#
# Zwei Proben treten demselben Raum bei: die eine sendet einen 440-Hz-Ton, die
# andere schneidet mit. Geprüft wird:
#   1. Der Ton kommt an (Energie im Mitschnitt).
#   2. Es ist derselbe Ton (440 Hz, nicht 220 oder 880).
#   3. Er bleibt im Raum (eine Probe in einem anderen Raum bekommt nichts).
#   4. Der Sprecher hört sich nicht selbst (kein Rückkanal).
#   5. Die Laufzeit durch den Server (Ankunft minus Sendezeitpunkt).
#
# Aufruf:  scripts/sprechprobe.sh [port]
# Ergebnis: exit 0 = alles in Ordnung, sonst mit Begründung.

set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${1:-17443}"
DATEN="$(mktemp -d)"
PROBE="$WURZEL/target/debug/sprechprobe"
SERVER="$WURZEL/target/debug/sprechwerk"
RAUM_MS=3000

aufraeumen() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$DATEN"
}
trap aufraeumen EXIT

echo "== Bauen"
cargo build -q --manifest-path "$WURZEL/Cargo.toml" -p sprechwerk-server -p sprechwerk-audio 2>&1 | tail -2

echo "== Server starten (Port $PORT, Daten in $DATEN)"
SPRECHWERK_DATEN="$DATEN" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  "$SERVER" >"$DATEN/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 50); do
  grep -q lauscht "$DATEN/server.log" 2>/dev/null && break
  sleep 0.1
done

ADR="127.0.0.1:$PORT"

# Reihenfolge zählt: der **erste** Angemeldete wird Verwalter und darf den
# zweiten Raum anlegen. Deshalb tritt die Raumprobe zuerst bei.
echo "== Probe 1: legt einen zweiten Raum an und sitzt dort (darf nichts hören)"
"$PROBE" mitschnitt --server "$ADR" --name Fremder --neuer-raum "Nebenraum" --dauer 8 \
  --wav "$DATEN/raum9.wav" >"$DATEN/hoerer2.log" 2>&1 &
HOERER2_PID=$!
sleep 1.5

echo "== Probe 2: sendet 3 s einen 440-Hz-Ton in Raum 1"
"$PROBE" ton --server "$ADR" --name Sender --raum 1 --dauer 3 --hz 440 \
  >"$DATEN/sender.log" 2>&1 &
SENDER_PID=$!

echo "== Probe 3: schneidet in Raum 1 mit"
"$PROBE" mitschnitt --server "$ADR" --name Hoerer --raum 1 --dauer 6 \
  --wav "$DATEN/raum1.wav" >"$DATEN/hoerer1.log" 2>&1 &
HOERER1_PID=$!

wait "$HOERER2_PID" || true
wait "$SENDER_PID" || true
wait "$HOERER1_PID" || true

echo
echo "== Auswertung"
python3 "$WURZEL/scripts/sprechprobe.py" "$DATEN"

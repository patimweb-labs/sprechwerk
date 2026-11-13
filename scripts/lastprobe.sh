#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Lastprobe: **mehrere Sprecher gleichzeitig** in einem Raum, mehrere Hörer,
# dazu ein Fremder in einem anderen Raum.
#
# Die bestehende `sprechprobe.sh` prüft einen Sprecher und einen Hörer — genug
# für „der Pfad tut das Richtige". Hier geht es um das, was im Betrieb zählt:
#
#   1. Jeder Hörer hört **alle** Sprecher (vier Töne, sauber getrennt).
#   2. Kein Sprecher geht im anderen unter (kein Vertauschen, kein Mischen).
#   3. Der Fremde im Nachbarraum hört **nichts**.
#   4. Der Server verwirft nichts (`verworfen` in der Messzeile).
#
# Aufruf:  scripts/lastprobe.sh [port]
# Ergebnis: exit 0 = alles in Ordnung.

set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${1:-17444}"
DATEN="$(mktemp -d)"
SERVER="$WURZEL/target/debug/sprechwerk"
PROBE="$WURZEL/target/debug/sprechprobe"

# Vier Sprecher mit verschiedenen Tönen: so lässt sich im Mitschnitt
# nachweisen, dass jeder bei jedem ankommt.
FREQUENZEN="${FREQUENZEN:-300 400 500 600}"
HOERER="${HOERER:-3}"
TONDAUER=4
HOERDAUER=6

aufraeumen() {
  [[ -n "${SERVER_PID:-}" ]] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$DATEN"
}
trap aufraeumen EXIT

echo "== Bauen"
cargo build -q --manifest-path "$WURZEL/Cargo.toml" -p sprechwerk-server -p sprechwerk-audio 2>&1 | tail -2

echo "== Server starten (Port $PORT, Zähler alle 10 s)"
SPRECHWERK_DATEN="$DATEN" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  SPRECHWERK_MESSUNG=1 "$SERVER" >"$DATEN/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 50); do
  grep -q lauscht "$DATEN/server.log" 2>/dev/null && break
  sleep 0.1
done

ADR="127.0.0.1:$PORT"

# Der erste Angemeldete wird Verwalter und legt den Raum an.
echo "== Vorbereiter: legt „Last“ an und hört mit"
"$PROBE" mitschnitt --server "$ADR" --name Vorbereiter --neuer-raum Last \
  --dauer "$((HOERDAUER + 3))" --wav "$DATEN/hoerer0.wav" >"$DATEN/vorbereiter.log" 2>&1 &
VORBEREITER_PID=$!
sleep 1.5

# Der Fremde bekommt **keinen** Raum genannt: er landet im Wurzelraum. Die
# Sprecher sind in „Last“ (Raum 2) — er darf nichts davon hören.
echo "== Fremder: Wurzelraum, darf nichts hören"
"$PROBE" mitschnitt --server "$ADR" --name Fremder \
  --dauer "$((HOERDAUER + 2))" --wav "$DATEN/fremd.wav" >"$DATEN/fremd.log" 2>&1 &
FREMD_PID=$!
sleep 0.5

echo "== Hörer: $HOERER Proben im Raum „Last“"
HOERER_PIDS=()
for i in $(seq 1 "$HOERER"); do
  "$PROBE" mitschnitt --server "$ADR" --name "Hoerer$i" --raum 2 --dauer "$HOERDAUER" \
    --wav "$DATEN/hoerer$i.wav" >"$DATEN/hoerer$i.log" 2>&1 &
  HOERER_PIDS+=("$!")
done
sleep 0.8

echo "== Sprecher: $(echo "$FREQUENZEN" | wc -w) gleichzeitig ($FREQUENZEN Hz)"
SPRECHER_PIDS=()
for hz in $FREQUENZEN; do
  "$PROBE" ton --server "$ADR" --name "Ton$hz" --raum 2 --dauer "$TONDAUER" --hz "$hz" \
    >"$DATEN/ton$hz.log" 2>&1 &
  SPRECHER_PIDS+=("$!")
done

for pid in "${SPRECHER_PIDS[@]}" "${HOERER_PIDS[@]}"; do wait "$pid" || true; done
wait "$VORBEREITER_PID" || true
wait "$FREMD_PID" || true

# Die Zählermeldung des Servers kommt alle 10 s — so lange warten wir.
while ! grep -q "Sprachrahmen" "$DATEN/server.log" 2>/dev/null; do
  sleep 0.5
done 2>/dev/null || true
sleep 0.5

echo
echo "== Serverseite"
grep -E "Sprachrahmen" "$DATEN/server.log" | tail -1 | sed 's/^\s*//' || true

echo
echo "== Auswertung"
FREQUENZEN="$FREQUENZEN" python3 "$WURZEL/scripts/lastprobe.py" "$DATEN"

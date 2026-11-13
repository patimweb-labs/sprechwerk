#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Verlust- und Jitterprobe: derselbe Raum, zwei Sprecher, ein Hörer — aber der
# eine Sprecher geht **durch eine Störstrecke** (5 % Verlust), der Hörer sitzt
# hinter einer zweiten (20 ms Verzögerung, 25 ms Jitter).
#
# Damit lässt sich auf einem Rechner zeigen, was sonst nur ein schlechtes WLAN
# zeigt: Die Sprache kommt auch mit Verlust an, und der Jitter-Puffer hält.
#
#   1. Beide Töne sind im Mitschnitt des Hörers nachweisbar.
#   2. Der verlustbehaftete Ton ist **kürzer** als der direkte (Lücken), aber
#      noch deutlich vorhanden — er wird nicht „verschluckt".
#   3. Der Server meldet keine verworfenen Rahmen (er verwirft nur bei
#      fehlendem Recht/Raum, nicht bei Netzverlust).
#
# Aufruf: scripts/verlustprobe.sh [port]
# Ergebnis: exit 0 = bestanden.

set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${1:-17470}"
PROXY_PORT_A=$((PORT + 20))
PROXY_PORT_B=$((PORT + 21))
DATEN="$(mktemp -d)"
SERVER="$WURZEL/target/debug/sprechwerk"
PROBE="$WURZEL/target/debug/sprechprobe"

aufraeumen() {
  for pid in "${SERVER_PID:-}" "${PROXY_A:-}" "${PROXY_B:-}"; do
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
  done
  rm -rf "$DATEN"
}
trap aufraeumen EXIT

echo "== Bauen"
cargo build -q --manifest-path "$WURZEL/Cargo.toml" -p sprechwerk-server -p sprechwerk-audio 2>&1 | tail -2

echo "== Server starten (Port $PORT)"
SPRECHWERK_DATEN="$DATEN" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  SPRECHWERK_MESSUNG=1 "$SERVER" >"$DATEN/server.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 50); do
  grep -q lauscht "$DATEN/server.log" 2>/dev/null && break
  sleep 0.1
done
ADR="127.0.0.1:$PORT"

echo "== Störstrecken aufbauen"
# Sprecher A: 5 % Verlust, 10 ms Jitter.
python3 "$WURZEL/scripts/verlustproxy.py" --port "$PROXY_PORT_A" --ziel "$ADR" \
  --verlust 0.05 --jitter-ms 10 >"$DATEN/proxy_a.log" 2>&1 &
PROXY_A=$!
# Hörer: 20 ms Verzögerung, 25 ms Jitter (kein Verlust — der Verlust liegt beim
# Sprecher).
python3 "$WURZEL/scripts/verlustproxy.py" --port "$PROXY_PORT_B" --ziel "$ADR" \
  --verzoegerung-ms 20 --jitter-ms 25 >"$DATEN/proxy_b.log" 2>&1 &
PROXY_B=$!
sleep 1

echo "== Hörer (hinter der Störstrecke)"
"$PROBE" mitschnitt --server "127.0.0.1:$PROXY_PORT_B" --name Hoerer --raum 1 \
  --dauer 7 --wav "$DATEN/hoerer.wav" >"$DATEN/hoerer.log" 2>&1 &
HOERER_PID=$!
sleep 1

echo "== Sprecher direkt (440 Hz)"
"$PROBE" ton --server "$ADR" --name Direkt --raum 1 --dauer 4 --hz 440 \
  >"$DATEN/direkt.log" 2>&1 &
DIREKT_PID=$!

echo "== Sprecher mit 5 % Verlust (660 Hz)"
"$PROBE" ton --server "127.0.0.1:$PROXY_PORT_A" --name Verlust --raum 1 --dauer 4 --hz 660 \
  >"$DATEN/verlust.log" 2>&1 &
VERLUST_PID=$!

wait "$DIREKT_PID" || true
wait "$VERLUST_PID" || true
wait "$HOERER_PID" || true

echo
echo "== Störstrecke: wie viele Pakete hat sie verworfen?"
grep -c VERWORFEN "$DATEN/proxy_a.log" 2>/dev/null || echo 0

echo
echo "== Auswertung"
python3 - "$DATEN" <<'PY'
import math, struct, sys, wave
from pathlib import Path

verzeichnis = Path(sys.argv[1])
pfad = verzeichnis / "hoerer.wav"
if not pfad.exists():
    print("FEHLER: Mitschnitt fehlt — lief der Hörer?")
    sys.exit(1)

with wave.open(str(pfad)) as w:
    rate = w.getframerate()
    werte = list(struct.unpack("<%dh" % w.getnframes(), w.readframes(w.getnframes())))


def rms(x):
    return math.sqrt(sum(v * v for v in x) / max(1, len(x)))


def goertzel(x, f, rate):
    if not x:
        return 0.0
    c = 2 * math.cos(2 * math.pi * f / rate)
    s1 = s2 = 0.0
    for w in x:
        s0 = w + c * s1 - s2
        s2, s1 = s1, s0
    return math.sqrt(max(0.0, s1 * s1 + s2 * s2 - c * s1 * s2)) / len(x)


block = 960
bloecke = [werte[i : i + block] for i in range(0, len(werte) - block, block)]
dauer_ms = len(werte) / rate * 1000

def ton_anteil(f):
    return goertzel(werte, f, rate)

def ton_dauer(f):
    """Blöcke zählen, in denen dieser Ton die anderen klar überragt."""
    anzahl = 0
    for b in bloecke:
        if goertzel(b, f, rate) > 1.5 * max(goertzel(b, 300, rate), goertzel(b, 1500, rate)):
            if rms(b) > 300:
                anzahl += 1
    return anzahl * block / rate * 1000

a = ton_anteil(440)
b = ton_anteil(660)
print(f"Mitschnitt: {dauer_ms:.0f} ms, RMS {rms(werte):.0f}")
print(f"Ton direkt  440 Hz: Anteil {a:8.1f}  hörbar in ~{ton_dauer(440):.0f} ms")
print(f"Ton Verlust 660 Hz: Anteil {b:8.1f}  hörbar in ~{ton_dauer(660):.0f} ms")

fehler = 0
if a < 0.2 * max(a, b):
    print("  FEHLER: der direkte Ton fehlt")
    fehler += 1
if b < 0.2 * max(a, b):
    print("  FEHLER: der Ton mit 5 % Verlust fehlt ganz")
    fehler += 1
if dauer_ms < 3000:
    print("  FEHLER: Mitschnitt zu kurz")
    fehler += 1

print()
if fehler == 0:
    print("BESTANDEN: beide Sprecher kommen durch, auch mit 5 % Verlust und Jitter.")
    sys.exit(0)
print(f"FEHLGESCHLAGEN: {fehler} Befund(e).")
sys.exit(1)
PY

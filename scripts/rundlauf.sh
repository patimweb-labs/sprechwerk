#!/usr/bin/env bash
# Rundlauf von Hand: Server starten, zwei Clients, prüfen, aufräumen.
set -eu
PORT=${PORT:-7443}
DIR=$(mktemp -d)
trap 'kill "${PID:-}" 2>/dev/null || true; rm -rf "$DIR"' EXIT

# **Gefunden 2026-11-19:** ohne diesen Bau bricht das Skript auf einem frischen
# Arbeitsverzeichnis ab — die Binärdateien fehlen. Beides bauen, nicht nur den
# Server.
cargo build -p sprechwerk-server -p sprechwerk-cli

SPRECHWERK_DATEN="$DIR" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  SPRECHWERK_VERWALTUNG_AKTIV=0 \
  ./target/debug/sprechwerk > "$DIR/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 50); do grep -q "lauscht auf" "$DIR/server.log" && break; sleep 0.1; done

echo "== Server =="
sed -n '1,6p' "$DIR/server.log"

# Ende zu Ende (ADR 0012): Ada legt den Serverschlüssel an und bleibt online,
# damit Bosse ihn von ihr bekommt — ohne ein Mitglied, das ihn hat, kann
# niemand Neues lesen oder schreiben. Der Server sieht nur Geheimtext.
echo "== Client 1 (wird Administrator, legt den Serverschlüssel an) =="
(sleep 1; echo "Guten Abend"; sleep 4; echo /ende) |
  ./target/debug/sprechwerk-cli --server "127.0.0.1:$PORT" --name Ada \
    --schluessel "$DIR/k1" --raum 1 > "$DIR/ada.log" 2>&1 &
ADA=$!
sleep 2

echo "== Client 2 (bekommt den Schlüssel von Ada) =="
./target/debug/sprechwerk-cli --server "127.0.0.1:$PORT" --name Bosse \
  --schluessel "$DIR/k2" --raum 1 --einmal "Hallo Ada"

wait "$ADA"
echo "== Was Ada gesehen hat =="
cat "$DIR/ada.log"
grep -q "Bosse: Hallo Ada" "$DIR/ada.log" || { echo "FEHLER: Ada hat Bosse nicht gelesen"; exit 1; }

echo "== In der Datenbank steht kein Klartext =="
if grep -rqa "Hallo Ada\|Guten Abend" "$DIR"/sprechwerk.sqlitedb*; then
  echo "FEHLER: Klartext in der Datenbank"; exit 1
fi
echo "ok"

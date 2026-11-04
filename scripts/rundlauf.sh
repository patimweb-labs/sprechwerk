#!/usr/bin/env bash
# Rundlauf von Hand: Server starten, zwei Clients, prüfen, aufräumen.
set -eu
PORT=${PORT:-7443}
DIR=$(mktemp -d)
trap 'kill "${PID:-}" 2>/dev/null || true; rm -rf "$DIR"' EXIT

SPRECHWERK_DATEN="$DIR" SPRECHWERK_PORT="$PORT" SPRECHWERK_BIND=127.0.0.1 \
  ./target/debug/sprechwerk > "$DIR/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 50); do grep -q "lauscht auf" "$DIR/server.log" && break; sleep 0.1; done

echo "== Server =="
sed -n '1,6p' "$DIR/server.log"

echo "== Client 1 (wird Administrator) =="
./target/debug/sprechwerk-cli --server "127.0.0.1:$PORT" --name Ada \
  --schluessel "$DIR/k1" --raum 1 --einmal "Guten Abend"

echo "== Client 2 =="
./target/debug/sprechwerk-cli --server "127.0.0.1:$PORT" --name Bosse \
  --schluessel "$DIR/k2" --raum 1 --einmal "Hallo Ada"

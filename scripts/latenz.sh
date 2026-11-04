#!/usr/bin/env bash
# Latenz messen. Braucht keinen Root, keinen Server, keine Konfiguration:
# startet einen eigenen Server auf einem freien Port.
set -eu
echo "== Sprachrahmen durch den Server und Beitrittszeit =="
cargo test --release --test latenz -- --ignored --nocapture 2>&1 | grep -vE "^\s*(Compiling|Finished|Running|warning)" || true

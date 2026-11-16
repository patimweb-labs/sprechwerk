#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Alles prüfen — mit einem Aufruf.
#
# Warum es dieses Skript gibt: `cargo test` im Wurzelverzeichnis sammelt die
# Tests von `crates/desktop` **nicht** ein (Tauri hat eine eigene Sperrdatei,
# das Crate steht außerhalb des Arbeitsbereichs). Diese Prüfung lief deshalb
# jahrelang niemand — und die Oberfläche ist genau der Teil, den der Nutzer
# beurteilt.
#
#   scripts/pruefen.sh              Tests + Ansichten + Sprachproben
#   scripts/pruefen.sh --mit-audio  zusätzlich der echte Gerätepfad
#   scripts/pruefen.sh --nur-tests  schnell: nur kompilieren und testen
#
# Rückgabewert 0 = alles bestanden.

set -uo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
cd "$WURZEL"

MIT_AUDIO=0
NUR_TESTS=0
for arg in "$@"; do
  case "$arg" in
    --mit-audio) MIT_AUDIO=1 ;;
    --nur-tests) NUR_TESTS=1 ;;
    -h|--hilfe) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unbekannte Option: $arg" >&2; exit 2 ;;
  esac
done

BEFUNDE=0
berichte() {
  local name="$1" ergebnis="$2"
  if [[ "$ergebnis" -eq 0 ]]; then
    printf '  %-34s bestanden\n' "$name"
  else
    printf '  %-34s FEHLGESCHLAGEN\n' "$name"
    BEFUNDE=$((BEFUNDE + 1))
  fi
}

echo "== Versionen und Release-Konfiguration"
python3 scripts/versionen.py >/tmp/sprechwerk-pruefung-versionen.log 2>&1
berichte "scripts/versionen.py" "$?"

echo
echo "== Arbeitsbereich: Form, Werkzeuge, Tests"
cargo fmt --all --check >/dev/null 2>&1
berichte "cargo fmt --check" "$?"
cargo clippy --all-targets --quiet -- -D warnings >/tmp/sprechwerk-pruefung-clippy.log 2>&1
berichte "cargo clippy -D warnings" "$?"
cargo test --quiet 2>&1 | tail -3
berichte "cargo test (Arbeitsbereich)" "${PIPESTATUS[0]}"

echo
echo "== Fenster: Form, Werkzeuge, Tests (eigenes Crate, eigene Sperrdatei)"
if [[ -d crates/desktop ]]; then
  ( cd crates/desktop && cargo fmt --check >/dev/null 2>&1 )
  berichte "cargo fmt --check (desktop)" "$?"
  ( cd crates/desktop && cargo clippy --all-targets --quiet -- -D warnings \
      >/tmp/sprechwerk-pruefung-clippy-desktop.log 2>&1 )
  berichte "cargo clippy (desktop)" "$?"
  ( cd crates/desktop && cargo test --quiet 2>&1 | tail -3 )
  berichte "cargo test (crates/desktop)" "$?"
else
  echo "  crates/desktop fehlt"
  berichte "cargo test (crates/desktop)" 1
fi

echo
echo "== Oberfläche: Typen und Ansichten"
if [[ -d ui/node_modules ]]; then
  ( cd ui && npm run --silent typecheck >/dev/null 2>&1 )
  berichte "vue-tsc (ui)" "$?"
  ( cd ui && npm test --silent >/tmp/sprechwerk-pruefung-ui.log 2>&1 )
  berichte "vitest (ui, Ansichten)" "$?"
  if [[ "$NUR_TESTS" -eq 0 ]]; then
    # Echter Browser: Stile, Sichtbarkeit, alle drei Themes (docs/bilder/).
    ( cd ui && npm run --silent bilder >/tmp/sprechwerk-pruefung-bilder.log 2>&1 )
    berichte "Ansichten im Browser (3 Themes)" "$?"
  fi
else
  echo "  ui/node_modules fehlt — nicht geprüft (vorher 'npm install' in ui/)"
  berichte "vue-tsc (ui) [nicht geprüft]" 1
  berichte "vitest (ui) [nicht geprüft]" 1
fi

if [[ "$NUR_TESTS" -eq 1 ]]; then
  echo
  [[ "$BEFUNDE" -eq 0 ]] && { echo "Alles bestanden."; exit 0; }
  echo "$BEFUNDE Prüfung(en) fehlgeschlagen."
  exit 1
fi

echo
echo "== Sprache durch den Server"
./scripts/sprechprobe.sh >/tmp/sprechwerk-pruefung-sprechprobe.log 2>&1
berichte "scripts/sprechprobe.sh" "$?"

echo
echo "== Vier Sprecher gleichzeitig"
./scripts/lastprobe.sh >/tmp/sprechwerk-pruefung-lastprobe.log 2>&1
berichte "scripts/lastprobe.sh" "$?"

echo
echo "== Verlust und Jitter"
./scripts/verlustprobe.sh >/tmp/sprechwerk-pruefung-verlust.log 2>&1
berichte "scripts/verlustprobe.sh" "$?"

if [[ "$MIT_AUDIO" -eq 1 ]]; then
  echo
  echo "== Echter Gerätepfad (dauert eine Minute)"
  ./scripts/audio-ende-zu-ende.sh >/tmp/sprechwerk-pruefung-audio.log 2>&1
  berichte "scripts/audio-ende-zu-ende.sh" "$?"
else
  echo
  echo "  (Gerätepfad übersprungen — mit --mit-audio prüfen)"
fi

echo
if [[ "$BEFUNDE" -eq 0 ]]; then
  echo "Alles bestanden. Protokolle: /tmp/sprechwerk-pruefung-*.log"
  exit 0
fi
echo "$BEFUNDE Prüfung(en) fehlgeschlagen. Protokolle: /tmp/sprechwerk-pruefung-*.log"
exit 1

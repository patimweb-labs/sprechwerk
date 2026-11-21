#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Marke bauen: aus dem Raster in `weppelmann-ui/apps/markenwerk` die Dateien
# erzeugen, die das Paket braucht — PNG in den Desktop-Größen, SVG für
# hicolor, ICO für Windows, ICNS für macOS und das Logo für README und docs.
#
#   scripts/marke-bauen.sh [pfad/zu/apps/markenwerk]
#
# Ohne Pfad wird `../weppelmann-ui/apps/markenwerk` angenommen (Nachbarordner).
#
# Hier wird **nichts gezeichnet**. Die Quelle ist `marken/sprechwerk.pm`
# (Raster) und `src/schrift.ts` (Wortmarke); alles andere ist abgeleitet, siehe
# AGENTS.md §8 und weppelmann-ui/docs/marke/icons.md.

set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
MARKENWERK="${1:-$WURZEL/../weppelmann-ui/apps/markenwerk}"
ZIEL="$WURZEL/packaging/icons"
DIST="$MARKENWERK/dist/sprechwerk"

if [ ! -f "$MARKENWERK/marken/sprechwerk.pm" ]; then
  echo "markenwerk nicht gefunden: $MARKENWERK" >&2
  echo "Aufruf: scripts/marke-bauen.sh [pfad/zu/apps/markenwerk]" >&2
  exit 1
fi

echo "== Raster rendern =="
(cd "$MARKENWERK" && node src/cli.ts check >/dev/null && node src/cli.ts render sprechwerk \
  && node src/cli.ts logo sprechwerk)

echo "== nach packaging/icons kopieren =="
mkdir -p "$ZIEL/hicolor/scalable/apps" "$WURZEL/docs/marke"
for groesse in 32 128 256 512; do
  install -Dm644 "$DIST/icon-$groesse.png" "$ZIEL/icon-$groesse.png"
  install -Dm644 "$DIST/icon-$groesse.png" "$ZIEL/hicolor/${groesse}x${groesse}/apps/sprechwerk.png"
done
install -Dm644 "$DIST/icon.svg" "$ZIEL/hicolor/scalable/apps/sprechwerk.svg"

echo "== ICO und ICNS =="
# Beides sind Behälter aus Typ und PNG-Nutzlast; wir schreiben sie von Hand,
# weil ImageMagick ICNS nicht schreiben und ICO nur ungepackt kann.
python3 - "$DIST" "$ZIEL/icon.ico" <<'PY'
import struct
import sys
from pathlib import Path

quelle, ziel = Path(sys.argv[1]), Path(sys.argv[2])
# Windows liest seit Vista PNG-Einträge; sie sind ein Bruchteil der BMP-Größe.
groessen = [16, 24, 32, 48, 64, 256]
bilder = [(g, (quelle / f"icon-{g}.png").read_bytes()) for g in groessen]
kopf = struct.pack("<HHH", 0, 1, len(bilder))
offset = 6 + 16 * len(bilder)
eintraege = b""
for groesse, daten in bilder:
    # 256 wird als 0 eingetragen: das Feld ist ein Byte.
    kante = 0 if groesse == 256 else groesse
    eintraege += struct.pack("<BBBBHHII", kante, kante, 0, 0, 1, 32, len(daten), offset)
    offset += len(daten)
ziel.write_bytes(kopf + eintraege + b"".join(d for _, d in bilder))
print(f"{ziel}: {len(bilder)} Einträge, {ziel.stat().st_size} Bytes")
PY

python3 - "$DIST" "$ZIEL/icon.icns" <<'PY'
import struct
import sys
from pathlib import Path

quelle, ziel = Path(sys.argv[1]), Path(sys.argv[2])
# Typ -> Kantenlänge in Pixeln (Apples Namen, 1x und 2x gemischt).
eintraege = {
    "icp4": 16,   # 16
    "icp5": 32,   # 32
    "ic07": 128,  # 128
    "ic08": 256,  # 256
    "ic09": 512,  # 512
    "ic10": 1024, # 512@2x
    "ic11": 32,   # 16@2x
    "ic12": 64,   # 32@2x
    "ic13": 256,  # 128@2x
    "ic14": 512,  # 256@2x
}
teile = []
for typ, groesse in eintraege.items():
    daten = (quelle / f"icon-{groesse}.png").read_bytes()
    teile.append(typ.encode("ascii") + struct.pack(">I", len(daten) + 8) + daten)
koerper = b"".join(teile)
ziel.write_bytes(b"icns" + struct.pack(">I", len(koerper) + 8) + koerper)
print(f"{ziel}: {len(eintraege)} Einträge, {len(koerper) + 8} Bytes")
PY

echo "== Logo =="
install -Dm644 "$DIST/logo-dark.svg" "$WURZEL/docs/marke/logo-dark.svg"
install -Dm644 "$DIST/logo-light.svg" "$WURZEL/docs/marke/logo-light.svg"

echo "== Prüfsummen =="
(cd "$WURZEL" && sha256sum packaging/icons/icon-*.png packaging/icons/icon.ico \
  packaging/icons/icon.icns packaging/icons/hicolor/*/apps/* docs/marke/logo-*.svg)

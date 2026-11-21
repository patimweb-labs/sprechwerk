#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
#
# Erzeugt aus den SVG-Quellen in `assets/` die Icons, die installiert werden
# (AGENTS.md §8, ADR 0015). Nicht von Hand ablegen: dieses Skript ist die
# einzige Brücke von der Zeichnung zu den PNGs.
#
# Aufruf aus dem Wurzelverzeichnis: packaging/icons/erzeugen.sh
# Braucht: ImageMagick mit librsvg (SVG-Delegate) *oder* rsvg-convert oder resvg.
#
# 16, 24 und 32 px kommen aus dem vereinfachten Zeichen (`sprechwerk-klein.svg`),
# alles darüber aus dem vollen Logo; die SVGs bleiben als `scalable` und
# `symbolic` daneben liegen.
set -euo pipefail
cd "$(dirname "$0")/../.."

QUELLE=assets
ZIEL=packaging/icons
GROSS="$QUELLE/sprechwerk-logo.svg"
KLEIN="$QUELLE/sprechwerk-klein.svg"
TEMPORAER="$(mktemp -d)"
trap 'rm -rf "$TEMPORAER"' EXIT

# --- Zeichner wählen ------------------------------------------------------
# zeichne <svg> <kante> <ziel>   quadratisch, durchsichtiger Grund
if command -v resvg >/dev/null; then
  zeichne() { resvg --background '#00000000' --width "$2" --height "$2" "$1" "$3"; }
elif command -v rsvg-convert >/dev/null; then
  zeichne() { rsvg-convert --background-color='#00000000' --width "$2" --height "$2" -o "$3" "$1"; }
elif command -v magick >/dev/null; then
  # -strip und die datumlosen Chunks: sonst schreibt ImageMagick die Uhrzeit ins
  # PNG und zwei Läufe ergeben verschiedene Prüfsummen (AGENTS.md §8.7).
  zeichne() {
    magick -background none "$1" -resize "${2}x${2}" -strip \
      -define png:exclude-chunks=date,time "PNG32:$3"
  }
else
  echo "Kein SVG-Zeichner gefunden: resvg, rsvg-convert oder ImageMagick installieren." >&2
  exit 1
fi

# --- PNGs -----------------------------------------------------------------
# Klein (vereinfachtes Zeichen), damit die Blasen bei 16 px lesbar bleiben.
for n in 16 24 32; do
  zeichne "$KLEIN" "$n" "$TEMPORAER/icon-$n.png"
done
for n in 48 64 128 256 512 1024; do
  zeichne "$GROSS" "$n" "$TEMPORAER/icon-$n.png"
done

# hicolor: alle Größen, wie es die Paketskripte und Flatpak erwarten.
for n in 16 24 32 48 64 128 256 512 1024; do
  install -Dm644 "$TEMPORAER/icon-$n.png" "$ZIEL/hicolor/${n}x${n}/apps/sprechwerk.png"
  rm -f "$ZIEL/hicolor/${n}x${n}/apps/sprechwerk.png.bak"
done
# Feste Namen für AppImage und README (packaging/linux/paket.sh liest sie).
for n in 32 128 256 512; do
  install -Dm644 "$TEMPORAER/icon-$n.png" "$ZIEL/icon-$n.png"
done

# --- SVG: skalierbar, Zeichen für den Schreibtisch ------------------------
install -Dm644 "$GROSS" "$ZIEL/hicolor/scalable/apps/sprechwerk.svg"
install -Dm644 "$QUELLE/sprechwerk-symbolic.svg" \
  "$ZIEL/hicolor/symbolic/apps/sprechwerk-symbolic.svg"

# --- Wortmarke für README und Doku ---------------------------------------
# Dieselbe Zeichnung, nur mit der Schriftfarbe des Grundes: hell auf dunklem
# Grund, die Flächenfarbe auf hellem Grund.
mkdir -p docs/marke
sed 's/#FFFFFF/#E9E5DC/' "$QUELLE/sprechwerk-wortmarke.svg" > docs/marke/logo-dark.svg
sed 's/#FFFFFF/#0E1319/' "$QUELLE/sprechwerk-wortmarke.svg" > docs/marke/logo-light.svg

# --- ICO und ICNS ---------------------------------------------------------
# Beides sind Behälter aus Typ und PNG-Nutzlast; wir schreiben sie von Hand,
# weil ImageMagick ICNS nicht schreiben und ICO nur ungepackt kann.
python3 - "$TEMPORAER" "$ZIEL/icon.ico" <<'PY'
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

python3 - "$TEMPORAER" "$ZIEL/icon.icns" <<'PY'
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

echo "Icons erzeugt:"
find "$ZIEL" -name "sprechwerk*" -o -name "icon.*" -o -name "icon-*.png" | sort | while read -r f; do
  printf '  %-58s %6s Byte\n' "$f" "$(stat -c%s "$f")"
done
printf '  %-58s %6s Byte\n' docs/marke/logo-dark.svg "$(stat -c%s docs/marke/logo-dark.svg)"
printf '  %-58s %6s Byte\n' docs/marke/logo-light.svg "$(stat -c%s docs/marke/logo-light.svg)"

#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Erzeugt die Icons der Fenster als SVG aus Lucide (ISC), wie `@weppelmann/icons`.

Quelle ist dieselbe kuratierte Auswahl wie in weppelmann-ui: das Paket
`lucide-vue-next` dort (`../weppelmann-ui/node_modules`). Die Zeichnung wird
unverändert übernommen; nur `currentColor` wird zu Weiß, damit egui das Icon
mit der Theme-Farbe einfärben kann (`Image::tint`).

    python3 scripts/icons-holen.py [pfad/zu/lucide-vue-next]
"""
import re
import shutil
import sys
from pathlib import Path

WURZEL = Path(__file__).resolve().parent.parent
QUELLE = Path(sys.argv[1]) if len(sys.argv) > 1 else WURZEL.parent / "weppelmann-ui/node_modules/lucide-vue-next"
ZIEL = WURZEL / "crates/oberflaeche/icons"

# Lucide-Name → Name im Bausatz (wie @weppelmann/icons, ohne „Icon").
AUSWAHL = {
    "plus": "plus", "ellipsis": "more", "lock": "lock", "panel-right": "panel_right",
    "x": "close", "mic": "mic", "mic-off": "mic_off", "headphones": "headphones",
    "headphone-off": "headphone_off", "plug": "plug", "unplug": "unplug",
    "settings": "settings", "refresh-cw": "refresh", "volume-2": "speaker",
    "message-square": "chat", "trash-2": "trash", "copy": "copy", "smile-plus": "react",
    "server": "server", "users": "users", "scroll-text": "log", "ban": "ban",
    "play": "play", "square": "stop", "send-horizontal": "send", "link": "link",
    "triangle-alert": "warning", "external-link": "external", "panel-right-close": "panel_close",
    "hexagon": "brand",
    "paperclip": "anhang", "file": "datei", "film": "film", "download": "herunterladen", "smile": "smiley",
}

KOPF = ('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" '
        'fill="none" stroke="#ffffff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">')


def svg_aus(js: str) -> str:
    teile = []
    flach = re.sub(r"\s+", " ", js)
    for art, attr in re.findall(r'\[ ?"(\w+)", \{([^}]*)\} ?\]', flach):
        werte = re.findall(r'(\w+): "([^"]*)"', attr)
        attrs = " ".join(f'{k}="{v}"' for k, v in werte if k != "key")
        teile.append(f"<{art} {attrs}/>")
    if not teile:
        raise SystemExit("keine Formen gefunden")
    return KOPF + "".join(teile) + "</svg>\n"


ZIEL.mkdir(parents=True, exist_ok=True)
for lucide, name in AUSWAHL.items():
    js = (QUELLE / "dist/esm/icons" / f"{lucide}.js").read_text(encoding="utf-8")
    (ZIEL / f"{name}.svg").write_text(svg_aus(js), encoding="utf-8")
# Eigene Zeichnung aus weppelmann-ui (Sprechblase mit Schwanz unten rechts).
(ZIEL / "sprechblase.svg").write_text(
    KOPF + '<path d="M6 4 H18 A3 3 0 0 1 21 7 V15 A3 3 0 0 1 18 18 H17 V21.5 L13.5 18 H6 '
    'A3 3 0 0 1 3 15 V7 A3 3 0 0 1 6 4 Z"/></svg>\n', encoding="utf-8")
shutil.copy(QUELLE / "LICENSE", ZIEL / "LUCIDE-LICENSE.txt")
print(f"{len(AUSWAHL) + 1} Icons nach {ZIEL.relative_to(WURZEL)}")

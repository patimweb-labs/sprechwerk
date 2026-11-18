#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Prüft, dass ein Release mit release-please überhaupt gelingen kann.

Gefunden 2026-11-16, beim Einrichten der Release-Pipeline:

* `crates/audio` hatte als einziges Crate eine **feste** Version statt
  `version.workspace = true`. release-please hebt die Version im Arbeitsbereich
  und in `Cargo.lock` an — `audio` wäre stehen geblieben, die Sperrdatei hätte
  nicht mehr gepasst, und jeder Bau mit `--locked` (Docker, Release) wäre
  gescheitert. Erst **nach** dem Merge des Release-PRs, mitten im Release.
* (bis 1.3) `beforeBuildCommand` in `tauri.conf.json` zeigte ins Leere. Seit
  ADR 0013 gibt es kein Tauri mehr; geprüft wird stattdessen, dass Fenster,
  Starter, Flatpak und Metainfo dieselbe Kennung tragen.

Beides fällt nur auf, wenn man ein Release baut. Diese Prüfung findet es vorher.

    python3 scripts/versionen.py      # 0 = in Ordnung
"""

import json
import re
import sys
import tomllib
from pathlib import Path

WURZEL = Path(__file__).resolve().parent.parent
befunde: list[str] = []


def toml(pfad: str) -> dict:
    return tomllib.loads((WURZEL / pfad).read_text(encoding="utf-8"))


def json_(pfad: str) -> dict:
    return json.loads((WURZEL / pfad).read_text(encoding="utf-8"))


# 1. Eine Version, überall dieselbe.
arbeitsbereich = toml("Cargo.toml")
version = arbeitsbereich["workspace"]["package"]["version"]
# Seit ADR 0013 erben alle Pakete die Version aus dem Arbeitsbereich (Punkt 2);
# die Paketskripte lesen sie dort (`packaging/version.sh`).

# Vor dem ersten Release steht im Manifest 0.0.0 (release-please macht daraus
# mit dem ersten `feat` 0.1.0); danach muss es die laufende Version sein.
manifest = json_(".release-please-manifest.json")["."]
if manifest not in (version, "0.0.0"):
    befunde.append(f".release-please-manifest.json: {manifest}, Arbeitsbereich {version}")

# 2. Jedes Crate im Arbeitsbereich erbt die Version.
for mitglied in arbeitsbereich["workspace"]["members"]:
    paket = toml(f"{mitglied}/Cargo.toml")["package"]
    if paket.get("version") != {"workspace": True}:
        befunde.append(f"{mitglied}/Cargo.toml: `version.workspace = true` fehlt")

# 3. release-please kennt jede Datei und jedes eigene Paket in den Sperrdateien.
konfig = json_("release-please-config.json")["packages"]["."]
erfasst: dict[str, set[str]] = {}
for eintrag in konfig["extra-files"]:
    if not (WURZEL / eintrag["path"]).is_file():
        befunde.append(f"release-please-config.json: {eintrag['path']} gibt es nicht")
    namen = set(re.findall(r"@\.name\.value == '([^']+)'", eintrag["jsonpath"]))
    erfasst.setdefault(eintrag["path"], set()).update(namen)
for sperre in ("Cargo.lock",):
    eigene = {
        p["name"] for p in toml(sperre)["package"]
        if p["name"].startswith("sprechwerk-") and "source" not in p
    }
    fehlt = eigene - erfasst.get(sperre, set())
    if fehlt:
        befunde.append(
            f"release-please-config.json: {sperre} — {', '.join(sorted(fehlt))} "
            "wird nicht angehoben (in `extra-files` ergänzen)"
        )

# 4. Eine Kennung für das Fenster: App-ID (main.rs), Starter, Flatpak,
#    Metainfo. Weichen sie ab, findet der Schreibtisch Symbol und Starter nicht.
kennung = "de.weppelmann.Sprechwerk"
haupt = (WURZEL / "crates/fenster/src/main.rs").read_text(encoding="utf-8")
if f'with_app_id("{kennung}")' not in haupt:
    befunde.append(f"crates/fenster/src/main.rs: App-ID ist nicht {kennung}")
starter = WURZEL / "packaging/linux" / f"{kennung}.desktop"
if not starter.is_file():
    befunde.append(f"{starter.relative_to(WURZEL)} fehlt")
elif f"StartupWMClass={kennung}" not in starter.read_text(encoding="utf-8"):
    befunde.append(f"{starter.relative_to(WURZEL)}: StartupWMClass ist nicht {kennung}")
flatpak = WURZEL / "packaging/flatpak" / f"{kennung}.yml"
if not flatpak.is_file():
    befunde.append(f"Flatpak-Manifest {flatpak.relative_to(WURZEL)} fehlt (heißt nach der Kennung)")
elif not re.search(rf"^id: {re.escape(kennung)}$", flatpak.read_text(encoding="utf-8"), re.M):
    befunde.append(f"{flatpak.relative_to(WURZEL)}: `id` ist nicht {kennung}")
metainfo = WURZEL / "packaging/linux" / f"{kennung}.metainfo.xml"
if not metainfo.is_file() or f"<id>{kennung}</id>" not in metainfo.read_text(encoding="utf-8"):
    befunde.append(f"{metainfo.relative_to(WURZEL)}: fehlt oder <id> ist nicht {kennung}")

if befunde:
    print("Versionen und Release-Konfiguration:")
    for b in befunde:
        print(f"  - {b}")
    sys.exit(1)
print(f"Version {version} einheitlich, release-please erfasst alle Dateien.")

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
* `beforeBuildCommand` in `tauri.conf.json` zeigte auf `../../ui`. Tauri führt
  den Befehl aber im Verzeichnis **über** `crates/desktop` aus — der Pfad ging
  ins Leere, `tauri build` lief nie.

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
gefunden = {
    "crates/desktop/Cargo.toml": toml("crates/desktop/Cargo.toml")["package"]["version"],
    "crates/desktop/tauri.conf.json": json_("crates/desktop/tauri.conf.json")["version"],
    "ui/package.json": json_("ui/package.json")["version"],
    "ui/package-lock.json": json_("ui/package-lock.json")["version"],
    "ui/package-lock.json (packages[''])": json_("ui/package-lock.json")["packages"][""]["version"],
}
for wo, v in gefunden.items():
    if v != version:
        befunde.append(f"{wo}: Version {v}, Arbeitsbereich {version}")

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
for sperre in ("Cargo.lock", "crates/desktop/Cargo.lock"):
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

# 4. Tauri führt die Vorbefehle im Verzeichnis über `crates/desktop` aus.
tauri = json_("crates/desktop/tauri.conf.json")
for schluessel in ("beforeDevCommand", "beforeBuildCommand"):
    befehl = tauri["build"].get(schluessel, "")
    treffer = re.search(r"--prefix\s+(\S+)", befehl)
    if treffer and not (WURZEL / "crates" / treffer.group(1) / "package.json").is_file():
        befunde.append(f"tauri.conf.json {schluessel}: `{treffer.group(1)}` zeigt nicht auf ui/")

if befunde:
    print("Versionen und Release-Konfiguration:")
    for b in befunde:
        print(f"  - {b}")
    sys.exit(1)
print(f"Version {version} einheitlich, release-please erfasst alle Dateien.")

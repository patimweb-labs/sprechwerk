#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Schreibt die kleinen Fassungen der Marke: 16, 24 und 32 px (ADR 0015).

Das Zeichen ist ein Pixelbild. Bei 32 px ist ein Balken der Welle 14/512 breit,
also noch kein halber Pixel; wird das große Logo auf 16 px verkleinert, glättet
der Zeichner und aus den Blasen wird ein Fleck. Darum liegt hier für jede kleine
Größe ein eigenes Raster, Pixel für Pixel gezeichnet, und das SVG besteht nur
aus ganzzahligen Rechtecken (kein Pfad, keine Kante zwischen zwei Pixeln).

Quelle ist das ASCII-Gitter unten: `k` Kachel, `t` Türkis (hinten), `o` Orange
(vorn), `.` nichts. Die 1-px-Trennung zwischen den Blasen rechnet das Skript
selbst (ein Ring um die vordere Blase in der Kachelfarbe) — im Gitter steht sie
deshalb nicht.

Aufruf aus dem Wurzelverzeichnis: packaging/icons/klein-gitter.py
Danach packaging/icons/erzeugen.sh für die PNGs.
"""

from pathlib import Path

# --- Farben: Tokens aus crates/oberflaeche/tokens/ ------------------------
KACHEL = "#131A22"      # color.bg.raised, oben in sprechwerk-logo.svg
TRENNUNG = "#0E1319"    # color.bg.panel, unten in sprechwerk-logo.svg
TUERKIS = "#2EE6C8"     # color.accent.default
ORANGE = "#F7931E"      # color.signal.default

FARBEN = {"k": KACHEL, "d": TRENNUNG, "t": TUERKIS, "o": ORANGE}

# Malreihenfolge: Kachel, Türkis, Trennung, Orange — die vordere Blase zuletzt.
REIHENFOLGE = "ktdo"

# --- Die Raster ----------------------------------------------------------
# 16 px: 1 px Umriss, eine dicke Welle je Blase. Auf drei Balken ist hier kein
# Platz: sie berühren den Umriss und lesen sich als Klammern `]I[`.
GITTER = {}

GITTER[16] = """
..kkkkkkkkkkkk..
.kkkkkkkkkkkkkk.
kkkkkkkkkkkkkkkk
kkttttttttkkkkkk
kktkkkkkktkkkkkk
kktkttkkoooooook
kktkttkkokkkkkok
kktkttkkokkookok
kktkkkkkokkookok
kkttttttokkookok
kkktttkkokkkkkok
kkkttkkkoooooook
kkkkkkkkkkkoookk
kkkkkkkkkkkkookk
.kkkkkkkkkkkkkk.
..kkkkkkkkkkkk..
"""

# 24 px: 2 px Umriss, drei Balken kurz-lang-kurz.
GITTER[24] = """
..kkkkkkkkkkkkkkkkkkkk..
.kkkkkkkkkkkkkkkkkkkkkk.
kkkkkkkkkkkkkkkkkkkkkkkk
kkkkkkkkkkkkkkkkkkkkkkkk
kkkkttttttttttkkkkkkkkkk
kkkttttttttttttkkkkkkkkk
kkttttkkkkkktttkkkkkkkkk
kktttkktkkkkkooooooookkk
kkttktktktkkooooooooookk
kkttktktktkooookkkkoookk
kkttkkktkkkoootkokkkookk
kkttkkkkkkkootokokokookk
kktttkkkkkkootokokokookk
kktttttttttootkkokkkookk
kkkttttttttookkkkkkoookk
kkkkkttttttoookkkkooookk
kkkkkttttttooooooooookkk
kkkkktttkkkkooooooookkkk
kkkkkkkkkkkkkkkkkkooookk
kkkkkkkkkkkkkkkkkkooookk
kkkkkkkkkkkkkkkkkkkkookk
kkkkkkkkkkkkkkkkkkkkkkkk
.kkkkkkkkkkkkkkkkkkkkkk.
..kkkkkkkkkkkkkkkkkkkk..
"""

# 32 px: 2 px Umriss, fünf Balken kurz-lang-kurz, Schwänze mit drei Stufen.
GITTER[32] = """
...kkkkkkkkkkkkkkkkkkkkkkkkkk...
..kkkkkkkkkkkkkkkkkkkkkkkkkkkk..
.kkkkkkkkkkkkkkkkkkkkkkkkkkkkkk.
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
kkkkttttttttttttttkkkkkkkkkkkkkk
kkkttttttttttttttttkkkkkkkkkkkkk
kkttttkkkkkkkkkktttkkkkkkkkkkkkk
kktttkkkkktkkkkkooooooooooookkkk
kkttkkkktktktkkooooooooooooookkk
kkttkktktktktkooootkkkkkkkoookkk
kkttkktktktktkooottkkokkkkkookkk
kkttkkkktktktkookttokokokkkookkk
kkttkkkkkktkkkookotokokokokookkk
kkttkkkkkkkkkkoototokokokokookkk
kktttkkkkkkkkkootttokokokkkookkk
kkttttttttttttoottkkkokkkkkookkk
kkktttttttttttootkkkkkkkkkoookkk
kkkktttttttkkkoookkkkkkkkooookkk
kkkktttttttkkkooooooooooooookkkk
kkkkttttkkkkkkkooooooooooookkkkk
kkkkttttkkkkkkkkkkkkkkoooooookkk
kkkkttkkkkkkkkkkkkkkkkoooooookkk
kkkkkkkkkkkkkkkkkkkkkkkkkooookkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkookkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk
.kkkkkkkkkkkkkkkkkkkkkkkkkkkkkk.
..kkkkkkkkkkkkkkkkkkkkkkkkkkkk..
...kkkkkkkkkkkkkkkkkkkkkkkkkk...
"""

BESCHREIBUNG = {
    16: "1-px-Umriss, eine Welle je Blase, kein Glühen",
    24: "2-px-Umriss, drei Balken kurz-lang-kurz",
    32: "2-px-Umriss, fünf Balken kurz-lang-kurz",
}


def lies(text, kante):
    zeilen = [z for z in text.strip("\n").split("\n") if z]
    if len(zeilen) != kante:
        raise SystemExit(f"{kante} px: {len(zeilen)} Zeilen statt {kante}")
    for nummer, zeile in enumerate(zeilen):
        if len(zeile) != kante:
            raise SystemExit(f"{kante} px, Zeile {nummer}: {len(zeile)} Zeichen statt {kante}")
        if set(zeile) - set("kto."):
            raise SystemExit(f"{kante} px, Zeile {nummer}: unbekanntes Zeichen in {zeile!r}")
    return [list(z) for z in zeilen]


def trenne(gitter):
    """1 px Kachelfarbe um die vordere Blase, damit beide Blasen auseinanderfallen.

    Nur Türkis wird abgeschnitten: die Kachel bleibt eine Fläche, sonst entstünde
    um die vordere Blase ein dunkler Hof, den die Vorlage nicht hat.
    """
    kante = len(gitter)
    rand = [z[:] for z in gitter]
    for y in range(kante):
        for x in range(kante):
            if gitter[y][x] != "o":
                continue
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    yy, xx = y + dy, x + dx
                    if 0 <= yy < kante and 0 <= xx < kante and gitter[yy][xx] == "t":
                        rand[yy][xx] = "d"
    return rand


def rechtecke(gitter, zeichen):
    """Waagerechte Läufe einer Farbe zeilenweise zu Rechtecken verschmelzen."""
    kante = len(gitter)
    offen = {}
    fertig = []
    for y in range(kante + 1):
        laeufe = []
        if y < kante:
            x = 0
            while x < kante:
                if gitter[y][x] == zeichen:
                    start = x
                    while x < kante and gitter[y][x] == zeichen:
                        x += 1
                    laeufe.append((start, x))
                else:
                    x += 1
        for lauf in list(offen):
            if lauf not in laeufe:
                fertig.append((lauf[0], offen[lauf], lauf[1] - lauf[0], y - offen[lauf]))
                del offen[lauf]
        for lauf in laeufe:
            offen.setdefault(lauf, y)
    return fertig


def svg(kante, gitter):
    teile = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{kante}" height="{kante}" '
        f'viewBox="0 0 {kante} {kante}" shape-rendering="crispEdges">',
        f"  <title>sprechwerk ({kante} px)</title>",
        f"  <!-- Kleine Fassung für {kante} Pixel: {BESCHREIBUNG[kante]} (ADR 0015).",
        "       Nur ganzzahlige Rechtecke, damit der Zeichner nichts glättet.",
        "       Quelle ist das Gitter in packaging/icons/klein-gitter.py:",
    ]
    teile += ["       " + "".join(z) for z in gitter]
    teile.append("  -->")
    for zeichen in REIHENFOLGE:
        for x, y, breite, hoehe in rechtecke(gitter, zeichen):
            teile.append(
                f'  <rect x="{x}" y="{y}" width="{breite}" height="{hoehe}" fill="{FARBEN[zeichen]}"/>'
            )
    teile.append("</svg>")
    return "\n".join(teile) + "\n"


def main():
    ziel = Path(__file__).resolve().parents[2] / "assets"
    for kante in sorted(GITTER):
        gitter = trenne(lies(GITTER[kante], kante))
        datei = ziel / f"sprechwerk-{kante}.svg"
        datei.write_text(svg(kante, gitter))
        print(f"{datei}: {sum(len(rechtecke(gitter, z)) for z in REIHENFOLGE)} Rechtecke")


if __name__ == "__main__":
    main()

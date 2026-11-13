#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Wertet die Mitschnitte von `scripts/lastprobe.sh` aus.

Vier Sprecher senden gleichzeitig je einen Ton (Standard: 300, 400, 500,
600 Hz). Jeder Hörer muss **alle vier** Töne sauber getrennt hören — kein
Verschlucken, kein Vertauschen, kein Mischen. Der Fremde im Nachbarraum
darf **nichts** hören.

Gerechnet wird je Aufnahme:
  * der Gesamtpegel (RMS) — kommt überhaupt etwas an?
  * je Sprecherton der Goertzel-Anteil, bezogen auf den stärksten Ton
    (Verhältnis in Prozent) — fehlt ein Ton, bricht sein Anteil ein;
  * laute Blöcke — der Fremde muss bei null liegen.

Aufruf: scripts/lastprobe.py <verzeichnis> [--mindest-anteil 10]
"""

import math
import os
import struct
import sys
import wave
from pathlib import Path

BLOCK = 960  # 20 ms bei 48 kHz


def liese(pfad: Path):
    with wave.open(str(pfad)) as w:
        anzahl = w.getnframes()
        rate = w.getframerate()
        return list(struct.unpack("<%dh" % anzahl, w.readframes(anzahl))), rate


def rms(werte):
    if not werte:
        return 0.0
    return math.sqrt(sum(x * x for x in werte) / len(werte))


def goertzel(werte, frequenz, rate):
    """Anteil einer Frequenz (normiert auf die Länge der Aufnahme)."""
    if not werte:
        return 0.0
    k = 2 * math.pi * frequenz / rate
    c = 2 * math.cos(k)
    s1 = s2 = 0.0
    for w in werte:
        s0 = w + c * s1 - s2
        s2 = s1
        s1 = s0
    return math.sqrt(max(0.0, s1 * s1 + s2 * s2 - c * s1 * s2)) / len(werte)


def laute_bloecke(werte, schwelle):
    anzahl = 0
    for i in range(0, max(0, len(werte) - BLOCK), BLOCK):
        if rms(werte[i : i + BLOCK]) > schwelle:
            anzahl += 1
    return anzahl


def main() -> int:
    argumente = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not argumente:
        print(__doc__)
        return 2
    verzeichnis = Path(argumente[0])
    mindest = 10.0
    for i, a in enumerate(sys.argv[1:]):
        if a == "--mindest-anteil":
            mindest = float(sys.argv[i + 2])

    frequenzen = [float(f) for f in os.environ.get("FREQUENZEN", "300 400 500 600").split()]
    aufnahmen = sorted(verzeichnis.glob("hoerer*.wav"))
    fremd = verzeichnis / "fremd.wav"
    if not aufnahmen:
        print("Keine Hörer-Aufnahmen gefunden — lief die Lastprobe?")
        return 2

    fehler = 0
    print(f"Sprechertöne: {' '.join(f'{int(f)} Hz' for f in frequenzen)}")
    print()
    print(f"{'Aufnahme':<14} {'RMS':>7}  Anteile je Ton (%, bezogen auf den stärksten)")
    for pfad in aufnahmen:
        werte, rate = liese(pfad)
        pegel = rms(werte)
        anteile = [goertzel(werte, f, rate) for f in frequenzen]
        staerkster = max(anteile) if anteile else 0.0
        prozent = [(a / staerkster * 100.0) if staerkster > 0 else 0.0 for a in anteile]
        zeile = " ".join(f"{p:6.1f}" for p in prozent)
        print(f"{pfad.stem:<14} {pegel:7.1f}  {zeile}")

        schwach = [
            f"{int(f)} Hz ({p:.1f} %)"
            for f, p in zip(frequenzen, prozent)
            if p < mindest
        ]
        if schwach:
            print(f"  FEHLER: zu schwach — {', '.join(schwach)}")
            fehler += 1
        if pegel < 200:
            print(f"  FEHLER: Gesamtpegel zu niedrig ({pegel:.1f})")
            fehler += 1

    if fremd.exists():
        werte, rate = liese(fremd)
        pegel = rms(werte)
        laut = laute_bloecke(werte, 800.0)
        print()
        print(f"{'Nachbarraum':<14} {pegel:7.1f}  laute Blöcke: {laut} (muss 0 sein)")
        if laut > 0 or pegel > 200:
            print("  FEHLER: der Nachbarraum hört mit")
            fehler += 1
    else:
        print("  FEHLER: Aufnahme des Nachbarraums fehlt")
        fehler += 1

    print()
    if fehler == 0:
        print(
            f"BESTANDEN: alle {len(aufnahmen)} Hörer hören alle "
            f"{len(frequenzen)} Sprecher, der Nachbarraum bleibt still."
        )
        return 0
    print(f"FEHLGESCHLAGEN: {fehler} Befund(e).")
    return 1


if __name__ == "__main__":
    sys.exit(main())

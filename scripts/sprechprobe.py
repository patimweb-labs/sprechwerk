#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Wertet die Aufnahmen von `scripts/sprechprobe.sh` aus.

Der Ton ist bekannt (440 Hz). Deshalb lässt sich ohne Hörprobe sagen, ob der
Sprachpfad das Richtige tut:

* **Energie** — kommt überhaupt etwas an?
* **Frequenz** (Goertzel) — ist es der gesendete Ton und nicht ein Vielfaches?
* **Raumtreue** — bleibt der Ton im Raum (zweiter Raum bleibt still)?
* **Laufzeit** — Ankunftszeit des ersten Rahmens minus Sendezeitpunkt
  (beide Proben laufen auf demselben Rechner und teilen die Uhr).
"""

import math
import struct
import sys
import wave
from pathlib import Path

RAHMEN_MS = 20


def liese_scan(pfad: Path):
    """Liest Abtastwerte als Liste von int16."""
    with wave.open(str(pfad)) as w:
        anzahl = w.getnframes()
        return struct.unpack("<%dh" % anzahl, w.readframes(anzahl)), w.getframerate()


def blockpegel(werte, rate, block=960):
    return [
        math.sqrt(sum(x * x for x in werte[i : i + block]) / block)
        for i in range(0, len(werte) - block, block)
    ]


def goertzel(werte, frequenz, rate):
    """Anteil einer Frequenz (normiert auf die Blocklänge)."""
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


def laufzeit(sender_log: Path, hoerer_log: Path):
    """Laufzeit je Rahmen: Ankunft minus (Sendebeginn + Folge × 20 ms)."""
    beginn = None
    for zeile in sender_log.read_text(errors="replace").splitlines():
        if zeile.startswith("SENDEBEGINN"):
            beginn = int(zeile.split()[1])
    if beginn is None:
        return None
    werte = []
    for zeile in hoerer_log.read_text(errors="replace").splitlines():
        if not zeile.startswith("ANKUNFT"):
            continue
        _, folge, ankunft = zeile.split()
        versatz = (int(ankunft) - beginn) / 1e6  # ns → ms
        werte.append(versatz - int(folge) * RAHMEN_MS)
    return werte


def prozent(werte, anteil):
    if not werte:
        return 0.0
    sortiert = sorted(werte)
    index = min(len(sortiert) - 1, int(round(anteil * (len(sortiert) - 1))))
    return sortiert[index]


def main():
    ordner = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    fehler = []

    # ---- Raum 1: hier muss der Ton ankommen -------------------------
    werte, rate = liese_scan(ordner / "raum1.wav")
    pegel = blockpegel(werte, rate)
    laut = [i for i, p in enumerate(pegel) if p > 500]
    print(f"Raum 1: {len(werte) / rate:.1f} s, laute Blöcke: {len(laut)}")

    if not laut:
        fehler.append("Raum 1: kein Ton angekommen")
    else:
        dauer_ms = len(laut) * RAHMEN_MS
        print(f"  Tondauer im Mitschnitt: {dauer_ms} ms (gesendet: 3000 ms)")
        if not 1500 <= dauer_ms <= 4000:
            fehler.append(f"Raum 1: Tondauer unplausibel ({dauer_ms} ms)")

        start = laut[0] * 960
        abschnitt = werte[start : start + rate // 2]
        anteile = {f: goertzel(abschnitt, f, rate) for f in (220, 330, 440, 660, 880)}
        print("  Frequenzanteile: " + ", ".join(f"{f} Hz: {a:.0f}" for f, a in anteile.items()))
        if anteile[440] < 1000:
            fehler.append(f"Raum 1: 440 Hz zu schwach ({anteile[440]:.0f})")
        stoerer = max((a for f, a in anteile.items() if f != 440), default=0)
        if stoerer > anteile[440] / 10:
            fehler.append(f"Raum 1: Nebenfrequenz zu stark ({stoerer:.0f})")
        spitze = max(abs(x) for x in werte)
        print(f"  Spitze: {spitze} / 32767")
        if spitze >= 32767:
            fehler.append("Raum 1: Übersteuerung (Anschlag)")

    # ---- Raum 9: hier darf nichts ankommen --------------------------
    werte9, rate9 = liese_scan(ordner / "raum9.wav")
    pegel9 = blockpegel(werte9, rate9)
    laut9 = [p for p in pegel9 if p > 500]
    print(f"Raum 9: laute Blöcke: {len(laut9)} (muss 0 sein)")
    if laut9:
        fehler.append("Raum 9: Ton hat den Raum verlassen")

    # ---- Kein Rückkanal an den Sprecher ----------------------------
    sender_log = ordner / "sender.log"
    text = sender_log.read_text(errors="replace") if sender_log.exists() else ""
    if "angenommen: 0" not in text:
        fehler.append("Sprecher hat eigene Sprache zurückbekommen (Rückkanal!)")

    # ---- Laufzeit ---------------------------------------------------
    zeiten = laufzeit(sender_log, ordner / "hoerer1.log")
    if zeiten:
        print(
            "Laufzeit durch den Server (Ankunft − Sendezeitpunkt): "
            f"p50 {prozent(zeiten, 0.5):.1f} ms, "
            f"p95 {prozent(zeiten, 0.95):.1f} ms, "
            f"p99 {prozent(zeiten, 0.99):.1f} ms, "
            f"n={len(zeiten)}"
        )
        if prozent(zeiten, 0.95) > 200:
            fehler.append("Laufzeit über 200 ms (p95)")

    print()
    if fehler:
        print("NICHT BESTANDEN:")
        for f in fehler:
            print(f"  - {f}")
        return 1
    print("BESTANDEN: Sprache geht durch den Server, bleibt im Raum, kommt sauber an.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

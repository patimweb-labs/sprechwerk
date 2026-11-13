#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""Signalgeber und Auswertung für `scripts/audio-ende-zu-ende.sh`.

Zwei Aufgaben, ein Werkzeug — wie die Probe selbst:

```bash
# Signal auf die Standardausgabe (rohe 16-Bit-Werte, mono, 48 kHz)
audio-ende-zu-ende.py signal --art ton    --sekunden 3 --hz 440 | pacat ...
audio-ende-zu-ende.py signal --art stimme --sekunden 3

# Mitschnitte auswerten (Rückgabewert 0 = bestanden)
audio-ende-zu-ende.py ergebnis --ton ton.pcm --stimme stimme.pcm --hz 440
```

Ausgewertet wird ohne Hörprobe:

* **Ankunft** — ist überhaupt etwas angekommen (Blockpegel über dem Rauschen)?
* **Frequenz** (Goertzel) — ist es der gesendete Ton und nicht der halbe,
  doppelte oder ein fremder?
* **Stille** — war vor dem Signal wirklich nichts zu hören (kein Übersprechen,
  kein Rauschen, kein Rückkanal)?
* **Dauer** — liegt das Signal in der erwarteten Länge (Jitter-Puffer und
  Paketverlust verschieben, aber strecken nicht)?
* **Stimme** — kommt Sprache durch Rauschunterdrückung und Noisegate?
"""

import argparse
import math
import struct
import sys

RATE = 48000
BLOCK = 960  # 20 ms
RAUSCHEN = 120  # Blockpegel darunter gilt als Stille


# --------------------------------------------------------------- Signalgeber

def signal_ton(sekunden: float, hz: float, rate: int = RATE) -> list[int]:
    """Reiner Sinus — die einfachste Prüfung: kommt genau das an?"""
    anzahl = int(sekunden * rate)
    werte = []
    for i in range(anzahl):
        t = i / rate
        wert = 0.4 * math.sin(2 * math.pi * hz * t)
        werte.append(int(wert * 32767))
    return blende(werte, rate)


def signal_stimme(sekunden: float, rate: int = RATE) -> list[int]:
    """Stimmähnlich: Grundton mit Obertönen, silbenweise moduliert.

    Ein reiner Sinus wird von RNNoise zurecht als Ton und nicht als Stimme
    eingeordnet — für die Prüfung „kommt Sprache durch die Entrauschung?"
    braucht es deshalb etwas, das wie Sprache aussieht: 120 Hz Grundton mit
    Oberwellen und einer Hüllkurve von 4 Hz (Silben).
    """
    anzahl = int(sekunden * rate)
    werte = []
    for i in range(anzahl):
        t = i / rate
        huelle = 0.5 + 0.5 * math.sin(2 * math.pi * 4.0 * t - math.pi / 2)
        s = (
            math.sin(2 * math.pi * 120.0 * t)
            + 0.5 * math.sin(2 * math.pi * 240.0 * t)
            + 0.3 * math.sin(2 * math.pi * 360.0 * t)
        )
        werte.append(int(0.4 * huelle * s / 1.8 * 32767))
    return blende(werte, rate)


def blende(werte: list[int], rate: int, ms: int = 20) -> list[int]:
    """Ein- und ausblenden, damit der Anfang nicht knackt."""
    rand = int(rate * ms / 1000)
    for i in range(min(rand, len(werte))):
        faktor = i / rand
        werte[i] = int(werte[i] * faktor)
        werte[-1 - i] = int(werte[-1 - i] * faktor)
    return werte


# ------------------------------------------------------------------ Auswertung

def lies(pfad: str) -> list[int]:
    """Liest rohe 16-Bit-Werte (kleine Ende, mono)."""
    with open(pfad, "rb") as datei:
        roh = datei.read()
    anzahl = len(roh) // 2
    return list(struct.unpack("<%dh" % anzahl, roh[: anzahl * 2]))


def blockpegel(werte: list[int], block: int = BLOCK) -> list[float]:
    return [
        math.sqrt(sum(x * x for x in werte[i : i + block]) / block)
        for i in range(0, len(werte) - block + 1, block)
    ]


def goertzel(werte: list[int], frequenz: float, rate: int = RATE) -> float:
    """Anteil einer Frequenz, normiert auf die Anzahl der Werte."""
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


def fenster(pegel: list[float]) -> tuple[int, int]:
    """Erster und letzter lauter Block (20-ms-Raster)."""
    laut = [i for i, p in enumerate(pegel) if p > RAUSCHEN]
    if not laut:
        return (0, 0)
    return (laut[0], laut[-1])


def beschreibe(name: str, werte: list[int], soll_dauer: float = 3.0):
    """Misswerte eines Mitschnitts, ohne Urteil."""
    pegel = blockpegel(werte)
    anfang, ende = fenster(pegel)
    dauer = len(werte) / RATE
    laut = sum(1 for p in pegel if p > RAUSCHEN)
    return {
        "name": name,
        "dauer": dauer,
        "hoechster": max(pegel) if pegel else 0.0,
        "laut": laut,
        "anfang": anfang * BLOCK / RATE,
        "ende": (ende + 1) * BLOCK / RATE,
        "signal": (ende - anfang + 1) * BLOCK / RATE if laut else 0.0,
        "soll": soll_dauer,
        "werte": werte,
        "pegel": pegel,
    }


def ergebnis(args) -> int:
    fehler: list[str] = []
    hz = args.hz

    # ---- Lauf 1: reiner Ton ------------------------------------------
    ton = beschreibe("Ton 440 Hz", lies(args.ton))
    print(f"Ton (ohne Entrauschung): {ton['dauer']:.1f} s Mitschnitt")
    print(f"  höchster Blockpegel  {ton['hoechster']:7.0f}")
    print(f"  Signal von … bis     {ton['anfang']:7.2f} s … {ton['ende']:6.2f} s"
          f"  ({ton['signal']:.2f} s, gesendet {ton['soll']:.1f} s)")

    if ton["hoechster"] < 500:
        fehler.append(
            f"Ton: nichts angekommen (höchster Blockpegel {ton['hoechster']:.0f})"
        )
    if ton["anfang"] < 1.0:
        fehler.append(
            f"Ton: schon vor dem Senden war Ton zu hören ({ton['anfang']:.2f} s) — "
            "Rückkanal, Übersprechen oder Rauschen?"
        )
    if ton["signal"] and not (ton["soll"] * 0.5 <= ton["signal"] <= ton["soll"] * 1.6):
        fehler.append(
            f"Ton: Signallänge {ton['signal']:.2f} s passt nicht zu {ton['soll']:.1f} s"
        )

    # Frequenz im lautesten halben Block: derselbe Ton, nicht ein anderer?
    if ton["hoechster"] >= 500:
        beginn = int(ton["anfang"] * RATE) + int(0.2 * RATE)
        probe = ton["werte"][beginn : beginn + int(0.5 * RATE)]
        gemessen = {
            "gesendet": goertzel(probe, hz),
            "halbe": goertzel(probe, hz / 2),
            "doppelte": goertzel(probe, hz * 2),
            "fremd": goertzel(probe, 1000.0),
        }
        print("  Frequenzanteile      " + ", ".join(
            f"{k} {v:.0f}" for k, v in gemessen.items()
        ))
        if gemessen["gesendet"] <= gemessen["halbe"] or gemessen["gesendet"] <= gemessen["doppelte"]:
            fehler.append(
                f"Ton: {hz:.0f} Hz ist nicht der stärkste Anteil "
                f"(gemessen: {hz:.0f} Hz {gemessen['gesendet']:.0f}, "
                f"{hz / 2:.0f} Hz {gemessen['halbe']:.0f}, "
                f"{hz * 2:.0f} Hz {gemessen['doppelte']:.0f}, "
                f"1000 Hz {gemessen['fremd']:.0f})"
            )

    # ---- Lauf 2: Stimme durch RNNoise und Gate -----------------------
    stimme = beschreibe("Stimme (RNNoise + Gate)", lies(args.stimme))
    print(f"Stimme (mit Entrauschung und Gate): {stimme['dauer']:.1f} s Mitschnitt")
    print(f"  höchster Blockpegel  {stimme['hoechster']:7.0f}")
    print(f"  Signal von … bis     {stimme['anfang']:7.2f} s … {stimme['ende']:6.2f} s"
          f"  ({stimme['signal']:.2f} s, gesendet {stimme['soll']:.1f} s)")

    if stimme["hoechster"] < 300:
        fehler.append(
            "Stimme: Rauschunterdrückung oder Gate schluckt die Sprache "
            f"(höchster Blockpegel {stimme['hoechster']:.0f})"
        )
    if stimme["anfang"] < 1.0:
        fehler.append(f"Stimme: schon vor dem Senden war etwas zu hören ({stimme['anfang']:.2f} s)")
    if stimme["signal"] and not (
        stimme["soll"] * 0.4 <= stimme["signal"] <= stimme["soll"] * 1.8
    ):
        fehler.append(
            f"Stimme: Signallänge {stimme['signal']:.2f} s passt nicht zu {stimme['soll']:.1f} s"
        )

    # ---- Urteil ------------------------------------------------------
    if fehler:
        print()
        for text in fehler:
            print(f"FEHLER: {text}")
        return 1
    print()
    print("bestanden: Ton und Stimme kommen durch den echten Pfad an, "
          "in der richtigen Frequenz und ohne Vorklang.")
    return 0


def eingabe(args) -> int:
    if args.art == "ton":
        werte = signal_ton(args.sekunden, args.hz)
    else:
        werte = signal_stimme(args.sekunden)
    sys.stdout.buffer.write(struct.pack("<%dh" % len(werte), *werte))
    sys.stdout.buffer.flush()
    return 0


def main() -> int:
    zerleger = argparse.ArgumentParser(description=__doc__)
    unter = zerleger.add_subparsers(dest="befehl", required=True)

    geb = unter.add_parser("signal", help="Signal auf die Standardausgabe")
    geb.add_argument("--art", choices=["ton", "stimme"], default="ton")
    geb.add_argument("--sekunden", type=float, default=3.0)
    geb.add_argument("--hz", type=float, default=440.0)
    geb.set_defaults(funktion=eingabe)

    aus = unter.add_parser("ergebnis", help="Mitschnitte auswerten")
    aus.add_argument("--ton", required=True)
    aus.add_argument("--stimme", required=True)
    aus.add_argument("--hz", type=float, default=440.0)
    aus.set_defaults(funktion=ergebnis)

    args = zerleger.parse_args()
    return args.funktion(args)


if __name__ == "__main__":
    sys.exit(main())

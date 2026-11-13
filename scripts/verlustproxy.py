#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Patrick Weppelmann
"""UDP-Störstrecke: Verzögerung, Jitter und Verlust auf dem Weg zum Server.

Ein Client verbindet sich auf den Port dieses Programms; alles wird an den
echten Server weitergeleitet und zurück. Damit lässt sich auf **einem** Rechner
prüfen, was sonst nur ein schlechtes Netz zeigt:

* **Verlust** — der Empfänger hört Lücken (das ist der Alltag im WLAN).
* **Jitter** — Pakete kommen ungleichmäßig an (der Jitter-Puffer muss halten).
* **Verzögerung** — alles später, aber gleichmäßig.

Aufruf:

    verlustproxy.py --port 18000 --ziel 127.0.0.1:7443 \
                    [--verlust 0.03] [--jitter-ms 15] [--verzoegerung-ms 5]

Ein Client je Instanz: der Proxy merkt sich die zuletzt gesehene
Client-Adresse. QUIC braucht das nicht anders — es sieht nur eine Verbindung.

Ausgabe auf stderr: `ANGENOMMEN`, `VERWORFEN`, `WEITER` (Zähler zum Ablesen).
"""

from __future__ import annotations

import argparse
import heapq
import random
import socket
import sys
import time


def main() -> int:
    zerleger = argparse.ArgumentParser(description="UDP-Störstrecke")
    zerleger.add_argument("--port", type=int, required=True, help="Port für den Client")
    zerleger.add_argument("--ziel", required=True, help="Server als host:port")
    zerleger.add_argument("--verlust", type=float, default=0.0, help="0.0–1.0")
    zerleger.add_argument("--jitter-ms", type=float, default=0.0)
    zerleger.add_argument("--verzoegerung-ms", type=float, default=0.0)
    zerleger.add_argument("--bind", default="127.0.0.1")
    args = zerleger.parse_args()

    ziel_host, _, ziel_port = args.ziel.rpartition(":")
    ziel = (ziel_host or "127.0.0.1", int(ziel_port))
    angenommen = verworfen = 0

    vorne = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    vorne.bind((args.bind, args.port))
    vorne.setblocking(False)

    # Der Server sieht **eine** Adresse (uns). Wir schicken von einem eigenen
    # Socket, damit die Antworten eindeutig zurückkommen.
    hinten = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    hinten.bind((args.bind, 0))
    hinten.setblocking(False)

    client: tuple[str, int] | None = None
    # Warteschlange: (fällig, nummer, sockel, ziel, daten)
    wartend: list[tuple[float, int, str, tuple[str, int], bytes]] = []
    nummer = 0

    print(
        f"Strecke {args.bind}:{args.port} → {ziel[0]}:{ziel[1]} "
        f"(Verlust {args.verlust:.0%}, Jitter {args.jitter_ms:.0f} ms, "
        f"Verzögerung {args.verzoegerung_ms:.0f} ms)",
        file=sys.stderr,
        flush=True,
    )

    while True:
        jetzt = time.monotonic()
        # Fällige Pakete ausliefern.
        while wartend and wartend[0][0] <= jetzt:
            _, _, ausgang, adresse, daten = heapq.heappop(wartend)
            try:
                (vorne if ausgang == "vorne" else hinten).sendto(daten, adresse)
            except OSError:
                pass

        # Warten, bis etwas kommt (oder das nächste Paket fällig ist).
        frist = (wartend[0][0] - jetzt) if wartend else 0.2
        import select

        lesbar, _, _ = select.select([vorne, hinten], [], [], max(0.0, min(frist, 0.2)))
        for sockel in lesbar:
            try:
                daten, adresse = sockel.recvfrom(65535)
            except OSError:
                continue
            # `ausgang` ist der Sockel, über den **weitergeschickt** wird: ein
            # Paket vom Client geht über `hinten` zum Server, eines vom Server
            # über `vorne` zum Client. (Vertauscht man das, antwortet der Proxy
            # von einem fremden Port — QUIC verwirft das, weil der Client seine
            # Gegenstelle festhält.)
            if sockel is vorne:
                client = adresse
                ausgang, fernziel = "hinten", ziel
            else:
                if client is None:
                    continue
                ausgang, fernziel = "vorne", client

            if args.verlust > 0 and random.random() < args.verlust:
                verworfen += 1
                print(f"VERWORFEN {verworfen}", file=sys.stderr, flush=True)
                continue
            angenommen += 1

            verzoegerung = args.verzoegerung_ms / 1000.0
            if args.jitter_ms > 0:
                verzoegerung += random.uniform(0, args.jitter_ms) / 1000.0
            nummer += 1
            heapq.heappush(
                wartend,
                (time.monotonic() + verzoegerung, nummer, ausgang, fernziel, daten),
            )


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(0)

# Marke

Hier liegt die Vorlage und das, was aus ihr für README und Doku entsteht.
**Die Quelle sind die SVGs in [`../assets/`](../assets/)** — nicht dieses
Verzeichnis und kein ASCII-Raster.

| Datei | Zweck |
|---|---|
| `entwurf.png` | Vorlage des Nutzers, an der Zeichen und Wortmarke hängen |
| `logo-dark.svg` | Zeichen + Wortmarke für dunklen Grund (Wortmarke `#E9E5DC`) |
| `logo-light.svg` | dasselbe für hellen Grund (Wortmarke `#0E1319`) |

`entwurf.png` ist die Referenz, nicht das Ergebnis: daran sind die Formen
gemessen (zwei Blasen, Treppen als Fasen, Schwanz unten, Welle aus fünf Balken;
Wortmarke breit und fett, Strich knapp ein Viertel der Höhe). Die Maßtabelle
steht in [ADR 0015](../adr/0015-logo-und-farben.md).

Geändert wird in `assets/`. Danach:

```bash
packaging/icons/erzeugen.sh
```

Das Skript zeichnet die Icons (`packaging/icons/`) und legt die beiden
Logofassungen hier ab. Ein zweiter Lauf in einem leeren Verzeichnis muss
dieselben Bilder ergeben (AGENTS.md §8.7).

Die Farben und die Abweichung von der einfarbigen Markenregel stehen in
[ADR 0015](../adr/0015-logo-und-farben.md); die Vorgängerregel in
[ADR 0014](../adr/0014-marke-zwei-farben.md) ist ersetzt.

# Marke

Hier liegen die erzeugten Dateien der Marke — nicht die Quelle.

Die Quelle ist ein ASCII-Raster im Nachbar-Repository:
`weppelmann-ui/apps/markenwerk/marken/sprechwerk.pm` (Zeichen) und
`weppelmann-ui/apps/markenwerk/src/schrift.ts` (Wortmarke). Geändert wird dort,
danach:

```bash
scripts/marke-bauen.sh [pfad/zu/apps/markenwerk]
```

Das Skript rendert in `apps/markenwerk`, kopiert SVG und PNG nach
`packaging/icons` (die Paketskripte lesen von dort), baut ICO und ICNS und legt
die beiden Logofassungen hier ab. Ein frischer Lauf in einem leeren Verzeichnis
muss dieselben `sha256sum` ergeben (AGENTS.md §8.7).

| Datei | Zweck |
|---|---|
| `logo-dark.svg` | Zeichen + Wortmarke für dunklen Grund (Wortmarke `#E9E5DC`) |
| `logo-light.svg` | dasselbe für hellen Grund (Wortmarke `#0E1319`) |

Die Farben und die Abweichung von der einfarbigen Markenregel stehen in
[ADR 0014](../adr/0014-marke-zwei-farben.md).

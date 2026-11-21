# ADR 0014 — Marke in zwei Farben: dunkle Fläche, türkis, orange

**Status:** entschieden und umgesetzt (2026-11-21)
**Betrifft:** `packaging/icons/`, `docs/marke/`, `README.md`,
`weppelmann-ui/apps/markenwerk/marken/sprechwerk.pm`,
`weppelmann-ui/packages/tokens/src/primitives.tokens.json`,
`weppelmann-ui/docs/marke/icons.md`

## Kontext

Die Markenregel aus `weppelmann-ui/docs/marke/icons.md` war bis hierher
einfarbig: **eine kräftige Fläche je Anwendung, die Zeichnung immer in
Knochenweiß** `#E9E5DC`. Hexwerk grün, WLED rot, Crosspoint orange, sprechwerk
orange (`#F2A63B` bzw. `#C9862C`) — daran erkennt man am Dock, dass die
Programme zusammengehören.

Das bisherige sprechwerk-Zeichen war entsprechend gebaut: **eine** Sprechblase
als weißer Umriss auf oranger Kachel. Der neue Entwurf des Nutzers zeigt zwei
überlappende Sprechblasen — hinten links eine größere türkise, vorne rechts eine
kleinere orange, beide nur als Umriss, innen die dunkle Fläche, in jeder Blase
eine Welle aus Balken. Die Fläche ist dunkel.

Damit trägt die alte Regel nicht mehr:

* **Zwei Formen brauchen zwei Töne.** Beide Blasen in Knochenweiß auf einer
  Fläche verschmelzen bei 16 px zu einem Fleck — genau der Fehler, den
  `icons.md` §5 schon beschreibt („zwei gleich gewichtete Formen auf 16 px
  verschmelzen").
* **Die Fläche ist das Dunkel, nicht die Farbe.** Die Blasen sind Umrisse; die
  Fläche ist der Innenraum. Eine orange Fläche mit weißer Zeichnung kann diesen
  Entwurf nicht abbilden, ohne dass die vordere Blase in der Fläche verschwindet.
* **Der Schatten ist kein Dekor mehr, sondern die zweite Form.** Das
  `.pm`-Format kennt dafür seit Anfang `shade` (`+`) — bisher ungenutzt.

## Entscheidung

Die Marke von sprechwerk bekommt eine eigene Farbigkeit:

| Rolle | Wert | Token |
|---|---|---|
| Fläche (`surface`) | `#0E1319` | `color.neutral.900` / `color.marke.sprechwerk.surface` |
| Zeichnung (`ink`) | `#2EE6C8` | `color.marke.sprechwerk.ink` |
| Schatten (`shade`) | `#F7931E` | `color.marke.sprechwerk.shade` |

Der Entwurf des Nutzers nannte für die Fläche „~#111827". Übernommen wurde
`#0E1319`, das dunkle Canvas des Bausatzes (`color.neutral.900`): ein eigener
Navy-Ton neben dem vorhandenen Dunkel wäre eine zweite Dunkelfarbe, die niemand
unterscheiden kann. Türkis `#2EE6C8`, Orange `#F7931E` und Knochenweiß `#E9E5DC`
stehen so im Entwurf.

Was **bleibt**: die Achteck-Kachel des Generators (22 % Fase), die 2-px-
Strichsprache, mindestens 1 px Abstand zur Kante und zur Fase, das 32 × 32
Raster als Quelle und die Regel „erzeugen, nicht ablegen" (§8.6/§8.7).

Was **neu** ist:

1. **Zwei Farben im Zeichen.** `#` ist türkis, `+` ist orange. Die hintere Blase
   liegt oben links, die vordere unten rechts und deckt deren untere rechte
   Ecke ab — die Überlappung ist die Aussage „Gespräch", nicht Zierrat.
2. **Alles auf dem 2-px-Raster.** Der Verkleinerer entscheidet per Mehrheit;
   was auf halben Pixeln sitzt, verschwindet bei 16 px. Umriss, Balken und
   Fasen liegen deshalb auf geraden Koordinaten, der mittlere Balken jeder
   Welle ist 2 px breit — so bleibt bei 16 px wenigstens ein Strich je Blase.
3. **Wortmarke als Raster.** „SPRECHWERK" in Pixellettern
   (`weppelmann-ui/apps/markenwerk/src/schrift.ts`), gesetzt von
   `node src/cli.ts logo sprechwerk` in zwei Fassungen: `dark` mit Wortmarke in
   Knochenweiß, `light` mit Wortmarke in der Flächenfarbe. Kein `<text>` und
   keine Systemschrift — sonst sieht die Marke je Rechner anders aus.
4. **Werte als Primitiven.** `color.marke.sprechwerk.*` in
   `primitives.tokens.json`, damit die Zahlen eine Quelle haben; die `.pm`
   bleibt bei Hex-Werten, weil sie das Format so verlangt.
5. **Ein Bauschritt.** `scripts/marke-bauen.sh` rendert aus dem Raster und
   schreibt PNG, SVG, ICO, ICNS und das Logo; `sha256sum` belegt, dass die
   Dateien im Projekt aus der Quelle stammen.

## Alternativen

* **Regel beibehalten, eine Blase.** Billig, aber der Entwurf des Nutzers ist
  zwei Blasen; eine Blase sagt „Sprache", zwei sagen „Gespräch".
* **Zwei Blasen, beide Knochenweiß auf oranger Fläche.** Bei 32 px noch lesbar,
  bei 16 px ein Fleck (in `icons.md` §5 gemessen). Verworfen.
* **Zwei Blasen, Fläche bone, Zeichnung orange und türkis.** Drei Farben plus
  Umriss, und die Kachel wird hell — die Serie hätte dann ein helles Zeichen
  zwischen dunklen. Verworfen.
* **Verlauf, Glow oder Schattenwurf für die Tiefe.** Widerspricht dem
  Pixelstil (`icons.md` §1: kein Verlauf, kein Schatten).
* **Wortmarke als `<text>` mit Archivo/IBM Plex.** Kürzere Datei, aber die
  Marke hinge an einer installierten Schrift; im README und in Flatpak-
  Screenshots wäre sie unterschiedlich. Verworfen.

## Preis

* **Die Serie bricht.** Bisher galt „eine Fläche, eine Zeichnungsfarbe" für
  alle Anwendungen; sprechwerk steht jetzt als dunkles Zeichen mit zwei Farben
  daneben. Das ist gewollt (der Nutzer hat die Regel für diese Marke geändert),
  kostet aber die Wiedererkennung „Haus" — dafür trägt das Zeichen die
  Bildidee.
* **Der monochrome Stil verliert die Farbe.** Tray und `symbolic.svg` sind
  einfarbig; dort tragen nur die Umrisse. Bei 16 px bleibt je Blase ein Strich —
  weniger Detail als bei 32 px.
* **Ein Schatten mehr im Generator.** `shade` war optional und ungenutzt; jetzt
  hängt eine Marke daran, also muss jede Prüfung, die Farben zählt, mit drei
  Farben umgehen (Tests: „Verkleinern mischt keine Farben" prüft weiter zwei
  Farben bei einer Marke ohne `shade`).
* **Ein Bauschritt mehr.** Ohne `node` und das Nachbar-Repository
  (`weppelmann-ui`) lässt sich die Marke nicht neu erzeugen; die erzeugten
  Dateien liegen deshalb im Projekt und werden per Prüfsumme geprüft, nicht bei
  jedem Build neu gerechnet.

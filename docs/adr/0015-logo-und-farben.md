# 0015 – Logo 1:1 aus der Vorlage, Farben aus dem Zeichen

Stand: 2026-11-21 · entschieden · Ausnahme zu [AGENTS.md §8](../../AGENTS.md),
löst [ADR 0014](0014-marke-zwei-farben.md) ab

## Kontext

Wunsch des Nutzers, wörtlich: „Marke sprechwerk nach dem Vorbild Crosspoint …
Das Logo 1:1 aus der Vorlage, Quelle sind SVGs.“

Maßgeblich ist [ADR 0008 des Nachbarprojekts Crosspoint](../../../crosspoint/docs/adr/0008-logo-und-farben.md):
ein Bild des Nutzers als Vorlage, die Maße daraus ausgemessen, die Quelle sind
SVGs im Projekt, erzeugt wird mit einem Skript, die Farben stehen als Tokens,
und ein Test hält Zeichen und Tokens zusammen.

Vorlage ist `docs/marke/entwurf.png` (1536 × 1024): oben das Zeichen — zwei
überlappende Sprechblasen im Pixelstil, hinten links eine türkise, vorne rechts
eine orange, jede mit einer Welle aus fünf Balken (kurz, mittel, lang, mittel,
kurz) und einem leichten türkisen Glühen —, daneben die Wortmarke
**SPRECHWERK** in fetten, eckigen Großbuchstaben. Unten die Fassungen: dunkle
Kachel, helle Kachel (fast weiß), Mono (dunkle Kachel, weiße Zeichnung) sowie
das Zeichen ohne Kachel, groß und klein.

[ADR 0014](0014-marke-zwei-farben.md) hatte die Farben schon auf Türkis und
Orange gestellt, aber die Formen kamen aus einem ASCII-Raster
(`weppelmann-ui/apps/markenwerk`) und die Kachel war das Achteck des
Generators. Das ist ein anderes Zeichen als das in der Vorlage: dort ist die
Kachel abgerundet, die Blasen sind Konturen mit Stufen (Pixelstil), und die
Wortmarke ist eine gezeichnete Schrift, keine Pixelletterschrift.

Am Bild ausgemessen (Pixel, umgerechnet auf das 512er-Raster; das Zeichen misst
dort 382 × 285):

| Teil | Vorlage | 512er-Raster |
|---|---|---|
| Kachel | 246 × 226 px, Ecken r ≈ 43–48 | 512 × 512, r = 92 |
| Zeichen (beide Blasen) | 382 × 285 px | 382 × 285 |
| Blase hinten (türkis) | 248 × 190 px + Schwanz 47 hoch | 248 × 190, Schwanz 47 |
| Blase vorn (orange) | 222 × 190 px, Versatz 160/48 | 222 × 190, Versatz 160/48 |
| Steg (Kontur) | 31 px | 32 |
| Treppenstufe der Ecken | 15,5 px, 5 Stufen an zwei Ecken | 16, 5 Stufen |
| Welle | 5 Balken, 13–17 px breit, Lücke 12, Höhen 22/48/84/48/22 | 14/15/17/15/14, Lücke 12, gleiche Höhen |
| Schwanz | drei Stufen, Keil 44 breit, 40 tief | 44/29/15 breit, 12–15 hoch |
| Wortmarke | Großbuchstaben 100 hoch, Strich 23, `SPRECHWERK` 909 breit | 100 hoch, Strich 23, 909 breit |
| Buchstabenbreiten | S 74 · P 74 · R 78 · E 63 · C 72 · H 78 · W 119 · K 78 | dieselben, Lücke 15 |

Farben: Türkis `#2EE6C8`, Orange `#F7931E` (beide im Bild als Verlauf
`#01C6BB … #2EF4E0` bzw. `#F8841E … #FEBC3A`; die Tokens nennen je einen
Wert), Kachel dunkel `#131A22` → `#0E1319`, Kachel hell `#FFFDF8` → `#F6F2EA`,
Kachel Mono `#161616` → `#101010`, Zeichnung Mono `#FFFFFF`.

## Entscheidung

### 1. Das Logo wird übernommen, nicht an §8 angepasst

§8 gilt weiter für alles, was aus einem Raster entsteht; **sprechwerk ist die
Ausnahme**: Zeichen, Wortmarke und App-Icon sind so, wie der Nutzer sie gewählt
hat. Die Quelle ist nicht mehr das Raster, sondern die SVG-Datei. Die
Achteck-Kachel, die Pixelletterschrift und `apps/markenwerk` entfallen für
diese Marke.

### 2. Quelle sind die SVG-Dateien, erzeugt wird mit einem Skript

`assets/`:

| Datei | Was |
|---|---|
| `sprechwerk-logo.svg` | App-Icon: Kachel, beide Blasen, Glühen |
| `sprechwerk-zeichen.svg` | dasselbe ohne Kachel, mit Rand für das Glühen |
| `sprechwerk-hell.svg` | helle Fassung (Kachel `#FFFDF8` → `#F6F2EA`) |
| `sprechwerk-mono.svg` | Mono-Fassung (weiße Zeichnung, Trennlinie in der Fläche) |
| `sprechwerk-16.svg` | eigenes Raster für 16 px: 1-px-Umriss, eine Welle je Blase, kein Glühen |
| `sprechwerk-24.svg` | eigenes Raster für 24 px: 2-px-Umriss, drei Balken |
| `sprechwerk-32.svg` | eigenes Raster für 32 px: 2-px-Umriss, fünf Balken |
| `sprechwerk-wortmarke.svg` | Zeichen + `SPRECHWERK`, Buchstaben als Pfade |
| `sprechwerk-schriftzug.svg` | nur die Buchstaben, weiß — im Fenster eingefärbt |
| `sprechwerk-symbolic.svg` | einfarbig weiß, beide Blasen (Schreibtisch-Icon, Mono-Theme) |

Die Buchstaben der Wortmarke sind **Pfade, keine Schrift**: keine
Font-Abhängigkeit, kein Ausweichen auf eine Ersatzschrift, wenn Plex Sans ein
Zeichen nicht hat. Die Blasen sind Rechtecke auf einem 16er-Raster; die Ecken
sind Treppen aus fünf Stufen an der oberen linken und der unteren rechten Ecke
und einer Stufe an den beiden anderen — so steht es in der Vorlage. Der Schwanz
läuft als Treppe aus drei Stufen nach außen-unten aus (hinten nach unten-links,
vorn nach unten-rechts), an der Außenkante bündig: ein zusammenhängender Keil,
kein Tropfen.

Die drei kleinen Fassungen sind **nicht** aus dem großen Zeichen abgeleitet,
sondern Pixel für Pixel gezeichnet: Quelle ist das ASCII-Gitter in
`packaging/icons/klein-gitter.py`, das die SVGs schreibt (`k` Kachel, `t`
Türkis, `o` Orange, `.` nichts; die 1-px-Trennung um die vordere Blase rechnet
das Skript). Die SVGs bestehen nur aus ganzzahligen Rechtecken und tragen
`shape-rendering="crispEdges"`.

`packaging/icons/erzeugen.sh` zeichnet daraus die PNGs
(`packaging/icons/hicolor/<n>x<n>/apps/sprechwerk.png` für 16, 24, 32, 48, 64,
128, 256, 512 und 1024), kopiert `scalable` und `symbolic`, setzt ICO und ICNS
und legt `docs/marke/logo-dark.svg` und `logo-light.svg` für README und Doku
ab. 16, 24 und 32 px kommen aus je einer eigenen Klein-Fassung: bei 16 px ist
ein Balken von 14 Rastereinheiten noch keinen halben Pixel breit, die fünf
Stufen einer Ecke sind ein grauer Fleck und drei Balken berühren den Umriss —
sie lesen sich dann als Klammern `]I[`. Darum hat jede Größe ihr eigenes
Raster mit weniger, dickeren Balken. Gezeichnet wird in der Größe, die im SVG
steht, **ohne** Skalierung; `erzeugen.sh` prüft danach, dass das PNG so viele
Pixel wie die viewBox und höchstens sechs Farben hat — ein geglättetes Bild
hat Dutzende. Gerendert wird mit `resvg`, `rsvg-convert` oder ImageMagick — was
zuerst da ist.

`scripts/marke-bauen.sh` und `weppelmann-ui/apps/markenwerk/marken/sprechwerk.pm`
sind entfallen: das Raster beschreibt dieses Logo nicht, und ein Lauf von
`markenwerk` würde das alte Zeichen zurückbringen.

### 3. Die Farben stehen als Tokens, nicht im Code

`crates/oberflaeche/tokens/` bekommt zwei Grundskalen: `tuerkis` (die Farbe der
hinteren Blase) und `orange` (die der vorderen). `color.marke.sprechwerk.*`
bleibt als Herkunftsnachweis der Marke stehen.

| Theme | Fläche | Akzent (`tuerkis`) | Signal (`orange`) |
|---|---|---|---|
| dark | `#090C10` / `#0E1319` | `#2EE6C8` | `#F7931E` |
| light | `#F6F2EA` / `#FFFDF8` | `#0A6E63` (abgedunkelt) | `#B8640A` (abgedunkelt) |
| mono | `#0A0A0A` / `#101010` | `#E8E6E1` | `#D6D3CE` |

Im hellen Theme ist der Akzent dunkles Türkis, nicht das helle aus dem Zeichen:
`#2EE6C8` erreicht auf Weiß nur 1,7 : 1. Orange wird dort für Text ebenfalls
abgedunkelt (`#F7931E` auf Weiß: 2,1 : 1). Die Kacheln der SVGs tragen die
Flächenfarben der Palette — deshalb prüft der Test nicht nur die beiden
Signalfarben, sondern auch die Kachelwerte.

### 4. Türkis führt, Orange spricht

Das Zeichen erzählt: hinten die türkise Blase (das Programm), vorne die orange
(das Sprechen). Im Fenster heißt das:

* **Türkis** = Akzent: Fokusring, Auswahl, aktiver Raum, Links, der eigene
  Name, Hauptknopf, Rollenabzeichen des Administrators.
* **Orange** = Sprechsignal: das Sprechlicht neben dem Namen, der Rand des
  Kürzels, der Pegel über der Schwelle (Pegelbalken und Pegelregler), das
  Sprechlicht im Sprechtest.

Damit ist Orange keine Zierfarbe, sondern eine Aussage: *diese Stimme geht
gerade durch*. Die Stellen sind `bausteine::pegelbalken`,
`bausteine::pegelregler`, `bausteine::kuerzel`, `ansichten::raeume::sprechlicht`
und das Sprechlicht in `ansichten::einstellungen`.

Kontrast (WCAG 2.1, geprüft in `thema::tests::kontraste_halten`): Text auf
Fläche ≥ 7 : 1, gedämpfter Text ≥ 4,5 : 1, Akzent und Signal gegen die Fläche
≥ 3 : 1, Schrift auf dem Akzent ≥ 4,4 : 1 — in allen drei Themes.

### 5. Die Marke im Kopf ist das Zeichen mit der Wortmarke

Statt `sprechwerk` in der Akzentfarbe stehen in der Kopfleiste von
`crates/fenster` und `crates/server-fenster` das Zeichen (SVG, eigene Farben)
und daneben die Wortmarke als SVG, mit der Textfarbe eingefärbt
(`bausteine::marke`). Im Mono-Theme nimmt auch das Zeichen die Textfarbe an
(`sprechwerk-symbolic.svg`), damit auf einer einfarbigen Fläche nichts Farbiges
steht.

### 6. Ein Test hält Zeichen und Tokens zusammen

`thema::tests::zeichen_und_tokens_tragen_dieselben_farben` liest die SVGs und
verlangt, dass Türkis, Orange, die Kachelfarben und die Mono-Zeichnung der
Tokens darin vorkommen. Wer eine Farbe nur an einer der beiden Stellen ändert,
bekommt einen roten Test.
Zwei weitere Tests halten die kleinen Fassungen:
`kleine_fassungen_tragen_dieselben_farben` verlangt dieselben Tokenwerte auch
in `sprechwerk-16/24/32.svg`, und `kleine_fassungen_liegen_auf_dem_raster`
prüft, dass jede Koordinate ganzzahlig ist, im Feld liegt und `crispEdges`
gesetzt ist — ein halber Pixel ist genau der Fehler, der das Zeichen klein
verschwimmen lässt.

## Was gegenüber der Vorlage fehlt

Die Vorlage ist ein gerendertes Bild, kein Vektor. Übernommen sind Formen,
Maße und Farben; **nicht** übernommen sind zwei Render-Effekte: die Blasen
haben im Bild eine leichte 3D-Kante (hell oben links, dunkel unten rechts) und
einen weichen Schatten, und die Orange deckt die Türkis an der Überlappung
halb durchsichtig. Im SVG sind die Blasen flach und deckend — bei 16 px ist
beides ohnehin nicht zu sehen, und ein Verlauf im Zeichen widerspräche dem
Pixelstil. Das Glühen ist geblieben: es liegt hinter der hinteren Blase und
wird von ihrem Loch ausgespart.

## Verworfene Alternative: an §8 anpassen

Das Logo als ASCII-Raster nachzeichnen, Achteck-Kachel, eine Zeichnungsfarbe,
erzeugt von `apps/markenwerk`. Das wäre regelkonform und würde die
Werkzeugkette einhalten — es ist aber genau das, was der Nutzer nicht wollte:
„1 zu 1“. Ein nachgezeichnetes Achteck-Logo ist ein anderes Logo. Der Preis der
Ausnahme steht unten.

## Kosten

* **Zwei Quellen für dieselbe Marke**, je nach Repo: `apps/markenwerk` erzeugt
  die anderen Anwendungen, sprechwerk hat seine eigenen SVGs. Wer die
  Markenfamilie ändert, muss hier von Hand nachziehen.
* **Die Buchstaben sind Pfade.** Ein Tippfehler in der Wortmarke ist eine
  Zeichnung, kein Text; die Wortmarke lässt sich nicht umbenennen, ohne die
  Pfade neu zu bauen.
* **Nachzeichnen statt kopieren.** Die Vorlage ist ein Bild, kein Vektor: die
  Maße sind ausgemessen und auf runde Werte gebracht (Tabelle oben).
  Pixelgleich ist das Ergebnis nicht — es ist auf ein paar Zehntel genau, und
  bei 16 px bewusst vereinfacht.
* **Die SVGs sind nicht aus dem Raster ableitbar.** `assets/sprechwerk.pm` ist
  weg; ein `markenwerk render sprechwerk` gibt es nicht mehr.
* **Die großen SVGs sind erzeugt.** Sie sind von Hand nachvollziehbar, aber
  entstanden aus einem Einmalwerkzeug, das die Treppen und Pfade gerechnet hat;
  ein Tippfehler in einer Koordinate fällt nur im Bild auf, nicht im Test. Die
  Tests prüfen die Farben und das Raster, nicht die Form. Die kleinen Fassungen
  sind davon ausgenommen: ihr Gitter steht als Text in `klein-gitter.py` und
  lässt sich Zeile für Zeile lesen.

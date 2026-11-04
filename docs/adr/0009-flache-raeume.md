# ADR 0009 — Flache Raumliste statt Baum

**Status:** umgesetzt (2026-11-04)
**Betrifft:** `crates/domain`, `crates/protocol`, `crates/store`, `crates/ipc`,
`crates/desktop`, `ui/`

## Zusammenhang

Das Modell hatte Räume mit Eltern-Kind-Beziehung: beliebig tief, mit
Einrückung, Vererbung von Kanalrechten und einem Menüpunkt „Unterraum
anlegen". In TeamSpeak ist das ein Kernmerkmal — und der Grund, warum
Kanalgruppen, Vererbung und Leistungsrechte niemand mehr erklären kann.

## Entscheidung

**Räume sind flach.** Kein `eltern`, keine Tiefe, kein Verschieben von Räumen,
keine Prüfung auf Unterräume beim Löschen. Die Reihenfolge bestimmt
`reihenfolge`, dann der Name.

Was entfällt:

| Vorher | Nachher |
|---|---|
| `Room.eltern: Option<RoomId>` | — |
| `raum_anlegen(wer, eltern, name)` | `raum_anlegen(wer, name)` |
| `raum_verschieben(...)`, `Error::Kreis` | — |
| `raum_loeschen` verweigert bei Unterräumen (`Error::RaumNichtLeer`) | löscht ohne Prüfung |
| `raumliste()` als Baumdurchlauf | Sortierung nach `reihenfolge`, `name` |
| `Raumansicht.eltern`, `Raum.tiefe` im Fenster | — |

Die Datenbankspalte `channels.channel_parent_id` bleibt bestehen (alte
Datenbanken bleiben lesbar), wird aber nicht mehr gelesen und immer als NULL
geschrieben.

## Warum

1. **Kein Nutzen für die Zielgruppe.** Ein Verein mit fünf bis zwanzig Räumen
   ordnet sie nicht in einen Baum; er will eine Liste, die man ablesen kann.
2. **Kosten ohne Gegenwert.** Tiefe, Einrückung, Zyklusprüfung, Sonderfälle
   beim Löschen — vier Mechanismen, die nur existierten, weil ein Baum da war.
3. **Der Umzug bleibt einfach.** Räume, die im alten Modell Unterräume hatten,
   werden als eigene Räume gelesen; nichts geht verloren.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Baum behalten, nur flach darstellen | die Sonderfälle (Löschen, Verschieben, Tiefe) bleiben im Code |
| Kategorien (wie Discord) ohne Rechtevererbung | ein Zwischending: mehr Modell, kaum Nutzen bei 20 Räumen |
| Baum + Vererbung wie TeamSpeak | genau die Komplexität, von der wir weg wollen |

## Nachtrag: Standardraum

Räume sind flach **und** es gibt immer einen, in dem man sitzt: den
Standardraum (erster Raum der Liste). Beim Verbinden landet jeder dort, „aus dem
Raum werfen" führt dorthin zurück, und ein „Raum verlassen" gibt es nicht mehr.
Das Fenster zeigt den Server-Austritt als eigenen Knopf (Trennen → Serverliste).

## Kosten

* Wer später Gruppen braucht, bekommt sie als **Text im Raumnamen** oder als
  Filter — nicht als Struktur.
* Eine Umkehr (Baum wieder einführen) wäre eine Migration mit Datenverlust an
  Ordnung; sie ist bewusst teuer.

# ADR 0008 — Eingriffe an Personen und Räumen: Menüs, Zustände, Rechte

**Status:** umgesetzt (2026-11-04)
**Betrifft:** `crates/domain`, `crates/protocol`, `crates/server`, `crates/ipc`,
`crates/desktop`, `ui/`, `weppelmann-ui`

## Zusammenhang

Ein Moderator muss eingreifen können: stummschalten, verschieben, aus dem Raum
werfen, vom Server trennen, sperren. In TeamSpeak liegt das in einem
Kontextmenü mit 20 Einträgen, dessen Reihenfolge niemand erklären kann, und in
dem „Kicken" direkt neben „Informationen" steht.

## Was die Recherche sagt

* **Kontextmenüs sind für Nebenaktionen**, nicht für den Hauptweg
  ([NN/G](https://www.nngroup.com/articles/contextual-menus-guidelines/),
  [Apple HIG](https://developer.apple.com/design/human-interface-guidelines/context-menus)).
  Deshalb bleibt „Senden", „Betreten", „Verbinden" sichtbar — nur Eingriffe
  liegen im Menü.
* **Folgenreiche Aktionen weit weg von harmlosen**, mit zusätzlichen Signalen
  ([NN/G: Dangerous UX](https://www.nngroup.com/articles/proximity-consequential-options/)).
  Umsetzung: Trennlinie, Warnfarbe, eigener Dialog mit Grund.
* **Keine Bedienelemente im Menüeintrag** (sie sind nicht tastaturbedienbar) —
  · [Paste Design System](https://paste.twilio.design/components/menu). Wer eine
  Auswahl braucht, bekommt einen Dialog; Untermenüs gibt es nicht.
* **Sprechen, Stumm und Taub sind getrennte Zustände** — wer taub ist, ist auch
  stumm, aber nicht umgekehrt; jedes bekommt ein eigenes Zeichen, und beim
  Sprechen bewegt sich nur der Ring am Namenszeichen
  ([Gryt UI](https://ui.gryt.chat/examples/voice-panel)).

## Entscheidung

### Modell (Server)

| Feld / Aktion | Bedeutung |
|---|---|
| `stumm` | Mikrofon serverseitig gesperrt; der Sprachpfad verwirft Rahmen |
| `taub` | Wiedergabe gesperrt; setzt `stumm` mit |
| `stummschalten(wer, ziel, taub, an)` | Recht `PersonEingreifen`, protokolliert |
| `verschieben(wer, ziel, raum)` | Recht `PersonEingreifen`, protokolliert |
| `aus_raum_werfen(wer, ziel)` | zurück in den Wurzelraum, protokolliert |
| `Trennen` / `MitgliedSperren` | Verbindung schließen (Kennung sperren), protokolliert |

Der Zustand liegt **beim Server**, nicht beim Client: ein Client, der sich selbst
wieder einschaltet, kommt gegen den Server nicht an. Das ist der Unterschied zu
einem reinen Anzeigezustand.

### Oberfläche

* **Rechtsklick auf eine Person** öffnet das Menü; derselbe Inhalt hängt am
  „…"-Knopf, damit Tastatur und Touch denselben Weg haben.
  Dafür wurde `WpMenu` um `v-model:open` und `align` erweitert.
* Reihenfolge: harmlos oben (stumm, verschieben, aus Raum werfen), **Trennlinie**,
  Rollen, **Trennlinie**, dann in Warnfarbe „Vom Server trennen …" und „Sperren …".
* Aktionen mit Eingabe (Grund, Zielraum) öffnen einen Dialog — nichts wird im
  Menü verschachtelt.
* Kennzeichnung: Sprech-Ring am Namenszeichen (`WpAvatar speaking`), Strich für
  stumm (`WpAvatar muted`), gedämpft für taub (`away`), Textzeile nennt es
  zusätzlich („mitglied · taub").
* Sichtbarkeit folgt aus dem Modell: ohne `PersonEingreifen` gibt es kein Menü.

### Bibliothek (weppelmann-ui)

Beim Bauen fehlten drei Dinge, sie sind ergänzt:

1. **Radius-Tokens.** `WpButton` und `WpBadge` benutzten `--wp-radius-md` und
   `--wp-radius-pill`; die Primitive `radius` wurden vom Compiler aber **nie
   ausgegeben** (`BASE_GROUPS` enthielt `radius` nicht). Eine nicht deklarierte
   CSS-Variable ist ungültig — alle Ecken waren eckig, ohne dass es auffiel.
2. **`WpAvatar`** — Namenszeichen mit Sprech-Ring, Stumm-Strich, Abwesenheit.
3. **`WpLevelMeter`** — Pegel mit eingezeichneter Sprechschwelle (für den
   Audiopfad, dort noch nicht angeschlossen).

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Alles im Kontextmenü (wie TeamSpeak) | Hauptwege gehören sichtbar; ein Menü mit 20 Einträgen ist eine Liste, keine Führung |
| Untermenüs für „Verschieben" und „Rolle" | nicht tastaturbedienbar; Dialoge sind ehrlicher und zeigen den Zielraum mit Baumtiefe |
| Stummzustand nur im Client | ein Client kann sich selbst freischalten; der Server muss sperren |
| Kicken ohne Grund | der Grund ist die einzige Erklärung, die der Betroffene bekommt |

## Kosten und offene Punkte

* Kein Mehrfachauswahl-Menü („alle im Raum stummschalten") — kommt, wenn
  gebraucht.
* Kein „Sperre aufheben" in der Oberfläche: dafür braucht es eine Bannliste
  (Menüpunkt „Verwaltung → Sperren"), die noch fehlt.
* Die gezeigten Aktionen sind im Klickdummy vollständig; im echten Client
  funktionieren sie über `crates/desktop` gegen den Server.

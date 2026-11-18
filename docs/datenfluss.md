# Datenfluss: von der Tastatur zum Bild

## 1. Text schreiben

```mermaid
sequenceDiagram
    participant N as Nutzer
    participant UI as crates/fenster (egui)
    participant D as crates/client
    participant I as crates/ipc
    participant A as crates/adapters
    participant S as Server

    N->>UI: tippt, Enter
    UI->>D: Aktion::Befehl(Text { text })
    D->>D: Bund::verschluesseln (Serverschlüssel, Raum, Verfasser)
    D->>A: ClientFrame::Text { e1.<id>.<Geheimtext> }
    A->>S: Rahmen (JSON, Strom)
    S->>S: geheimtext::zerlegen (nur Geheimtext, aktueller Schlüssel)
    S->>S: domain::text_senden (Recht, Länge, Protokoll)
    S-->>A: ServerFrame::Text an alle im Raum (Geheimtext)
    A-->>I: Rahmen
    I->>I: abbilden::verarbeite → Nachricht „wartet"
    D->>D: chat_lesen: Bund::lesen → Text
    I-->>D: melde(Zustand)
    D-->>UI: Meldung::Zustand (weckt das Fenster)
    UI->>UI: render(modell) im nächsten Bild
    N->>UI: sieht die Nachricht
```

Ver- und entschlüsselt wird **im Client-Kern** (`crates/client`), nicht im
Server und nicht in der Oberfläche (ADR 0012). Der Server sieht nur die Hülle; die
Oberfläche bekommt nur das Ergebnis (`inhalt`: Text, wartet, unlesbar, …).
Fehlt der Schlüssel, bleibt eine Nachricht „wartet" und wird lesbar, sobald
ein Mitglied ihn weitergegeben hat — ohne den Verlauf neu zu laden.

### Reaktionen, Effekte, Link-Vorschau (ADR 0012, Stufe 2)

Alles läuft **denselben** Weg wie Text — der Server sieht nur Geheimtext und
weiß nicht, ob darin ein Text, eine Reaktion oder eine Vorschau steckt.

```mermaid
sequenceDiagram
    participant UI as crates/fenster (egui)
    participant D as crates/client
    participant L as crates/linkvorschau
    participant W as fremde Seite
    participant I as crates/ipc
    participant S as Server

    UI->>D: Aktion::Befehl(Vorschau { url }) (erster Link im Entwurf hat sich geändert)
    D->>D: Zustand.entwurf = laedt
    D->>L: Holer::holen (eigene Aufgabe)
    L->>W: GET (Seite, dann og:image)
    L-->>D: Linkvorschau { titel, beschreibung, bild (JPEG, 480 px) }
    D-->>UI: Zustand.entwurf = fertig → Karte über dem Eingabefeld
    UI->>D: Aktion::Befehl(Text { text, effekt })
    D->>S: Inhalt::Text { text, effekt, vorschau } verschlüsselt
    UI->>D: Aktion::Befehl(Reagieren { nachricht, zeichen })
    D->>D: abbilden::meine_reaktion → setzen oder zurücknehmen
    D->>S: Inhalt::Reaktion { ziel, zeichen, weg } verschlüsselt
    S-->>D: ServerFrame::Text (Geheimtext, an alle im Raum)
    D->>I: inhalt_abbilden (prüft Link, Längen, Bild) · reaktionen_zaehlen
    D->>I: effekt_ausloesen (nur bei neu angekommenen Nachrichten)
    D-->>UI: Zustand (nachrichten[].reaktionen, effekt { nr })
```

Nur der **Absender** ruft die Seite ab; Empfänger bekommen die fertige
Vorschau im Umschlag und fragen keine fremde Seite an. Reaktionen sind eigene
Umschläge, die der Client am Ziel zusammenfasst (`reaktionen_zaehlen`, rein,
jedes Mal aus dem ganzen Verlauf). Ein Effekt spielt, wenn `effekt.nr` sich
ändert — die Oberfläche braucht dafür keinen eigenen Zustand.

Der Rückweg über die Meldung ist **kein** Umweg: der Absender bekommt seine
Nachricht vom Server zurück, damit die Zeit und die Reihenfolge stimmen, die der
Server vergeben hat.

## 2. Raum betreten

1. Klick auf einen Raum → `Aktion::Befehl(betreten { raum })`.
2. Server prüft `RaumBetreten`, merkt sich den Raum, liefert den sichtbaren
   Verlauf (Gäste sehen nur, was seit ihrem Betreten geschrieben wurde).
3. `ServerFrame::Betreten { raum, verlauf, anwesen }`.
4. `abbilden::verarbeite` setzt `aktueller_raum`, `nachrichten`, `mitglieder`
   und markiert den Raum als `hier`.
5. Der Raum bekommt zusätzlich ein `Anwesenheit`-Rundruf an die anderen.

**Reihenfolge ist Teil des Vertrags.** Die Antwort `Betreten { anwesend }` ist
eine Momentaufnahme, und der Client **ersetzt** damit seine Mitgliederliste.
Eine Meldung über jemanden, der danach kam, muss deshalb auch danach
ankommen — sonst überschreibt die ältere Aufnahme die neuere Meldung, und der
Späterkommende fehlt dauerhaft (als Schlüsselwart: er bekommt nie einen
Raumschlüssel). Der Server sichert das, indem er Befehl ausführen und Antwort
ins Postfach legen **unter einer Sperre** erledigt (`crates/server/src/netz.rs`).
Geprüft am Draht: `gleichzeitiges_betreten_laesst_niemanden_verschwinden`
in `crates/server/tests/rundlauf.rs` (sechs Clients, zwölf Runden).

## 3. Sprache

```text
Client A: Opus-Rahmen + 6 Byte Kopf → QUIC-Datagramm
Server:   Kopf prüfen, Recht prüfen (Rollen), Raum aus dem Sitzungszustand
         → send_datagram an alle anderen im Raum (kein Puffer, kein Warten)
Client B: Kopf lesen, Jitter-Puffer, dekodieren, ausgeben
```

Verworfen wird bewusst: falscher Kopf, kein Rederecht, kein Raum, voller
Sendepuffer. Ein verspäteter Rahmen ist wertlos.

## 4. Verbindungsabbruch

`Verbindung::empfang()` endet oder meldet einen Fehler → `abbilden::getrennt`
setzt den Zustand auf `Fehler` (oder `Getrennt`), leert Mitglieder und Räume und
meldet den Stand. Das Fenster zeigt den Anmeldebildschirm mit der Meldung — es
gibt keinen halb verbundenen Zustand.

# ADR 0012 — Alles Ende zu Ende: Text, Sprache, Anhänge

**Status:** Stufen 1 und 2 umgesetzt (2026-11-17), Stufe 3 geplant
**Entscheidung (Nutzer, 2026-11-17):** „Das muss in sich geschlossen sein —
wenig Sinn, die Sprache zu verschlüsseln, aber den Text nicht." Lesen dürfen
**die Mitglieder des Servers**, nicht der Betreiber. Wer als Betreiber lesen
will, verbindet sich mit dem Client — dann ist er Mitglied wie jeder andere.
**Ändert:** SPEC §3 „Text bleibt serverseitig lesbar" (Nachtrag 2026-11-17),
ADR 0006 „Nur Sprache" und „je Raum einschaltbar".
**Betrifft:** `crates/protocol` (`geheimtext.rs`, `inhalt.rs`, neue Rahmen),
`crates/adapters` (`serverschluessel.rs`), `crates/linkvorschau` (neu, Stufe 2),
`crates/domain`, `crates/store`, `crates/server`, `crates/ipc`,
`crates/desktop`, `crates/cli`, `ui/`

## Was versprochen wird

Wer den Server betreibt — Datenträger, Datenbank, Sicherungen, Protokoll,
Verwaltung im Browser — **liest keinen Inhalt**: keinen Text, keine Sprache,
keine Datei, kein Bild, keine Reaktion, keine Link-Vorschau.

| Sieht der Server | Sieht er nicht |
|---|---|
| wer wann in welchem Raum ist und spricht | was gesagt wird |
| dass jemand schreibt, wie lang (ungefähr), wann | was geschrieben wird, welche Art (Text, Reaktion, Anhang) |
| Größe und Zeitpunkt eines Anhangs | Name, Typ, Inhalt, Vorschaubild |
| Namen, Rollen, Kennungen der Mitglieder | — |

**Nicht** versprochen: Schutz vor einem Mitglied (es kann lesen und
weitergeben), vor einem kompromittierten Gerät, vor einem Betreiber, der sich
selbst als Mitglied anmeldet (er ist dann Teilnehmer, nicht Mitleser; seine
Kennung steht in der Mitgliederliste wie jede andere).

**Ein umgebauter Server** kann dasselbe heimlich versuchen: ein erfundenes
Mitglied anmelden und um den Schlüssel bitten lassen. Davor schützt keine
Gruppenverschlüsselung, deren Mitgliederliste vom Server kommt — Signal und
Matrix lösen das mit einem Abgleich der Kennungen außerhalb des Servers
(Sicherheitsnummern, Geräteprüfung). Hier: das erfundene Mitglied stünde in der
Mitgliederliste und im Prüfprotokoll; ein Abgleich der Kennungen ist **noch
nicht gebaut** (Kandidat für später). Das gilt für Sprache genauso.

## Entscheidung

### 1. Ein Serverschlüssel für alles Gespeicherte

Ein symmetrischer Schlüssel `S` (32 Byte) **je Server**, gemeinsam für alle
Mitglieder. Damit lesen auch später Dazugekommene den Verlauf (wie in einem
Vereinsforum oder Discord) — das ist die Zusage „lesen dürfen die Mitglieder".

* **Kennung des Schlüssels:** `id = hex(SHA-256("sprechwerk serverschluessel v1" ‖ S)[0..16])`.
  Wer `S` bekommt, prüft die Kennung nach. Ein Mitglied kann deshalb keinen
  falschen Schlüssel unter einer richtigen Kennung unterschieben.
* **Erzeugt vom ersten Client**, der einen Server ohne Schlüssel antrifft
  (`ServerschluesselAnlegen { id }`). Der Server nimmt **den ersten** an und
  kennt danach nur die Kennung. Kommen zwei gleichzeitig, verliert der zweite
  und verwirft seinen.
* **Weitergegeben von Mitglied zu Mitglied, vermittelt vom Server:** Jeder
  Verbundene meldet, welche Kennungen er hält (`ServerschluesselVorhanden`).
  Fehlt einem Verbundenen ein Schlüssel, bittet der Server **einen** Halter
  (den mit der kleinsten Mitgliedskennung) mit
  `ServerschluesselAnfrage { fuer, kennung, id }`. Der Halter verpackt den
  Schlüssel für diese Kennung wie die Raumschlüssel der Sprache (`Umschlag`:
  X25519 ephemer + statisch, HKDF, ChaCha20-Poly1305) und **unterschreibt**
  mit Ed25519. Der Server leitet nur weiter.

  *Warum vermittelt der Server?* Er weiß, wer verbunden ist; ein Client kennt
  nach dem Betreten eines Raums nur dessen Anwesende. Eine Regel „alle rechnen
  aus, wer verteilt" bräuchte die vollständige Liste der Verbundenen bei jedem
  Client. Am Schutz ändert das nichts — die Liste käme ohnehin vom Server.
* **Fehlt einem Client etwas**, meldet er sich alle 5 s neu; der Server bittet
  höchstens alle 5 s je Mitglied und Schlüssel einen Halter.
* **Neuer Schlüssel nach einer Sperre.** Der Server setzt „aktuell" zurück,
  der nächste Client erzeugt `S'`. Alte Schlüssel bleiben gültig zum Lesen
  (alle Kennungen stehen in der Liste); geschrieben wird nur mit dem aktuellen.
  Die gesperrte Person bekommt `S'` nie.
* **Im Client gespeichert** neben der Anmeldeschlüsseldatei
  (`serverschluessel.json`, 0600), je Server nach Zertifikat-Fingerabdruck.
  Geht die Datei verloren, gibt ein anderes Mitglied den Schlüssel erneut.

**Preis:** Wer **allein** online ist und den Schlüssel noch nicht hat (neues
Mitglied, neues Gerät ohne die Datei), kann erst lesen und schreiben, wenn ein
Halter online kommt. Die Oberfläche sagt das. Gehen **alle** Halter verloren
(alle Geräte weg), ist der Verlauf unlesbar — auch für den Betreiber. Das ist
die Kehrseite des Versprechens.

### 2. Jede Chatnachricht ist ein Umschlag mit Art

Der Server speichert und verteilt nur

```text
e1.<id 32 hex>.<hex(nonce 12 ‖ ChaCha20-Poly1305(inhalt))>
```

Zusatzdaten (AAD): `raum ‖ verfasser ‖ id`. Der Server kann eine Nachricht
also weder in einen anderen Raum hängen noch einem anderen Verfasser
zuschreiben, ohne dass sie unlesbar wird.

Der Inhalt ist versioniertes JSON (`sprechwerk_protocol::inhalt::Inhalt`):

| Art | Felder | Stufe |
|---|---|---|
| `text` | `text`, optional `effekt`, optional `vorschau { url, titel, beschreibung, bild }` | 1, 2 |
| `reaktion` | `ziel` (Nachricht), `zeichen`, `weg` | 2 |
| `anhang` | `text`, `dateien [{ id, name, typ, groesse, schluessel, pruefsumme, vorschau }]` | 3 |

Eine neue Art braucht **keine** Protokolländerung — der Server sieht nur
Geheimtext. Ein älterer Client zeigt eine unbekannte Art als „neuere Version
nötig", statt sie falsch darzustellen.

*Umgesetzt anders als zuerst geplant (Stufe 2):* Effekt und Vorschau sind
**optionale Felder am Text**, keine eigenen Arten `effekt` und `link`. So kann
eine Nachricht beides haben, und ein Client der Stufe 1 liest sie noch als Text
(unbekannte Felder übergeht er) — er zeigt nur keinen Effekt und keine Karte.
Ein unbekannter Effekt wird zu `unbekannt` und fällt weg; der Text bleibt.

**Was ein Mitglied schickt, wird geprüft** (`ipc::abbilden::inhalt_abbilden`):
ein Vorschau-Link muss `http(s)` sein, Titel und Beschreibung werden gekürzt,
das Bild muss Base64 sein; eine Reaktion braucht eine gültige
Nachrichtenkennung und ein kurzes Zeichen ohne Leerraum. Der Schlüssel sagt
nur, dass ein Mitglied es geschickt hat — nicht, dass es harmlos ist.

**Reaktionen** sind eigene Umschläge, die auf eine Nachricht zeigen; der Client
fasst sie zusammen. Der Server kann sie nicht zählen — er weiß nicht, dass es
Reaktionen sind.

**Link-Vorschau:** Der **Absender** ruft die Seite ab (Titel, Beschreibung,
Bild), packt die Vorschau in den Umschlag. Empfänger rufen nichts ab und
verraten ihre Adresse keiner fremden Seite; der Server sieht nichts. Wie
Signal. *Nutzerentscheidung 2026-11-17.*

Umsetzung in `crates/linkvorschau` (eigenes Crate, damit der Server weder
HTTP-Client noch Bilddecoder mitbaut):

* Abruf mit `reqwest` (rustls, Anbieter `aws-lc-rs` wie quinn, Zertifikate des
  Systems), 8 s Frist, höchstens 5 Weiterleitungen, höchstens 512 KiB HTML
  (der Kopf steht vorn) und 8 MiB Bild; neutrale Kennung
  `sprechwerk-linkvorschau`, keine Cookies.
* Gelesen werden nur `<title>` und `<meta>` im Kopf (`og:*`, `twitter:*`,
  `description`) — ein kleiner Leser statt eines HTML-Parsers.
* Das Bild wird mit `image` dekodiert (Grenzen gegen Pixelbomben), auf
  höchstens 480 px verkleinert, Durchsichtiges auf Weiß gesetzt und als JPEG
  neu geschrieben (≤ 120 KiB): kein Originalbild, keine Metadaten.
* Der Entwurf zeigt die Vorschau über dem Eingabefeld (`Zustand.entwurf`); sie
  lässt sich wegklicken und geht nur mit, wenn ihr Link noch im Text steht.
  Ein Link wird je Entwurf nur einmal abgerufen.
* **Abschaltbar** (Einstellungen → Chat): dann wird nichts abgerufen, Links
  bleiben anklickbar. Anklicken öffnet den **Browser des Systems**
  (`tauri-plugin-opener`, aufgerufen aus Rust nach der Prüfung auf `http(s)`),
  nie das Fenster selbst.

**Effekte** (Konfetti, Herzen, Beben) spielen einmal, wenn eine Nachricht
**neu ankommt** — nicht beim Laden des Verlaufs, nicht nach einem
Raumwechsel. `Zustand.effekt { nachricht, effekt, nr }`: die Oberfläche spielt,
wenn `nr` sich ändert; „noch einmal" zählt `nr` hoch. Mit
`prefers-reduced-motion` spielt nichts, der Effekt steht nur als Wort am
Beitrag.

### 3. Anhänge (Stufe 3)

* Der Client verschlüsselt jede Datei mit einem **eigenen** Zufallsschlüssel
  (ChaCha20-Poly1305 in Blöcken zu 64 KiB, je Block eigener Nonce — große
  Dateien passen nicht in den Speicher). Der Dateischlüssel steht **im
  Nachrichtenumschlag**, nicht beim Server.
* Vorschaubilder (Bild, erstes Videobild) erzeugt der **Absender**; sie sind
  ein kleiner, ebenso verschlüsselter Anhang.
* Hochladen und Herunterladen auf einem **eigenen QUIC-Strom** derselben
  Verbindung, nicht über den Steuerstrom und nicht über Datagramme (AGENTS.md
  §9: „nie Dateien über die Sprachverbindung"). Der Server legt Blöcke unter
  `daten/anhaenge/<id>` ab und gibt sie nur an Mitglieder aus.
* Vorschau-Fenster für Bild und Video: der Client entschlüsselt in den Speicher
  und zeigt es; Herunterladen speichert die entschlüsselte Datei.
* **Keine Größengrenze** in der Vorgabe (*Nutzerentscheidung*); der Betreiber
  kann je Datei und gesamt begrenzen (`[anhaenge] max_mb`, `gesamt_mb`).
  Ohne Grenze kann ein Mitglied den Datenträger füllen — das steht in der
  Beispielkonfiguration.
* Löschen einer Nachricht löscht ihre Anhänge.

### 4. Sprache immer verschlüsselt

Der Schalter „Raum verschlüsseln" entfällt: **jeder** Raum ist Ende zu Ende
verschlüsselt, bestehende werden beim Start umgestellt. Die Schlüsselrunde je
Raum aus ADR 0006 bleibt (Wart, Epoche, Übergang — gemessen und geprüft): für
Sprache ist ein Schlüssel nur der **Anwesenden** richtig, weil Gesprochenes
nicht gespeichert wird und wer geht, nicht weiter mithören soll.

### 5. Altbestand

Beim ersten Start mit dieser Version löscht der Server **alle Nachrichten, die
kein Geheimtext sind**, schreibt es ins Prüfprotokoll und verdichtet die
Datenbank (`VACUUM`, `secure_delete`), damit der Klartext nicht in freien
Seiten liegen bleibt. *Nutzerentscheidung 2026-11-17.* Sicherungen von vorher
bleiben unberührt — wer sie nicht mehr will, löscht sie.

Danach nimmt der Server **nur** Geheimtext an: ein Text ohne `e1.` wird
abgelehnt. Ein alter Client kann so nicht versehentlich Klartext ablegen; die
Protokollversion steigt auf 2, ein alter Client bekommt „Protokollversion
passt nicht".

Die Datenbank wird mit 0600 angelegt (vorher 0644 — für alle Benutzer des
Rechners lesbar).

## Verworfen

| Alternative | Warum nicht |
|---|---|
| Schlüssel je Raum auch für Text (wie Sprache) | Wer später in den Raum kommt, läse den Verlauf nicht; Räume sind offen (ADR 0009), ein Schlüssel je Raum schützt hier vor niemandem zusätzlich |
| MLS (RFC 9420) / Megolm | richtig für große, wechselnde Gruppen mit Forward Secrecy je Nachricht; hier soll gerade der **ganze** Verlauf für alle Mitglieder lesbar sein. Eine Bibliothek dieser Größe für eine schwächere Zusage lohnt nicht |
| Server hält den Schlüssel „getrennt" (HSM, Datei) | dann liest der Betreiber — genau das soll nicht sein |
| Dateien per HTTP vom Server | zweiter Weg, eigene Anmeldung; der QUIC-Strom hat Anmeldung und Verschlüsselung schon |
| Link-Vorschau durch jeden Empfänger | jede fremde Seite erführe die Adresse aller Leser |
| GIF-Suche (Giphy) | fremder Dienst mit Schlüssel und Suchbegriffen; *vorerst nicht* (Nutzer) |

## Kosten

* Text wird um rund 2 × (28 + Länge) + 36 Zeichen größer (Hex). Bei 4000
  Zeichen Höchstlänge passt das bequem in einen Rahmen (1 MiB).
* Der Server kann nicht mehr nach Inhalt suchen, filtern oder Vorschauen bauen
  — Suche muss später im Client laufen.
* Neues Mitglied allein online: wartet auf einen Halter (siehe oben).
* Jede Reaktion ist eine eigene gespeicherte Nachricht (zählt zur Aufbewahrung
  und zum Verlauf). Reaktionen auf Nachrichten außerhalb des geladenen
  Verlaufs fallen still weg.
* Link-Vorschau: die Seite sieht die Adresse des **Absenders**; ein Bild
  macht den Umschlag bis zu ~160 KiB groß (Base64). Neue Abhängigkeiten im
  Fenster: `reqwest`, `image`, `url`, `base64` (in `crates/linkvorschau`) und
  `tauri-plugin-opener`.
* Protokollversion 2: alte Clients verbinden sich nicht mehr.

# sprechwerk — Festlegungen

Arbeitstitel: **sprechwerk** (umbenennbar; Verzeichnis und Crates heißen so,
bis du einen Namen nennst).

Dieses Dokument hält jede Festlegung fest: was du entschieden hast, was ich
entschieden habe, wo die Grenze liegt. Es ist die Grundlage für alles Weitere —
wer später etwas ändern will, ändert hier zuerst.

Legende: **[P]** = deine Entscheidung aus dem Gespräch.
**[A]** = meine Entscheidung, weil du sie delegiert hast; änderbar, aber dann
muss der betroffene Code mit.

---

## 1. Zweck und Zielgruppe

**[P] Zielgruppe:** dieselbe wie bei TeamSpeak 3 — Leute, die in Gruppenräumen
sprechen, mit Rollen, Rechten und Text. Ein Verein, ein paar hundert
Verbundene, keine Firma mit Zehntausenden.

**[P] Grund zum Umstieg:** nicht neue Funktionen, sondern
*Struktur, die nicht verrottet* (D) und *Betrieb, der beherrschbar ist* (E).

**[A] Konsequenz:** Das Produkt wird nicht über Funktionsumfang verkauft,
sondern über Ordnung und Nachvollziehbarkeit. Ein Feature, das die Ordnung
verschlechtert, kommt nicht hinein.

## 2. Betrieb

**[P] Selbst gehostet, Docker, Konfiguration von außen.**
Ein Container, ein Volume, Konfiguration über Datei und Umgebungsvariablen.

**[P] Keine Betriebs-Weboberfläche.** Konfiguration ist die Datei; Räume und
Rollen werden im Client verwaltet. Dazu kommt eine schlanke Diagnose (wer ist
verbunden, letztes Protokoll, letzte Sicherung) — **[A]** als Unterbefehl des
Servers, nicht als Weboberfläche.

**[P] Sicherung: automatisch nach Plan**, die letzten N werden behalten, dazu
ein Handbefehl.

**[A] Sicherungsformat: Text (JSON), versionsunabhängig.** Die Datenbank ist
Betriebsmittel, der Text ist Wahrheit für Export und Import. Beim Einspielen
wird die Datenbank neu aufgebaut. Der Export trägt ein Feld `format_version`;
Lesen muss jede künftige Version können, Schreiben immer die neueste.

**[P] Export enthält alles**, auch abgelegte Dateien (ein Archiv). Für
Version 1 ist das theoretisch, weil Dateien nicht drin sind.

**[A] Update:** Beim Start wird die Datenbankversion geprüft, bei Abweichung
migriert, vorher automatisch eine Sicherung geschrieben. Rückweg: Sicherung
einspielen. Ein echtes Downgrade (Rückwärtsmigration) gibt es nicht — das ist
der bewusst gewählte Preis.

**[A] Ausfall beim Update:** kurz erlaubt (Container stoppen, starten).
Übergangsfreie Updates sind erst nötig, wenn mehrere Instanzen im Spiel sind.

## 3. Umfang von Version 1

**[P] Drei Säulen: Sprache, Text, Rechte.** Alles andere ist „später", nicht
„gestrichen": Dateien, Status/Abwesenheit, Bildschirm teilen, Video,
Hintergrundeffekte, Emojis/Reaktionen, Suche.

**[A] Ende-zu-Ende je Raum:** Sprache kann je Raum Ende zu Ende verschlüsselt
werden (ADR 0006, umgesetzt). Text bleibt serverseitig lesbar — die Zusage gilt
nur für Sprache. Kosten: 16 Byte je Rahmen.

**[P] Aufnahme-Schutz:** Rauschunterdrückung und Noisegate sind eigene Schalter
(Vorgabe an). Monitore (`.monitor`-Quellen) sind **keine** Mikrofone und werden
abgelehnt — sonst hört man die eigene Wiedergabe. Siehe `docs/AUDIO.md`.

**[P] Sprache schmal:** Gruppenräume, Push-to-Talk, Stummschalten, einer
spricht — alle hören. Kein Flüstern, keine 3D-Position, keine Aufnahme.
Der Server mischt nichts, er verteilt.

**[P] Text dauerhaft gespeichert** (kein Verfall). Also: Löschkonzept,
Aufbewahrung auf Wunsch, Rechte auf fremde Nachrichten.

**[P] Ein Raum ist beides:** Sprachraum mit Text darunter (wie TS3/Discord).

**[P] Ein Standardraum.** Wer den Server betritt, landet dort — es gibt keinen
Zustand „in keinem Raum". Ein Raum lässt sich deshalb nicht „verlassen"; man
wechselt nur. Wer den Server verlassen will, trennt die Verbindung (Knopf oben
rechts) und landet auf der Serverliste.

**[P] Flache Raumliste**, keine Unterräume, keine Kategorien. Räume entstehen
durch Rollen (Administrator, Moderator), nicht nur durch den Betreiber.
Begründung: Unterräume brachten Vererbung, Einrückung und Sonderfälle — und
niemand hat sie vermisst (Entscheidung 2026-11-04, ADR 0009).

## 4. Identität und Zugang

**[P] Keine Konten.** Die Kennung entsteht auf dem Gerät; Rechte hängen an ihr.

**[P] Kennung ist mitnehmbar** (Export/Import). Damit ist sie ein Schlüssel:
verlierbar, kopierbar. **[A]** Die Kennung ist kein Zufallswert, sondern aus
einem Schlüsselpaar abgeleitet (Ed25519). Der Server prüft bei jeder
Verbindung eine Signatur — Besitznachweis statt Inhabertoken. Der Export ist
die Schlüsseldatei.

**[A] Umzug wird protokolliert** (Aktion `member.key_imported`), weil er der
einzige Weg ist, eine Identität zu übernehmen.

**[P] Zugang wie TS3:** freie Adresse, optional ein Serverpasswort. Kein
Einladungszwang, kein Warteraum.

**[P] Sperren wie TS3:** Kennung **und** Adresse. Adressen werden gespeichert,
also mit Aufbewahrungsfrist und im Export enthalten.

## 5. Rechte

**[P] Vier feste Rollen:** Administrator, Moderator, Mitglied, Gast. Die Rechte
sind im Programm festgelegt und dokumentiert, nicht konfigurierbar.

**[P] Rollen gelten global**, nicht je Raum. Keine Kanalgruppen, keine
Vererbung, keine Leistungsrechte.

**[A] Rechtekanon** (das ist die vollständige Liste, mehr gibt es nicht):

| Recht | Administrator | Moderator | Mitglied | Gast |
|---|---|---|---|---|
| Raum betreten | ja | ja | ja | ja |
| Text lesen (auch Verlauf) | ja | ja | ja | nur im Augenblick |
| Text schreiben | ja | ja | ja | ja |
| Sprache senden | ja | ja | ja | nein |
| Raum anlegen/umbenennen/verschieben/löschen | ja | ja | nein | nein |
| Nutzer verschieben, stummschalten, rauswerfen | ja | ja | nein | nein |
| Nutzer sperren | ja | nein | nein | nein |
| Rollen vergeben | ja | nein | nein | nein |
| Prüfprotokoll lesen | ja | ja | nein | nein |
| Einstellungen ändern | ja | nein | nein | nein |

**[A] „Verlauf lesen" für Gäste:** nur was während ihrer Anwesenheit
geschrieben wurde. Alle anderen sehen den ganzen Verlauf des Raums, den sie
betreten dürfen. Begründung: der Gast ist der einzige, dessen Rolle „nur mal
vorbeischauen" bedeutet — und Text ist dauerhaft.

**[A] Wer moderiert wen:** ein Moderator darf alles, was ein Mitglied darf,
plus Eingriffe in Räume und Personen. Ein Administrator darf zusätzlich
Rollen, Sperren und Einstellungen. Es gibt keine Rangordnung *innerhalb* einer
Rolle — zwei Moderatoren sind gleich.

## 6. Text

**[P] Text ist dauerhaft.** Also:
**[A] Löschen:** der Verfasser darf seine eigene Nachricht löschen (dann bleibt
ein Platzhalter „gelöscht", damit der Verlauf lesbar bleibt); ein Moderator
darf jede Nachricht löschen, hart, mit Protokolleintrag.
**[A] Aufbewahrung:** standardmäßig unbegrenzt, im Container einstellbar
(`retention_days`, 0 = unbegrenzt).
**[A] Nachrichtenumfang:** reiner Text, Länge begrenzt (Standard 4 000
Zeichen). Keine Formatierung, keine Anhänge, keine Reaktionen in Version 1.

## 7. Technik

**[P] Eigene Protokollsprache**, kein TS3-Protokoll. Bestehende TS3-Clients
können nicht beitreten — der Umzug bedeutet einen neuen Client. `ts3rust` dient
als Nachschlagewerk, nicht als Codebasis.

**[P] Transport: QUIC.** TLS 1.3 eingebaut, Ströme für Steuerung/Text, QUIC-
Datagramme für Sprache, WebTransport als spätere Tür für Browser.

**[P] Installierter Client** (Rust, pro System gebaut). Browser später
vielleicht, über WebTransport.

**[A] Aufteilung in Crates:**

```
domain   Zustand: Räume, Rollen, Mitglieder, Nachrichten, Prüfprotokoll — kein Netz
protocol Sprache auf der Leitung: Rahmen, Serialisierung, Versionierung
adapters Netz: QUIC-Verbindung, Anmeldung, Beweis (Ed25519); später Audio
ipc      Vertrag zum Fenster: Befehle, Ereignisse, Modellstand
store    SQLite + JSON-Export/-Import + Migrationen
server   QUIC-Listener, Befehlsausführung, Sicherung, Konfiguration
cli      Referenzclient (zum Prüfen und für die Kommandozeile)
desktop  Fensteranwendung (Tauri 2, eigene Workspace) + ui/ (Vue 3 + TS)
```

**[A] Sprache auf der Leitung:** JSON-Frames, längenpräfixiert (4 Byte,
big-endian), über einen bidirektionalen QUIC-Strom. Sprache als QUIC-Datagramm
mit eigenem, kleinem Kopf.

**[A] Verschlüsselung:** nur TLS 1.3 aus QUIC. Kein eigenes Krypto, keine
 eigene Schlüsselableitung. Für den Besitznachweis der Kennung Ed25519.

**[A] Kennungen:** Räume und Mitglieder mit zufälliger 128-Bit-Kennung (UUID),
nach außen als Text. Keine Wiederverwendung.

**[A] Größenordnung:** ein Prozess, ein Container, einige hundert Verbundene,
Dutzende Räume. Bei 50 Sprechenden im selben Raum sind es 2 500 Rahmen je
Sekunde — das muss die Verteilung tragen, aber nicht mehr.

## 8. Prüfprotokoll

**[P] Ein Protokoll, kein Rückgängig, kein Abspielen.** Jede Änderung wird
zusätzlich weggeschrieben: wer, wann, was, vorher/nachher.

**[A] Pflichtfelder je Eintrag:** Zeitpunkt (UTC), Handelnder (Kennung und
Name), Aktion, betroffenes Objekt, alter und neuer Zustand als Text, Ergebnis
(erlaubt/abgelehnt). Abgelehnte Versuche werden **auch** protokolliert —
gerade die sind interessant.

**[A] Lesbar für Administrator und Moderator**, filterbar nach Zeitraum,
Handelndem und Objekt. Aufbewahrung wie Text (unbegrenzt, einstellbar).

## 9. Grenzen, die bewusst so sind

* Kein Kontosystem heißt: ein Gerät, eine Stimme. Wer sein Gerät teilt, teilt
  seine Rechte.
* Sperren treffen hinter gemeinsamer Adresse Unbeteiligte. Das ist der Preis
  für „Sperren wie TS3".
* Kein Rückwärtsmigration beim Update.
* Keine Formatierung, keine Anhänge, kein Video in Version 1.
* Keine Betriebs-Weboberfläche: wer einen Raum umbenennen will, braucht den
  Client.

## 10. Nachtrag: Entscheidungen aus der Umsetzung

Beim Schreiben des Codes waren weitere Festlegungen nötig. Alle sind **[A]**.

| Frage | Entscheidung | Warum |
|---|---|---|
| Wer öffnet den Steuerstrom? | der Server | Ein QUIC-Strom wird der Gegenstelle erst mit den ersten Bytes sichtbar; öffnet der Client und wartet auf die Beweisaufforderung, während der Server auf `accept_bi` wartet, stehen beide still (gemessen: genau das passierte im ersten Lauf) |
| Kennungen auf der Leitung | Text (`raum_3f2a…`) | JSON kennt kein 128-Bit-Integer; ein lesbarer Wert hilft beim Fehlersuchen mehr als 16 gesparte Byte |
| Erster Ankömmling | wird Administrator | ohne Konten braucht es einen Weg zum ersten Verwalter; protokolliert als `erster_beitritt` |
| Standardrolle neuer Mitglieder | `mitglied`, konfigurierbar | „Gast" wäre sicherer, aber ein Server, auf dem niemand sprechen darf, wird nicht benutzt; wer zu ist, stellt `standard_rolle = "gast"` |
| Nachricht löschen | Verfasser weich (Platzhalter), Moderator hart | der Verlauf bleibt lesbar, und niemand kann nachträglich ein Gespräch verfälschen |
| Raum löschen | nur ohne Unterräume, Nachrichten des Raums werden mitgelöscht | erst aufräumen, dann löschen — sonst entstehen Waisen |
| Anwesende beim Löschen | landen im Wurzelraum | niemand darf in einem gelöschten Raum sitzen bleiben |
| Letzter Administrator | kann nicht herabgestuft werden | sonst ist der Server unverwaltbar und niemand kann es rückgängig machen |
| Zertifikat | selbst signiert, Fingerabdruck beim Start | eine Zertifizierungsstelle ist für selbst gehostete Instanzen nicht praktikabel; Scheinsicherheit wäre schlimmer |
| Speichern | nach jeder Änderung vollständig (Write-Through) | einfach und ausfallsicher bei dieser Größe; bei Bedarf später gezielte Schreibvorgänge |
| Sicherung im Betrieb | `VACUUM INTO` plus JSON-Export | eine kopierte SQLite-Datei im Betrieb ist manchmal heil, manchmal nicht |

## 11. Nächste Schritte

1. ~~`core`~~ — erledigt (28 Tests).
2. ~~`proto`~~ — erledigt (8 Tests).
3. ~~`store`~~ — erledigt (7 Tests).
4. ~~`server`~~ — erledigt (Konfiguration, TLS, QUIC, Sicherung, Sperren).
5. ~~`cli`~~ — erledigt (Referenzclient, interaktiv und `--einmal`).
6. ~~Sprachpfad~~ — erledigt (Datagramme, Rederecht, Verteilung nur im Raum).
7. Offen: Audio im Client (Opus-Aufnahme/wiedergabe), Fensteranwendung,
   Dateien, Status, Video/Bildschirm, Suche, Emojis.
8. Offen: gestaffelte Speicherung, wenn die Vollschreibung zum Flaschenhals
   wird; Wiederherstellung aus Sicherung als Unterbefehl.

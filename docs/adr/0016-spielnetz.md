# ADR 0016 — Spielnetz: Spiele im Raum freigeben

**Status:** angenommen (2026-11-23), Umsetzung in Stufen (siehe unten)
**Entscheidung (Nutzer, 2026-11-23):** „Ein Hamachi-Netz, damit man
untereinander spielen kann. Muss aktiv ausgelöst und aktiv zugestimmt werden,
der Server dient als Anmeldeknoten. Ein dediziertes Modul, das auf Server und
Client ab- und anschaltbar ist."
**Betrifft:** `crates/spielnetz` (neu), `crates/server` (`konfig.rs`,
`netz.rs`, Anschluss an `kern.rs`), `crates/domain` (`role.rs`: ein Recht),
`crates/client`, `crates/ipc`, `crates/fenster`, `crates/server-fenster`
**Ändert:** nichts an Sprache, Text oder Anhängen. ADR 0012 („der Server liest
nicht mit") gilt auch hier.

## Anlass

Vereine, die zusammen sprechen, wollen auch zusammen spielen. Heute braucht
das Portweiterleitung am Router oder ein zweites Werkzeug (Hamachi, ZeroTier,
Tailscale) mit eigenem Konto. sprechwerk hat schon alles, was so ein Werkzeug
braucht: Kennungen (Ed25519), einen Knoten, den alle erreichen, und Räume, die
sagen, wer zusammengehört.

## Entscheidung

### 1. Portfreigabe statt virtueller Netzwerkkarte (Stufe 1)

Ein Gastgeber gibt **einen Port** seines Rechners frei (z. B. Minecraft,
25565/tcp). Wer im selben Raum zusagt, bekommt diesen Port auf
`127.0.0.1` bei sich gespiegelt und verbindet das Spiel mit `localhost:25565`.

Das läuft vollständig im Programm, braucht weder Treiber noch Adminrechte
und funktioniert unter Linux, Windows und macOS gleich. Es reicht für alle
Spiele, bei denen man eine Adresse eingibt.

**Nicht** abgedeckt: Spiele, die sich nur per Broadcast im LAN finden. Dafür
bräuchte es eine virtuelle Netzwerkkarte (Stufe 3).

### 2. Ein eigenes Modul, eigene Verbindung

* Eigenes Crate `crates/spielnetz`. Server und Client binden es an, es kennt
  weder `Kern` noch `Anwendung` (Anschluss über ein Trait, siehe §6).
* Eigene QUIC-Verbindung zum **selben UDP-Port**, unterschieden am ALPN:
  `sprechwerk/1` bleibt die Sitzung, `sprechwerk-spielnetz/1` ist das Modul.
  Spielverkehr berührt damit weder den Steuerstrom noch die
  Sprachdatagramme, und die Staukontrolle der Sprache sieht ihn nicht.
* Dasselbe Zertifikat, derselbe angeheftete Fingerabdruck (TOFU) wie die
  Sitzung, dieselbe Anmeldung (Beweisaufforderung mit Nonce, Ed25519).

### 3. Ab- und anschaltbar

| Wo | Schalter | Vorgabe | Wirkung, wenn aus |
|---|---|---|---|
| Server | `[spielnetz] aktiv`, `SPRECHWERK_SPIELNETZ_AKTIV`, Schalter im Server-Fenster | **aus** | ALPN `sprechwerk-spielnetz/1` wird nicht angeboten, der Handschlag scheitert. Kein Code des Moduls läuft. |
| Client | Einstellung „Spielnetz", gespeichert im Profil | **aus** | Keine zweite Verbindung, keine Angebote sichtbar, kein lokaler Port offen. |

Der Client erfährt am Handschlag, ob der Server das Modul anbietet. Er zeigt
dann „Dieser Server bietet kein Spielnetz an“, statt es zu verstecken.

### 4. Zustimmung: zweimal aktiv

1. **Gastgeber** wählt „Spiel freigeben“, gibt Name, Port und TCP/UDP an und
   liest: „Alle im Raum *X* können auf Port *N* deines Rechners zugreifen,
   solange die Freigabe läuft.“ Erst dann geht das Angebot an den Server.
2. **Teilnehmer** sieht das Angebot im Raum und wählt „Beitreten“. Erst dann
   öffnet sein Client `127.0.0.1:N`.

Ende, ohne dass jemand nachdenken muss:

* Gastgeber beendet oder entfernt einen Teilnehmer,
* Gastgeber oder Teilnehmer verlässt den Raum oder trennt die Verbindung,
* Modul wird auf Client oder Server ausgeschaltet.

### 5. Ende zu Ende, Server nur Vermittler

Stufe 1 leitet **alles über den Server**. Dafür sieht kein Spieler die
öffentliche IP eines anderen. Damit der Server nicht mitliest:

* Je Paar (Gastgeber, Teilnehmer) ein gemeinsames Geheimnis aus X25519 der
  beiden Kennungen (`oeffentlich_aus_kennung`, wie ADR 0012).
* HKDF-SHA256-Extract mit Salz = Angebotsnummer ergibt den Paarschlüssel.
* **Jede Seite bringt für jede Sitzung eigenen Zufall mit.** Sonst ergäbe ein
  zweites Beitreten zu derselben Freigabe dieselben Schlüssel mit Zählern ab
  0, und ein umgebauter Server könnte durch Wiedereinspielen eines alten
  Salzes erzwingen, dass eine Seite Nonces wiederholt. Bei ChaCha20 heißt das:
  Klartext fällt heraus.
* **TCP:** Beide Seiten schicken als erste Bytes ihrer Richtung 16
  Zufallsbytes. Die Schlüssel je Richtung entstehen aus beiden Salzen
  (`Info …tcp hin` bzw. `…tcp her`). Der Teilnehmer sendet erst, wenn das Salz
  des Gastgebers da ist (eine Laufzeit mehr beim Verbindungsaufbau). Danach
  Blöcke `[u16 Länge][Geheimtext]`, Nonce = Blockzähler.
* **UDP:** Jeder Absender wählt beim Beitreten bzw. beim ersten Datagramm 8
  Zufallsbytes als Sitzung. Datagramm: `Kopf (8) | Sitzung (8) | Zähler (8) |
  Geheimtext`. Der Richtungsschlüssel entsteht aus Paarschlüssel und Sitzung.
  Wiederholte Zähler werden verworfen (Gleitfenster wie bei IPsec), abgelöste
  Sitzungen ebenfalls.

**Restrisiko:** Ein umgebauter Server kann eine *ganze* früher mitgeschnittene
UDP-Sitzung erneut einspielen, bevor die neue beginnt. Lesen oder fälschen
kann er nicht. Das ist dieselbe Grenze wie in ADR 0012 (Mitgliederliste vom
Server).

Der Server sieht: wer was freigibt (Name, Port, Art), wer beitritt, wie viel
Verkehr fließt. Er sieht nicht, was fließt. Das entspricht der Tabelle in
ADR 0012.

### 6. Server: Vermittlung als reine Logik

`spielnetz::vermittlung` ist ein Zustandsautomat ohne I/O: Ereignisse
(`Anbieten`, `Beitreten`, `Raumwechsel`, `Abgemeldet`, …) gehen hinein,
Aktionen (`Senden an X`, `Strom verbinden`, `Kanal zuordnen`, `Beenden`)
kommen heraus. Der Server-Kern meldet Raumwechsel und Abmeldungen über ein
schmales Trait. So bleibt das Modul prüfbar, ohne Server zu starten.

### 7. Grenzen (Vorgaben, einstellbar)

| Grenze | Vorgabe | Warum |
|---|---|---|
| Freigaben je Mitglied | 2 | ein Spiel plus ein Sprachserver eines Spiels |
| Teilnehmer je Freigabe | 16 | typische Vereinsrunde |
| TCP-Ströme je Teilnehmer | 32 | Spiele öffnen selten mehr als eine Handvoll |
| Durchsatz je Freigabe | 20 000 kbit/s, 0 = unbegrenzt | schützt die Leitung des Betreibers und damit die Sprache |
| UDP-Datagramm | ≤ 1 100 Byte Nutzlast | QUIC-Datagramm minus Kopf, Sitzung, Zähler und Prüfsumme; größere werden verworfen und gezählt |

### 8. Rechte

Neues Recht `Capability::Spielnetz` für Administrator, Moderator und Mitglied,
**nicht** für Gast. Wie Sprechen: wer nur zu Gast ist, öffnet niemandem einen
Port und bekommt auch keinen.

### 9. Sicherheit am Rand

* Der Gastgeber verbindet **nur** `127.0.0.1:<port>`. Das Modul ist kein Weg
  in sein Heimnetz.
* Der Teilnehmer bindet **nur** `127.0.0.1`. Sein Heimnetz sieht den Port
  nicht.
* Ist der gewünschte lokale Port belegt, nimmt der Teilnehmer den nächsten
  freien und zeigt ihn an.

## Stufen

1. **Portfreigabe über den Server** (dieses ADR): TCP und UDP, Ende zu Ende,
   Server- und Client-Schalter, Fenster, CLI.
2. **Direkt, wenn möglich:** Hole-Punching mit dem Server als Vermittler,
   Rückfall auf Stufe 1. Spart dem Betreiber Bandbreite, kostet die
   gegenseitige Sichtbarkeit der IP (eigene Zustimmung).
3. **Virtuelle Netzwerkkarte** für Broadcast-Spiele (WireGuard über
   `boringtun`, TUN). Braucht Signatur (macOS Network Extension), Wintun
   unter Windows und Adminrechte. Erst, wenn Stufe 1 genutzt wird.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Virtuelle Netzwerkkarte sofort | Treiber, Adminrechte, Apple-Entwicklerkonto; bricht „starten ohne Installation“ |
| Spielverkehr auf der Sitzungsverbindung | teilt Staukontrolle und Strombudget mit Sprache und Text; Modul wäre nicht abschaltbar, ohne die Sitzung anzufassen |
| Eigener UDP-Port fürs Modul | zweite Portfreigabe für den Betreiber, widerspricht „ein UDP-Port“ |
| Ohne Ende-zu-Ende über den Server | bricht das Versprechen aus ADR 0012 |
| Tailscale/ZeroTier einbinden | fremdes Konto, fremder Koordinationsdienst — genau das, was sprechwerk ersetzt |

## Preis

* Ein neues Crate und eine zweite Verbindung je Client, der das Modul nutzt.
* Bandbreite des Betreibers (Stufe 1 leitet alles durch), begrenzt durch §7.
* Broadcast-Spiele gehen erst mit Stufe 3.
* UDP-Spiele mit Paketen über 1 100 Byte verlieren diese Pakete.

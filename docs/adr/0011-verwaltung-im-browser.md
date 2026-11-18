# ADR 0011 — Verwaltung im Browser

**Status:** umgesetzt (2026-11-17), **ersetzt durch [ADR 0013](0013-alles-in-rust.md)** (2026-11-18): Verwaltung auf der Konsole (`sprechwerk stand/rolle/protokoll`) und im Server-Fenster, kein HTTP, kein Port 7444 nach außen.
**Betrifft:** `crates/server` (`verwaltung.rs`, `mitschrift.rs`, `build.rs`),
`crates/domain` (`rolle_setzen_als_betreiber`), `ui/src/verwaltung/`,
`Dockerfile`, `docker-compose.yml`, `.github/workflows/server.yml`
**Ändert:** SPEC §2 „Keine Betriebs-Weboberfläche" (Nachtrag 2026-11-17),
ADR 0007 „nur 7443/udp nach außen" (bleibt so — siehe unten)

## Anlass

Zwei Wünsche des Betreibers:

1. Den Server **ohne Docker** betreiben — als Programm unter Windows und macOS,
   mit einem Fenster, das das Protokoll zeigt.
2. **Administrator werden**, ohne Wettlauf. Bisher galt nur „wer zuerst kommt,
   wird Administrator" (SPEC §7). War jemand schneller, oder soll ein Zweiter
   dazu, half nur ein Administrator im Client — oder Export, Bearbeiten,
   Import.

Im Container gibt es kein Fenster. Damit beide Betriebsarten dasselbe bekommen,
ist es **eine** Oberfläche im Browser, die der Server selbst ausliefert.

## Entscheidung

* **Der Server liefert eine Verwaltungsseite aus** (HTTP, Vorgabe-Port
  7444/tcp). Inhalt: Server (Port, Laufzeit, Verbundene, letzte Sicherung,
  Fingerabdruck), Mitglieder mit Rolle — änderbar —, Protokoll live.
* **Administrator wird man durch Beförderung**, nicht durch einen Schlüssel:
  mit der App verbinden, in der Verwaltung beim eigenen Namen die Rolle wählen.
  Der Betreiber ist dabei kein Mitglied; im Prüfprotokoll steht er als
  `Betreiber (Verwaltung)` ohne Kennung. Die Regel „der letzte Administrator
  bleibt" gilt auch für ihn (eine Stelle: `State::rolle_umsetzen`).
* **Gleiches Aussehen wie das Fenster:** Vue mit `@weppelmann/components`,
  dieselben Tokens, Texte (`texte.json`, vier Sprachen) und drei Themes. Ein
  zweiter Einstieg in `ui/` (`verwaltung.html`), kein zweites Projekt.
* **Eingebettet, nicht daneben gelegt:** `build.rs` nimmt
  `ui/dist-verwaltung/` mit `include_bytes!` ins Programm. Der Server bleibt
  eine Datei; der Container braucht kein Node. Fehlt der Bau, startet der Server
  trotzdem und zeigt eine Seite mit der Bauanleitung — `cargo test` hängt nicht
  an Node.
* **Protokoll aus derselben Quelle:** `tracing` schreibt in einen
  `Mitschreiber`, der jede Zeile an stdout gibt **und** die letzten 5000 im
  Speicher hält. Die Seite fragt „ab Nummer N" im Sekundentakt.
* **Programm unter Windows/macOS:** das Konsolenfenster ist das
  Protokollfenster; beim Start öffnet sich der Browser mit dem Anmeldelink.
  Unter Linux nur mit `DISPLAY`/`WAYLAND_DISPLAY`, im Container nie. Daten
  liegen ohne Angabe in `daten/` neben dem Programm (`/data`, wenn es das gibt —
  Container und bisherige Installationen ändern sich nicht).

## Schutz

Wer die Seite bedient, kann Administratoren ernennen. Deshalb:

| Maßnahme | Wogegen |
|---|---|
| Bindet an `127.0.0.1`; im Container `0.0.0.0`, aber Compose reicht nur an `127.0.0.1` des Hosts weiter | Zugriff aus dem Netz |
| Zugangscode, 128 Bit Zufall je Start, im Protokoll als Link | wer an den Port kommt, aber nicht an die Konsole |
| Selbst gewählter Code mindestens 16 Zeichen, 400 ms Pause nach falschem Code | Durchprobieren |
| Vergleich in gleichbleibender Zeit | Raten über die Antwortzeit |
| Keks `HttpOnly; SameSite=Strict`, nicht Browser-Speicher | Skripte, fremde Seiten |
| Ändern nur mit `Content-Type: application/json` | Formulare fremder Seiten (CSRF) |
| `Content-Security-Policy` ohne fremde Quellen, `frame-ancestors 'none'`, `X-Frame-Options: DENY` | Einbetten, eingeschleuste Skripte |
| `Referrer-Policy: no-referrer`, Weiterleitung nach `/anmelden` | Code aus der Adresszeile weitergetragen |
| HTTP mit `axum`/`hyper`, nicht von Hand | Fehler beim Zerlegen von Anfragen |

Kein HTTPS: die Seite ist nur über `127.0.0.1` bzw. einen SSH-Tunnel
erreichbar — beides verlässt den Rechner nicht im Klartext. Wer sie anders
erreichbar macht (`verwaltung.bind = "0.0.0.0"` ohne Container), schickt Code
und Keks unverschlüsselt; die Beispielkonfiguration sagt deshalb „nur dieser
Rechner".

## Was sie bewusst nicht kann

Räume anlegen, Sperren, Konfiguration ändern. Räume und Sperren verwaltet der
Client (mit Prüfprotokoll je Handlung), die Konfiguration ist die Datei
(SPEC §2). Die Verwaltung schließt nur die Lücke, die der Client nicht
schließen kann: den ersten bzw. nächsten Administrator.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Admin-Schlüssel auf dem Server erzeugen und herunterladen | der Server kennte den geheimen Schlüssel; die App kann keine Schlüsseldatei einlesen; eine Datei mit Administratorrecht läge dann in „Downloads" |
| Einmal-Code, den die App beim Verbinden mitschickt | Protokolländerung für einen einmaligen Vorgang |
| Eigenes Fenster (Tauri) für den Server | eigene Pakete je System, doppelte Bauzeit — und im Container trotzdem nichts |
| Weboberfläche nur im Container, Fenster für das Programm | zwei Oberflächen, die auseinanderlaufen |
| Server-Sent Events/WebSocket für das Protokoll | Abfrage im Takt übersteht Neustarts ohne eigene Wiederverbindung und reicht für einen Menschen, der hinsieht |

## Kosten

* Ein TCP-Port mehr (7444) — nicht nach außen, aber ein offener Port auf dem
  Rechner.
* `axum` + `hyper` als Abhängigkeit (~25 Crates, rein Rust).
* Server-Bau braucht für die Oberfläche einmal Node (CI: Job `verwaltung` in
  `server.yml`, dessen Ergebnis auch der Container-Bau nimmt).
* Windows- und macOS-Server sind unsigniert und noch auf keinem echten Rechner
  gelaufen — gebaut wird in der CI.

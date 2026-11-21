# Sicherheit

## Eine Lücke melden

**Nicht als öffentliches Issue.** Bitte über den Reiter **Security** dieses
Repositories → **„Report a vulnerability"** (GitHub Private vulnerability
reporting). Der Bericht ist dann nur für die Maintainer sichtbar, und wir können
gemeinsam einen Termin für die Veröffentlichung festlegen.

Hilfreich im Bericht:

* was ein Angreifer erreicht (mithören, mitlesen, sich als jemand ausgeben,
  Dienst zum Stehen bringen),
* die Version (Tag oder Paketname, z. B. `3.0.2`) und das System,
* Schritte zum Nachstellen, wenn es geht mit Protokoll (`SPRECHWERK_LOG=debug`).
  **Keine** Schlüsseldateien, Serverpasswörter oder echten Gesprächsinhalte.

Wir antworten innerhalb von 7 Tagen. Ist die Lücke bestätigt, gibt es einen
Fix und — je nach Schwere — ein Release; im CHANGELOG steht dann, was war,
ohne dass eine offene Lücke beschrieben wird, bevor der Fix draußen ist.

## Unterstützte Versionen

Die jeweils **letzten drei 3.x-Versionen** bekommen Sicherheitsfixes (Stand
3.0.x). Ältere Stände nicht: ein Update ist ein Container-Tausch
oder ein Paket, und eine Sicherheitslücke wird nicht rückportiert.

## Was als Lücke gilt

Alles, was eine der Zusagen aus der README bricht:

* **Der Betreiber liest mit** — Text, Sprache, Reaktionen oder Vorschauinhalte
  im Klartext auf dem Server, in der Datenbank, in einer Sicherung oder im
  Protokoll ([ADR 0012](docs/adr/0012-alles-ende-zu-ende.md)).
* **Transport oder Anmeldung** — TLS 1.3 über QUIC, angeheftetes Zertifikat,
  Besitznachweis per Ed25519 ([ADR 0002](docs/adr/0002-verschluesselung.md)).
  Dazu gehören ein umgangener Fingerabdruck-Vergleich und eine Anmeldung ohne
  gültigen Beweis.
* **Rechte** — jemand tut, was seine Rolle nicht darf (stumm schalten, werfen,
  sperren, Rollen setzen), oder das Prüfprotokoll lässt einen Eingriff aus.
* **Räume** — jemand hört einen Raum mit, in dem er nicht ist; der Raum steht
  im Sitzungszustand des Servers, nicht im Sprachkopf.

Nichts davon ist eine Lücke: der Betreiber sieht Metadaten (siehe README,
„Sicherheit und Daten"), wer den Server umbaut, kann ein Mitglied erfinden, und
die hybride Umlage des Raumschlüssels ist noch nicht verdrahtet
([ADR 0010](docs/adr/0010-quantensicherheit.md) Schritt 2) — das steht offen in
der Dokumentation.

## Was wir schon wissen

Die offenen Punkte stehen in der README unter „Sicherheit und Daten" („Was das
nicht leistet") und in [`docs/adr/`](docs/adr/). Ein Blick dorthin spart einen
Bericht.

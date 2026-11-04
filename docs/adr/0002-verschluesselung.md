# ADR 0002 — Verschlüsselung: TLS 1.3 durch QUIC, Besitznachweis durch Ed25519

**Status:** entschieden (2026-11-04), Endezuende für Sprache offen (siehe unten)
**Betrifft:** `crates/adapters`, `crates/server`, `crates/protocol`

## Zusammenhang

Der Nutzer verlangt ausdrücklich Verschlüsselung. TeamSpeak hat dafür ein
eigenes Verfahren gebaut (EAX mit selbst abgeleiteten Schlüsseln, Puzzle im
Handshake). Genau dort liegen dessen Fehlerquellen — und ein Erbe, das wir
nicht antreten wollen.

## Entscheidung

1. **Transportverschlüsselung kommt von QUIC.** Jede Verbindung ist TLS 1.3 mit
   AEAD; Schlüsselwechsel, Forward Secrecy und Replay-Schutz sind Teil des
   Protokolls. Wir schreiben **keine** eigene Krypto.
2. **Anmeldung ist ein Besitznachweis.** Die Kennung ist ein
   Ed25519-Schlüsselpaar. Der Server schickt eine Zufallszahl (32 Byte), der
   Client signiert sie, der Server prüft. Eine mitgelesene Anmeldung ist damit
   nicht wiederverwendbar. (TeamSpeak schickt stattdessen eine dauerhafte
   Kennung, die man kopieren kann.)
3. **Zertifikat:** selbst signiert, Fingerabdruck (SHA-256) wird beim Start
   angezeigt und beim ersten Verbinden bestätigt (Trust on first use).
   Eine Zertifizierungsstelle ist für selbst gehostete Instanzen nicht
   praktikabel; eine Scheinprüfung wäre schlechter als eine ehrliche
   Bestätigung.
4. **Kein Klartext auf der Leitung.** Auch Sprachdatagramme sind QUIC-Pakete
   und damit verschlüsselt — es gibt keinen unverschlüsselten Pfad wie bei
   TeamSpeak (`Ping`/`Pong` ausgenommen, die keine Inhalte tragen).

## Was das **nicht** leistet

Der Server kann die Sprache entschlüsseln — er muss sie weiterleiten. Wer dem
Betreiber nicht traut, braucht Ende-zu-Ende-Verschlüsselung.

## Offener Punkt: Endezuende für Sprache

**Ziel:** der Betreiber kann Gespräche nicht mithören.

**Vorschlag (noch nicht entschieden):**

1. Jeder Client hat ein langlebiges X25519-Schlüsselpaar (dasselbe wie die
   Kennung, zusätzlich zum Signaturschlüssel).
2. Beim Betreten eines Raums erzeugt der erste Anwesende einen Gruppenschlüssel
   und verteilt ihn einzeln (X25519 + AEAD) an die anderen — über den Server,
   der ihn nicht lesen kann.
3. Neue Teilnehmer bekommen den Schlüssel vom „Schlüsselwart" des Raums
   (der erste Anwesende, danach deterministisch der älteste).
4. Beim Verlassen wird der Schlüssel gewechselt (Rotation), damit Ausgeschiedene
   nichts mehr mitlesen können.
5. Der Server sieht nur Datagramme ohne lesbaren Inhalt — Verteilen und
   Rechteprüfung bleiben seine Aufgabe.

**Kosten:** Gruppenschlüsselverwaltung, Rotation bei jedem Verlassen,
Nachrichtenversand für Schlüsselwechsel, ein zweiter Krypto-Baustein im Client.
**Nutzen:** der Betreiber ist nicht mehr allmächtig — für Vereine mit
Datenschutzauflagen der entscheidende Punkt.

**Nächster Schritt:** Messen, was das kostet (Schlüsselwechsel bei 50 Leuten),
dann entscheiden. Bis dahin gilt: Transport ist verschlüsselt, Ende-zu-Ende
nicht — und das steht so in der Anleitung, statt es zu verschweigen.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| Eigenes Krypto wie TeamSpeak | genau die Fehlerquelle, die wir vermeiden |
| Nur Passwort im Klartext (wie TeamSpeak-Query) | indiskutabel |
| Zertifikat von einer CA | Zusatzaufwand für Betreiber, kein Sicherheitsgewinn bei TOFU-fähigen Clients |
| DTLS-SRTP je Medienstrom (WebRTC) | erst nötig, wenn Browser mitsprechen sollen |

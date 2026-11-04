# ADR 0006 — Ende-zu-Ende-Verschlüsselung für Sprache

**Status:** entworfen, Entscheidung offen (2026-11-04)
**Betrifft:** `crates/adapters`, `crates/protocol`, `crates/server`

## Ausgangslage

Der Transport ist verschlüsselt (TLS 1.3 über QUIC, ADR 0002). Der **Server
kann die Sprache trotzdem entschlüsseln** — er muss sie weiterleiten. Wer dem
Betreiber nicht traut (Verein mit Datenschutzauflage, gehostete Instanz), will
mehr.

## Umfang

**Nur Sprache.** Text bleibt serverseitig lesbar, weil er dauerhaft gespeichert
wird (SPEC §6). Ein „Ende-zu-Ende" für gespeicherten Text wäre eine andere
Zusage: dann könnte der Server nicht suchen, aufräumen oder moderieren. Das
wird hier bewusst nicht versprochen.

## Entwurf

1. **Zwei Schlüssel je Client.** Der Signaturschlüssel (Ed25519, Anmeldung)
   bleibt; dazu ein **X25519-Schlüssel** für den Schlüsselaustausch. Beide
   liegen in derselben Schlüsseldatei.
2. **Gruppenschlüssel je Raum und Epoche.** Beim Betreten erzeugt der
   „Schlüsselwart" (der am längsten anwesende Client, deterministisch aus der
   Mitgliedsliste) einen 32-Byte-Schlüssel `K_epoche`.
3. **Verteilung einzeln.** `K` wird für jeden Anwesenden mit dessen X25519-
   Schlüssel verschlüsselt (X25519 + HKDF-SHA256 + ChaCha20-Poly1305) und über
   den Server verschickt. Der Server sieht nur Geheimtext.
4. **Rahmenverschlüsselung.** `nonce = keyid || folge`, AEAD
   ChaCha20-Poly1305 über den Opus-Rahmen. Kopf bleibt lesbar (Version, Codec,
   Folge), damit der Server **verteilen und Rechte prüfen** kann.
5. **Wechsel.** Beim Verlassen erhöht sich die Epoche, der Schlüsselwart
   verteilt neu. Wer geht, kann danach nichts mehr lesen.
6. **Überlappung.** Zwei Epochen dürfen kurz parallel gelten (Schlüsselwechsel
   braucht einen Rahmen Zeit), sonst gehen beim Wechsel Wörter verloren.

## Was das schützt — und was nicht

| Geschützt | Nicht geschützt |
|---|---|
| Betreiber liest Gespräche nicht mit | ein Mitglied, das mitschneidet (es hat den Schlüssel) |
| Sicherungen enthalten keinen Klartext | ein kompromittierter Client |
| Gehostete Instanz kann nicht „mithören" | Metadaten (wer wann mit wem) |

## Kosten

* Schlüsselverwaltung im Client (Verteilung, Epochen, Wechsel, Verlust von
  Schlüsselpaketen).
* Zwei bis vier zusätzliche Rahmen beim Betreten eines Raums.
* Kein Einfluss auf Latenz (Verschlüsseln eines 80-Byte-Rahmens: Mikrosekunden).
* Testbarkeit: prüfbar ohne Audio — Schlüsselaustausch ist reine Rechnerei.

## Empfehlung

**Bauen, aber nach Audio und Messung.** Reihenfolge: Audio-Pfad → Messung →
E2E. Grund: E2E verändert das Rahmenformat (12 Byte Aufschlag) und damit die
Messung; wer zuerst verschlüsselt, misst das Falsche.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| MLS (RFC 9420) | richtig für große Gruppen und häufige Wechsel, aber ein großer Baustein; erst bei Bedarf |
| Nur Server vertrauen | für „selbst gehostet, ich bin der Betreiber" völlig ausreichend — und billiger |
| Gesamter Verkehr über ein VPN | verschiebt das Problem zum Netz, nicht zur Software |

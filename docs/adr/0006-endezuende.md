# ADR 0006 — Ende-zu-Ende-Verschlüsselung für Sprache

**Status:** umgesetzt (2026-11-04) — je Raum einschaltbar
**Entscheidung (Nutzer):** pro Raum optional, abschaltbar; Text bleibt lesbar.
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

   **Nachtrag 2026-11-13.** Der Code nahm dafür die kleinste `MemberId`, der
   Kommentar sagte „kleinster öffentlicher Schlüssel" — beides nicht das, was
   hier steht, und die Id-Regel hatte einen Fehler: kam jemand mit kleinerer Id
   dazu, wechselte der Wart **mitten in der Epoche**, erzeugte einen zweiten
   Schlüssel und verteilte ihn. Erreichte die Verteilung nicht alle, hörten die
   Betroffenen einander nicht mehr — ohne Fehlermeldung. Jetzt entscheidet die
   Anwesenheitszeit (`raum_seit`, im Protokoll mitgeführt); ein Neuankömmling
   kann den Wart nicht mehr ablösen.
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

**Gemessen 2026-11-13** (Ende zu Ende, echter Server, drei echte Clients,
`schluesselwechsel_im_laufenden_gespraech` in `crates/desktop/src/verschluesselung.rs`):
Das Wechselfenster — vom Verlassen bis zum neuen Schlüssel bei den
Verbliebenen — beträgt **35–110 ms**, also **zwei bis fünf 20-ms-Rahmen**
(mehrere Läufe, Loopback).

Punkt 6 ist damit ebenfalls gebaut: der **vorige** Schlüssel bleibt 1,5 s
gültig (`UEBERGANG` in `verschluesselung.rs`), länger nicht. Ein Rahmen, der
beim Wechsel schon unterwegs war, kommt also an — einzeln geprüft
(`uebergang_laesst_den_alten_schluessel_kurz_gelten`) und Ende zu Ende
(der Test schickt nach dem Wechsel einen Rahmen mit dem alten Schlüssel).

Dabei fiel ein zweiter Fehler auf: ein **Späterkommender** bekam keinen
Schlüssel. Der Wart hatte den Schlüssel der Epoche schon und tat deshalb
nichts — wer danach kam (oder nach einem Abbruch zurückkam), sah nur „warte auf
den Schlüssel des Raums". Behoben: der Wart verteilt erneut, sobald sich die
Runde der Anwesenden ändert.

**Testausführung:** `crates/desktop` liegt außerhalb des Arbeitsbereichs (Tauri
hat eine eigene Sperrdatei), `cargo test` im Wurzelverzeichnis sammelt seine
Tests also nicht ein. Für Fenster und Verschlüsselung:
`cd crates/desktop && cargo test` (der Ende-zu-Ende-Test startet den echten
Serverprozess; vorher `cargo build -p sprechwerk-server`).

**Nachtrag 2026-11-13.** Punkt 5 war bis heute nur beschrieben, nicht gebaut:
`State::verlassen` ließ `verschluesselt_seit` unverändert, die Epoche drehte
sich nur beim Ein- und Ausschalten der Verschlüsselung. Wer den Raum verließ,
behielt den Schlüssel also beliebig lange — und konnte mitgeschnittene Rahmen
später entschlüsseln. Gefunden beim Prüfen der Zusagen des Protokolls
(`ClientFrame::Schluesselpaket`: „Zählt bei jedem Verlassen hoch"), behoben in
`State::epoche_weiter`; der Schritt ist mindestens eine Sekunde, weil die
Epoche Sekunden zählt und zwei Drehungen sonst dieselbe Zahl ergäben.

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

## Umgesetzt

| Stück | Ort |
|---|---|
| Raumschlüssel, Umschlag (X25519 + HKDF-SHA256 + ChaCha20-Poly1305) | `crates/adapters/src/verschluesselt.rs`, 11 Tests |
| Rahmen verschlüsseln/entschlüsseln (Kopf bleibt lesbar) | dort, 2 Tests |
| Schlüsselverwaltung, Wärter-Wahl, Unterschrift prüfen | `crates/desktop/src/verschluesselung.rs` |
| Übergangsfenster (ADR §6) | dort, `uebergang_laesst_den_alten_schluessel_kurz_gelten` |
| Ende zu Ende: Wechsel im laufenden Gespräch, verlorenes Paket, Späterkommende | dort, 3 Tests (echter Serverprozess) |
| Raum-Flag, Epoche, Protokoll-Eintrag | `crates/domain/src/state.rs` |
| Epoche dreht beim Verlassen, Verschieben und Rauswurf | dort, `verlassen_dreht_die_epoche` (3 Fälle) |
| Speicherung (überlebt Neustart) | `crates/store/src/lib.rs`, 1 Test |
| Weiterleitung der Pakete, Recht, Raumgrenze | `crates/server/src/kern.rs` |
| **Beweis am echten Server** | `crates/server/tests/verschluesselt.rs`, 4 Tests |

Zwei Abweichungen vom Entwurf oben, beide begründet:

1. **Der X25519-Schlüssel steht nicht in der Schlüsseldatei.** Er wird aus dem
   Anmeldeschlüssel abgeleitet (SHA-512, klemmen — dasselbe, was libsodium
   macht). Damit bleibt die Datei bei 32 Byte, und es gibt keinen zweiten
   Schlüssel, den man beim Umzug vergisst.
2. **Die Epoche ist ein Zeitstempel**, kein Zähler: `verschluesselt_seit / 1000`.
   Alle Clients rechnen denselben Wert aus, ohne dass jemand ihn aushandeln
   muss.

**Was der Server nicht kann:** den Umschlag öffnen (er ist für genau einen
Empfänger verschlüsselt) oder einen eigenen unterschieben (er ist mit dem
Ed25519-Schlüssel des Absenders unterschrieben; geprüft wird gegen die Kennung,
die der Server selbst weiterreicht). Er kann **verweigern** — dann gibt es
keinen Ton, aber kein Mithören.

**Kosten, gemessen:** 16 Byte Aufschlag je Rahmen (AEAD) plus 6 Byte Kopf,
zusammen 22 von rund 100 Byte — rund 20 % auf einen 32-kbit/s-Rahmen. Der
Sprachkopf bleibt lesbar, weil der Server zum Verteilen und für das Rederecht
Folgenummer und Sprecher braucht.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| MLS (RFC 9420) | richtig für große Gruppen und häufige Wechsel, aber ein großer Baustein; erst bei Bedarf |
| Nur Server vertrauen | für „selbst gehostet, ich bin der Betreiber" völlig ausreichend — und billiger |
| Gesamter Verkehr über ein VPN | verschiebt das Problem zum Netz, nicht zur Software |

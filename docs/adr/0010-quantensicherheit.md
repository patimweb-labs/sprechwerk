# ADR 0010 — Quantensicherheit: was bricht, was kostet der Umbau

**Status:** Schritt 1 umgesetzt (2026-11-14), Schritt 2 offen
**Betrifft:** `crates/adapters`, `crates/server`, `Cargo.toml` (Anbieter von
`rustls`), `crates/protocol` (Umlagen des Raumschlüssels)

## Warum das jetzt auf den Tisch kommt

„Quantensicher" ist kein Zukunftsthema, sondern eines von heute: Wer heute
Verkehr mitschneidet und aufbewahrt, kann ihn später entschlüsseln
(*harvest now, decrypt later*). Ein Verein mit Datenschutzauflage sollte
wissen, welche Zusage er gibt.

**Was bricht** (Shors Algorithmus, für einen kryptografisch relevanten
Quantenrechner, den es noch nicht gibt):

| Baustein | Wo | Bricht |
|---|---|---|
| X25519 (Schlüsselaustausch) | Transport (QUIC/TLS), Umlage des Raumschlüssels | ja |
| Ed25519 (Unterschrift) | Anmeldung, Unterschrift der Schlüsselpakete | ja |
| ChaCha20-Poly1305, SHA-256 | Rahmenverschlüsselung, Ableitung | nein (Grover halbiert nur die Stärke; 256 Bit bleiben 128) |

**Was nicht bricht:** die Zusage „der Betreiber liest nicht mit" (ADR 0006).
Ende-zu-Ende bleibt Ende-zu-Ende — nur gegen den falschen Gegner.

## Was gemessen ist (2026-11-14, Loopback, Release)

**1. Reines Rust reicht für den Schlüsselaustausch der Sprache.**
`ml-kem 0.2.3` (RustCrypto, ML-KEM-768, FIPS-203):

```
50 Umschläge: verschlüsseln 1,33 ms (27 µs je), öffnen 1,69 ms (34 µs je)
Umschlag 1088 Byte, öffentlicher Schlüssel 1184 Byte, Geheimnis 32 Byte
Bauzeit: unter einer Sekunde, keine Systempakete
```

**2. Auch der Transport kann es — mit `aws-lc-rs` statt `ring`.**
`quinn 0.11` + `rustls 0.23.46` mit `kx_groups = [X25519MLKEM768]` **und sonst
nichts**: der Handschlag gelingt.

```
Verbunden mit NUR X25519MLKEM768 (Loopback, erster Handschlag: 18 ms kalt)
aws-lc-sys 0,47 baut auf dieser Maschine in 5 s — ohne cmake, mit gcc
```

Der erste Handschlag enthält die Einrichtung des Anbieters; aussagekräftig ist
der warme Vergleich, siehe „Umgesetzt".

`ring` bietet kein ML-KEM an; die PQ-Gruppe gibt es nur im Anbieter
`aws_lc_rs`. Das ist keine reine Rust-Abhängigkeit mehr (C-Code, `aws-lc-sys`),
bleibt aber ohne zusätzliche Systempakete baubar (C-Compiler genügt, cmake
nicht nötig).

## Zwei Schritte, getrennt entscheidbar

### Schritt 1 — Transport hybrid (**umgesetzt 2026-11-14**)

**Änderung:** `rustls` mit `features = ["aws-lc-rs", "std"]` statt `ring`,
`quinn` mit `rustls-aws-lc-rs` statt `rustls-ring`; in `crates/server/src/netz.rs`
und `crates/adapters` den Anbieter tauschen und die Reihenfolge der Gruppen auf
`[X25519MLKEM768, X25519]` setzen (hybrid: **beide** müssen brechen, nicht
eine). Prüfung: ein Test, der wie oben **nur** die PQ-Gruppe anbietet — kommt
die Verbindung zustande, ist sie PQ.

**Kosten:**

* C-Abhängigkeit in jedem Bau (Entwickler, CI, Docker) — bisher baute nur das
  optionale Merkmal `audio` C-Code (Opus).
* ClientHello wächst um rund 1,2 kB; kein zusätzlicher Umlauf, keine
  spürbare Latenz (18 ms Handschlag im Versuch, davon ist der PQ-Anteil
  Bruchteile).
* Ein Anbieter je Prozess: `ring` und `aws-lc-rs` schließen sich als
  Standardanbieter aus. Der Austausch ist also ganz oder gar nicht.

**Nutzen:** Text, Anmeldung, Serverpasswort und Sprachsteuerung sind gegen
späteres Entschlüsseln geschützt — auch gegen einen Mitschnitt von heute.

### Schritt 2 — Sprache Ende zu Ende hybrid (**Krypto liegt**, Verdrahtung offen)

**Änderung:** Der Umschlag je Empfänger trägt zusätzlich ein ML-KEM-768
Geheimnis: `K = HKDF(ML-KEM ‖ X25519(eph, sitzung) ‖ eph ‖ sitzung)` (Kombinierer wie X-Wing), dazu die
öffentlichen KEM-Schlüssel je Mitglied und Epoche (1184 Byte, mit Ed25519
unterschrieben, über den Server verteilt). Neue Protokollversion, weil die
Umlage länger wird.

**Nebenbefund, wichtiger als die Quantenfrage:** die Umlage ist heute
**statisch-statisch** — `X25519` aus dem langlebigen Anmeldeschlüssel, ohne
ephemere Werte. Damit gibt es auf dieser Schicht **keine Forward Secrecy**: wer
die Schlüsseldatei eines Mitglieds später erbeutet, kann alle aufgezeichneten
Schlüsselpakete entschlüsseln, die je an dieses Mitglied gingen. Der Umbau auf
ephemere Werte behebt beides in einem Zug — und kostet dasselbe.

**Kosten:** 1088 Byte je Umschlag (heute rund 100), 1184 Byte je Mitglied und
Epoche, rund 60 µs Rechenzeit je Mitglied (gemessen: 3 ms für 50 Mitglieder je
Schlüsselwechsel). Bei 32 kbit/s je Sprecher ist das kein Problem — Umlagen
passieren beim Betreten und Verlassen, nicht je Rahmen.

### Nicht geplant — und warum

* **Ed25519 durch ML-DSA ersetzen:** Die Unterschrift schützt gegen eine
  *aktive* Fälschung im Moment der Anmeldung, nicht gegen Mitlesen von heute.
  ML-DSA-65 bringt 1952 Byte öffentlichen Schlüssel und 3309 Byte Unterschrift
  in jedes `kennung`-Feld, jedes Prüfprotokoll und jede Sperrliste — viel
  Fläche für einen Angriff, der keinen Mitschnitt von heute verwertet. Erst
  wenn der Transport steht und der Bedarf bleibt.
* **Symmetrie aufstocken:** nicht nötig, 256 Bit sind schon quantenfest
  genug (Grover halbiert nur).

## Was auch dann nicht geschützt ist

| Nicht geschützt | Warum nicht |
|---|---|
| Metadaten (wer wann mit wem) | der Server muss verteilen und Rechte prüfen |
| Ein kompromittiertes Gerät | es hat die Schlüssel im Klartext |
| Betreiber, der Mitglied wird | er ist dann Teilnehmer, kein Mitleser |
| Quantenrechner, der *heute* rechnet | gibt es nicht; die Frage ist nur der Mitschnitt |

## Umgesetzt: Schritt 1 (2026-11-14)

| Stück | Ort |
|---|---|
| Anbieter `aws-lc-rs` statt `ring`, eine Wahl im Arbeitsbereich | `Cargo.toml`, `crates/server/Cargo.toml` |
| Gruppenreihenfolge `[X25519MLKEM768, X25519]` im Client | `crates/adapters/src/lib.rs` (`kx_gruppen`) |
| dieselbe Reihenfolge im Server | `crates/server/src/netz.rs` |
| Test: Gruppe steht zuerst, Anbieter kennt sie | dort, 2 Tests |
| **Test am Draht**: PQ-only-Client gegen den echten Server | `crates/server/tests/quantensicher.rs`, 2 Tests |
| **Test am Draht**: echter Client gegen PQ-only-Server (bis zur Ablehnung) | dort |

Der zweite Drahttest ist der wichtige: der Client muss den Handschlag mit
ausschließlich `X25519MLKEM768` schaffen, sonst käme die Antwort des Servers nie
an. Beide Richtungen sind damit geprüft, nicht nur eine Liste im Code.

**Kosten, gemessen** (200 Handschläge je Variante, dieselbe Maschine, gleicher
Prozess, Loopback):

```
klassisch X25519        p50  0,19 ms   p95  0,22 ms   p99  0,28 ms
hybrid X25519MLKEM768   p50  0,24 ms   p95  0,28 ms   p99  0,38 ms
```

Also **+50 µs je Verbindungsaufbau** — bei einem Beitritt von rund 1,5 ms
(`scripts/latenz.sh`) ist das ein Drei-Prozent-Anteil, der einmal je Sitzung
anfällt. Der Sprachpfad selbst ist unberührt (dort rechnet `chacha20poly1305`,
nicht der TLS-Anbieter): 15–18 µs je Rahmen vorher wie nachher, innerhalb der
Maschinenschwankung. Auf der Leitung wächst der erste Flug um den ML-KEM-Anteil
(1184 Byte öffentlicher Schlüssel, 1088 Byte Geheimtext), kein zusätzlicher
Umlauf.

**Was weiter klassisch ist:** die Anmeldung (Ed25519) und die Umlage des
Raumschlüssels für die Sprache (X25519). Ein Mitschnitt von heute ist damit für
**Text** wertlos, für **Sprache** noch nicht — das ist Schritt 2.

**Was das gekostet hat, außer Rechenzeit:** eine C-Abhängigkeit
(`aws-lc-sys`) in jedem Bau. Kein cmake, aber ein C-Compiler; Docker- und
CI-Images bringen ihn mit. `ring` ist raus — zwei Anbieter je Prozess gehen
nicht, und ein halber Wechsel (Handschlag PQ, Unterschriften klassisch) wäre
eine Konfiguration, die niemand prüft.

## Stand Schritt 2 (2026-11-14): die Krypto-Schicht steht

| Stück | Ort |
|---|---|
| `Austausch` — ephemere Sitzung (X25519 + ML-KEM-768), veröffentlichbar als Hex | `crates/adapters/src/verschluesselt.rs` |
| `FremderAustausch` — was ein anderes Mitglied in den Raum stellt, geprüft auf Länge | dort |
| `verpacken_hybrid` / `Austausch::oeffnen` — Umschlag gegen die **Sitzung** statt gegen den Langzeitschlüssel | dort |
| Ableitung über **beide** Geheimnisse und die X25519-Werte (HKDF-SHA256, Aufbau wie X-Wing: `ss_kem ‖ ss_x ‖ eph ‖ empfänger`, Raum und Epoche als Salt) | dort |
| X25519-Punkte kleiner Ordnung werden abgelehnt (`was_contributory`) — sonst bliebe still nur ML-KEM | dort |
| 9 Tests: Rundlauf, fremde Sitzung, verfälscht, Raum/Epoche, Größen, kaputte Eingaben, Nullpunkt, Bindung an die X25519-Werte | dort |
| Messwerkzeug (Release) | `crates/adapters/examples/umschlagprobe.rs` |

**Gemessen** (`cargo run --release --example umschlagprobe -p sprechwerk-adapters`):

```
veröffentlicht je Mitglied: 1216 Byte (2432 Zeichen hex)
Umschlag je Mitglied:       1180 Byte   (bisher rund 92)
eigene Sitzung erzeugen:    40 µs
verpacken für 50:           p50 3,47 ms   p95 3,63 ms
öffnen für 50:              p50 3,36 ms   (rund 67 µs je Empfänger)
```

Also: **~3,5 ms Rechenzeit je Schlüsselwechsel** für einen Raum mit 50
Mitgliedern, und ~1,2 kB je Mitglied einmal je Raumaufenthalt. Umlagen
passieren beim Betreten, Verlassen und Rechtewechsel — nicht je Rahmen. Der
Sprachpfad selbst bleibt bei 16 Byte Aufschlag je Rahmen (unverändert).

**Was noch fehlt (Schritt 2b/2c), damit es wirkt:**

1. Zwei Rahmen im Protokoll: der Client stellt seinen `Austausch` in den Raum
   (signiert mit dem Anmeldeschlüssel), der Server reicht ihn weiter — wie bei
   `Schluesselpaket`, mit denselben Rechten und derselben Raumgrenze.
2. Der Wart sammelt die veröffentlichten Hälften, statt sie aus der `kennung`
   zu rechnen, und verteilt erst, wenn er sie hat (Nachfordern gibt es schon).
3. Der Client wirft seine Sitzung beim Verlassen weg — das ist der Teil, der
   Forward Secrecy tatsächlich herstellt.
4. Ende-zu-Ende-Tests am echten Server für: Verteilung, Wechsel, Späterkommende,
   und **kein Öffnen** mit dem alten Langzeitschlüssel.

Solange 2b/2c fehlen, ist der laufende Pfad unverändert (klassisch, ohne
Forward Secrecy) — der Code dafür liegt bereit und ist geprüft, aber nicht
verdrahtet. Das ist Absicht: eine halb verdrahtete Schlüsselverteilung wäre
schlimmer als gar keine.

## Empfehlung

**Schritt 1 ist erledigt, Schritt 2 als nächstes.** Schritt 1 ist eine
Abhängigkeitsänderung mit einem Test; Schritt 2 braucht eine
Protokollversion und einen Übergang (alte und neue Umlage nebeneinander, sonst
hören sich gemischte Fassungen nicht). Beides zusammen macht die Zusage
haltbar: *aufgezeichneter Verkehr von heute ist auch in zwanzig Jahren nicht
lesbar* — bis auf Metadaten und das, was ein Mitglied selbst mitschneidet.

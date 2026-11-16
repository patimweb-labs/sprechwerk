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

## Nachtrag 2026-11-14 — der Client prüfte gar nichts

**Befund.** Punkt 3 dieser Entscheidung („Fingerabdruck beim ersten Verbinden
bestätigt") stand seit dem ersten Tag in den Dokumenten, aber nicht im Code.
`crates/adapters/src/lib.rs` hatte einen Prüfer, der für **jedes** Zertifikat
`Ok(())` zurückgab — Zertifikat, TLS-1.2- und TLS-1.3-Unterschrift. Damit war
der Transport zwar verschlüsselt (niemand konnte passiv mitlesen), aber nicht
verbunden: wer sich in die Leitung stellt, zeigt sein eigenes Zertifikat, und
der Client nimmt es an. Text, Steuerverkehr und — beim ersten Verbinden — das
Serverpasswort wären lesbar gewesen. Ende-zu-Ende-Sprache hätte nur noch
Geheimtext preisgegeben.

**Zwei Fehler, nicht einer.** Auch mit Anheftung allein wäre die Lücke
geblieben: ein Zertifikat ist ein *öffentlicher* Wert, er steht in jedem
Handschlag. Wer ihn vorzeigt, muss den privaten Schlüssel nicht haben — solange
niemand die Handschlag-Unterschrift prüft. Deshalb prüft der Client jetzt beides:
den angehefteten Fingerabdruck **und** die Unterschrift, mit den Verfahren des
Anbieters (`ring`), nicht mit einer eigenen Auswahl.

**Was jetzt gebaut ist:**

| Stück | Ort |
|---|---|
| Fingerabdruck-Rechnung an **einer** Stelle (Server-Anzeige = Client-Prüfung) | `crates/protocol/src/zertifikat.rs`, 6 Tests |
| Anheften, Prüfen, Unterschrift prüfen, Grund merken | `crates/adapters/src/lib.rs` (`Zertifikatspruefung`), 3 Tests |
| Abbruch **im Handschlag** — vor Kennung, Beweis und Serverpasswort | dort, `oeffnen_mit` |
| Anheften beim ersten Kontakt, Vergessen als Handlung | `crates/desktop/src/einstellungen.rs` (`vertrauen`), 4 Tests |
| Geprüfter Wert im Modell, im Fenster sichtbar | `crates/ipc` (`zustand.zertifikat`), `ui/src/views/ServerView.vue` |
| Nachweis am echten Server: passend, fremd, erster Kontakt | `crates/server/tests/zertifikat.rs`, 3 Tests |
| Nachweis durch das Fenster (Modell + Handschlag) | `crates/desktop/src/anwendung.rs`, 1 Test |

Der Test `fremdes_zertifikat_bricht_ab_bevor_die_anmeldung_geht` prüft die
Reihenfolge: nach einem abgelehnten Handschlag ist der Administratorplatz noch
frei — also hat der Server die Anmeldung nie gesehen.

**Was weiter gilt:** Wer den Schlüsselbund eines Mitglieds kopiert, ist dieses
Mitglied. Das Anheften schützt gegen den *fremden* Server in der Leitung, nicht
gegen ein gestohlenes Gerät.

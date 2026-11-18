# Geschwindigkeit und Qualität — Kriterien und Messungen

Ziel: **so schnell wie TeamSpeak 3 oder schneller, und mindestens dieselbe
Sprachqualität.** Dieses Dokument macht daraus Zahlen, die man nachprüfen kann,
statt aus einem Gefühl.

## 1. Abnahmekriterien

| Größe | Ziel | Stand |
|---|---|---|
| Serverdurchlauf eines Sprachrahmens, p99 | < 1 000 µs | **28 µs** (gemessen) |
| Beitritt bis „Willkommen", p95 | < 100 ms | **1,3 ms** (gemessen) |
| Aufschlag durch den Sprachkopf | < 8 % eines 20-ms-Rahmens | **7 %** (6 von 86 Byte) |
| Ende-zu-Ende im LAN, p99 | < 60 ms | **p95 1,7 ms** durch den Server; Puffer der Soundkarte (20 + 60 + 40 ms) kommen im Betrieb dazu |
| Ende-zu-Ende über WAN, p99 | < 120 ms | offen |
| Verlust 5 %, hörbare Aussetzer | keine | **vollständig ausgeglichen** (Paritätsrahmen, Test im Code); mit 5 % Verlust auf einer Störstrecke auf **einem** Rechner nachgemessen (`scripts/verlustprobe.sh`, siehe §2b) — im echten WAN weiter offen |
| Echo-Dämpfung (Lautsprecher) | > 20 dB | **39 dB** (AEC3, Release 0,7 % Kern) |
| Bandbreite je Sprecher | ≤ 45 kbit/s | **32 kbit/s** Opus + 25 % Paritätsrahmen + 6 Byte Kopf je 20 ms ≈ **41 kbit/s** |

Der Client ist gebaut; die Tonmessung steht in [`AUDIO.md`](AUDIO.md).
Was noch fehlt: ein **Verlusttest über ein echtes Netz** (`tc netem loss 5%`)
und der Vergleich gegen TeamSpeak mit demselben Ton.

## 2. Was gemessen ist (2026-11-04, dieselbe Maschine, Loopback)

```
Sprachrahmen durch den Server (A → Server → B), 1000 Messungen:
  min        12 µs
  p50        15 µs
  p95        20 µs
  p99        28 µs
  max       263 µs
Beitritt (QUIC + Anmeldung + Willkommen), 20 Versuche:
  p50     1 037 µs
  p95     1 300 µs
```

Ein Opus-Rahmen kommt alle **20 000 µs**. Der Server braucht im schlechtesten
Hundertstel davon. Gemessen wird von „Client A sendet" bis „Client B hat den
Rahmen" — also inklusive beider Aufgabenplanungen in Tokio, nicht nur der
Serverarbeit. Die Zahl ist damit eher zu hoch als zu niedrig.

Nachprüfen: `scripts/latenz.sh` (Serverdurchlauf), `scripts/sprechprobe.sh`
(ganzer Sprachpfad: Ton, Raumtreue, Rückkanal, Laufzeit),
`scripts/audio-ende-zu-ende.sh` (derselbe Weg mit Geräten: Aufnahme,
Entrauschung, Gate, Opus, Jitter-Puffer, Wiedergabe).

## 2b. Verlust und Jitter auf einem Rechner (2026-11-13)

`tc netem` braucht Root-Rechte und einen zweiten Rechner. Es geht auch ohne:
`scripts/verlustproxy.py` ist eine UDP-Störstrecke — Verlust, Jitter,
Verzögerung — zwischen Client und Server. Der Client merkt keinen Unterschied,
weil QUIC nur eine Gegenstelle sieht.

`scripts/verlustprobe.sh` misst damit zwei Sprecher gleichzeitig:

```
Sprecher direkt        440 Hz
Sprecher mit 5 % Verlust  660 Hz
Hörer hinter 20 ms Verzögerung + 25 ms Jitter
```

Ergebnis (Loopback, Release):

```
Störstrecke: 22 Pakete verworfen (5 %)
Ton direkt  440 Hz: hörbar in ~3 920 ms von 4 000 ms
Ton Verlust 660 Hz: hörbar in ~3 880 ms von 4 000 ms
BESTANDEN
```

Beide Sprecher kommen durch; die 22 verworfenen Pakete erzeugen **keine**
hörbare Lücke. Das ist der Beleg für den Paritätsrahmen im Client — bis jetzt
stand dazu nur „Test im Code".

**Was diese Messung nicht zeigt:** ein echtes WAN (Umlaufzeit, Reordering,
Wechsel der Bandbreite) und den Vergleich gegen einen TeamSpeak-Client. Beides
bleibt offen; die Störstrecke ist die Vorarbeit dafür, weil sie ohne zweiten
Rechner und ohne Root-Rechte läuft.

## 2c. Protokollsweep: jede Nachricht einmal mit Unsinn

`cargo test -p sprechwerk-server --bin sprechwerk protokollsweep` prüft jede
Client-Nachricht mit unbrauchbarer Eingabe: unbekannte Kennungen, leerer und zu
langer Text, `usize::MAX` in den Zählfeldern, kaputte Sprachrahmen.

Drei Regeln, an denen sich das Protokoll messen lassen muss:

1. **Keine Nachricht bleibt unbeantwortet.** Ein Befehl, der stillschweigend
   nichts tut, ist im Betrieb nicht von einem Netzproblem zu unterscheiden.
2. **Unbekannte Kennungen ergeben einen Fehler**, nicht `Ok` — sonst meldet die
   Oberfläche „gelöscht", obwohl nie etwas da war.
3. **Grenzwerte werden abgewiesen, nicht abgeschnitten.**

Der Sweep hat drei Fehler gefunden (alle behoben): `Trennen` und `Sperren`
meldeten `Ok` für Ziele, die es nicht gab bzw. für Kennungen, die keine waren;
und derselbe Schlüssel in **anderer Groß-/Kleinschreibung** galt als anderes
Mitglied — eine Sperre ließ sich durch Großschreiben umgehen.

## 3. Was die Sprachqualität bestimmt — und wo wir im Vergleich stehen

Die Qualität entsteht **im Client**, nicht im Server. Der Server ist ein
Verteiler: er prüft, kopiert und schickt weiter. Deshalb sind die
Serverzahlen oben so gut — und deshalb entscheidet der Client über „besser als
TeamSpeak".

Die Einstellungen des echten TeamSpeak-3-Clients (gemessen aus
`settings.db`, Profil „Standard"):

```
Capture/Standard/PreProcessing:
  vad=true  voiceactivation_level=-40  agc=true  denoise=true  denoiser_level=1
  echo_cancellation=false  echo_reduction=false  typing_suppression=false
  delay_ptt=true  delay_ptt_msecs=300   <- 300 ms Verzögerung bei Push-to-Talk
Playback/Standard:
  AGC=true  ComfortNoiseEnabled=false  MonoSoundExpansion=2
```

Daraus folgt für uns:

| Punkt | TeamSpeak 3 | unser Ziel | wie |
|---|---|---|---|
| Codec | Opus, 20 ms | Opus, 20 ms | festgelegt im Sprachkopf (Codec 0) |
| Push-to-Talk-Verzögerung | 300 ms (Vorgabe) | 0–50 ms | Vorlaufpuffer statt Verzögerung: 60 ms Audio im Ring halten und beim Tastendruck mitschicken |
| Verlustbehandlung | nicht einstellbar, clientintern | Opus-In-Band-FEC + 60–120 ms Jitter-Puffer | Client |
| Stille | VAD im Client | VAD/DTX, keine Pakete bei Stille | Client; spart rund 60 % Bandbreite |
| Staukontrolle | eigener UDP-Pfad | **BBR** statt Cubic | Server: `staukontrolle = "bbr"` |
| Paketaufschlag | eigener Kopf | 6 Byte | bereits umgesetzt (vorher 8) |

Der Punkt, an dem wir **besser** werden können, ist die
Push-to-Talk-Verzögerung: 300 ms Vorgabe in TeamSpeak sind im Gespräch deutlich
hörbar. Ein Vorlaufpuffer liefert dieselbe Sicherheit ohne die Wartezeit.

## 4. Was am Server dafür schon getan ist

* **Sprachkopf von 8 auf 6 Byte** — der Raum steht nicht mehr im Datagramm,
  sondern kommt aus dem Sitzungszustand. Weniger Aufschlag und zugleich sicher:
  niemand kann in einen fremden Raum hineinsprechen.
* **Staukontrolle umstellbar** (`staukontrolle = "bbr"`, Vorgabe). Cubic
  halbiert bei Verlust die Rate — für Sprache falsch.
* **Kleiner Sendepuffer** (`sprachpuffer_kib = 64`): Sprache wird verworfen,
  nicht angestaut. Ein verspäteter Rahmen ist wertlos.
* **Harte Obergrenze** 1 200 Byte je Sprachrahmen (unter der Pfad-MTU).
* **Zähler** für gesendete, verteilte und verworfene Rahmen
  (`SPRECHWERK_MESSUNG=1` protokolliert sie alle 10 s).

## 5. Was noch fehlt, um das Ziel zu belegen

1. **Audio im Client**: Opus-Aufnahme und -Wiedergabe, Ringpuffer,
   Jitter-Puffer, FEC. Ohne das gibt es keine Ende-zu-Ende-Zahl.
2. **Verlusttest**: `tc netem loss 2%` und `5%` auf einem Testrechner, dann
   p99-Aussetzdauer und gefühlte Qualität messen.
3. **Vergleichsmessung gegen TeamSpeak**: derselbe Ton, derselbe Rechner,
   einmal über den TS3-Server, einmal über sprechwerk; gemessen wird die Zeit
   vom Einspeisen bis zur Wiedergabe (Schleifenkabel oder virtuelles Gerät).
   Erst dieser Vergleich beantwortet „mindestens so gut" endgültig.

## 4. Alles auf einmal prüfen

```bash
scripts/pruefen.sh              # Tests + drei Sprachproben
scripts/pruefen.sh --mit-audio  # zusätzlich der echte Gerätepfad
scripts/pruefen.sh --nur-tests  # schnell: nur kompilieren und testen
```

Seit ADR 0013 ist alles ein Arbeitsbereich in Rust: `cargo test --workspace`
prüft Server, Client-Kern und beide Fenster. Dazu kommen die drei
Sprachproben, die kein Test sind, sondern Messungen an einem laufenden Server.

Die Oberflächentests (`crates/fenster/tests/oberflaeche.rs`,
`crates/server-fenster/tests/oberflaeche.rs`, `egui_kittest`) zeichnen jede
Ansicht mit einem Beispielmodell — ohne Server, ohne Soundkarte — und bedienen
sie über AccessKit: ein Klick auf „Verbinden" ergibt die Aktion `Verbinden`,
Enter im Eingabefeld sendet, ein Mitglied sieht keinen Reiter „Protokoll". Sie
scheitern, wenn ein Bedienelement keinen zugänglichen Namen hat oder ein
Zeichen in keiner Schrift vorkommt (gefunden beim Umstieg: Emoji, `●`, `⋯`).
Mit `SPRECHWERK_BILDER=docs/bilder` schreiben sie die Bildschirmfotos.

Der Ablauf meldet je Prüfung „bestanden" oder „FEHLGESCHLAGEN" und gibt am Ende
einen Rückgabewert — geeignet für einen Wächter vor dem Ausliefern. Die
Protokolle der Messungen liegen unter `/tmp/sprechwerk-pruefung-*.log`.

## 5. Grenzen gegen Fluten

Ein Client kann den Server mit Steuerrahmen zuschütten: jeder Rahmen kostet
eine Sperre, eine Zustandsänderung oder einen Rundruf an alle. Sprache läuft
als Datagramm daneben und hat ihre eigene Grenze (Sendepuffer,
`MAX_SPRACHRAHMEN`).

Einstellung: `rahmen_je_sekunde` (Vorgabe **100**, `0` = aus) — ein Mensch
erzeugt wenige Rahmen je Sekunde. Wer die Grenze überschreitet, bekommt einmal
die Antwort „später nochmal" und wird dann getrennt; die Protokollzeile nennt
Mitglied, Grenze und Zahl der Abweisungen.

Geprüft: `crates/server/src/netz.rs` (Fenster je Sekunde, Stille öffnet wieder,
`0` = unbegrenzt) und `crates/server/tests/rundlauf.rs` (echter Server mit
Grenze 5: der sechste Rahmen wird abgewiesen).

## 2c. Aufpreis der Quantensicherheit (2026-11-14, gleiche Maschine)

ADR 0010 Schritt 1: die Schlüsselvereinbarung ist hybrid `X25519MLKEM768`.
Gemessen wird der Handschlag, nicht das Hochfahren des Prozesses: 200
Verbindungen je Variante, gleicher Lauf, nur die Gruppe getauscht.

```
klassisch X25519        p50  0,19 ms   p95  0,22 ms   p99  0,28 ms
hybrid X25519MLKEM768   p50  0,24 ms   p95  0,28 ms   p99  0,38 ms
```

**+50 µs je Verbindungsaufbau**, einmal je Sitzung. Der Sprachpfad ist
unberührt — dort rechnet `chacha20poly1305`, nicht der TLS-Anbieter:

```
scripts/latenz.sh, drei Läufe nach dem Umbau:
  Sprachrahmen   p50 15–18 µs   p95 18–22 µs   p99 26–29 µs
  Beitritt       p50 1,47–1,81 ms   p95 1,80–2,76 ms
```

Vorher (Anbieter `ring`, ein Lauf): p50 20 µs / p95 32 µs / p99 350 µs und
Beitritt p50 1,75 ms / p95 2,53 ms. Der Unterschied zwischen den Läufen ist
größer als der Unterschied zwischen den Anbietern — auf dem Sprachpfad ist
kein Aufpreis messbar. Auf der Leitung wächst der erste Flug um den ML-KEM-Anteil
(1184 Byte öffentlicher Schlüssel, 1088 Byte Geheimtext), ohne zusätzlichen
Umlauf.

## 2d. Antwort und Rundruf unter einer Sperre (2026-11-15, gleiche Maschine)

Der Server führt einen Befehl jetzt **unter derselben Sperre** aus, unter der
er die Antwort ins Postfach legt (vorher: zwei Sperren, dazwischen eine Lücke,
siehe `docs/datenfluss.md` §2). Die Sperre umfasst damit zusätzlich ein
Einreihen in einen Kanal ohne Warten. `scripts/latenz.sh`, je drei Läufe:

```
vorher (zwei Sperren)   Sprachrahmen p50 15–16 µs  p99 21–29 µs   Beitritt p50 1,42–2,12 ms  p95 2,07–3,69 ms
nachher (eine Sperre)   Sprachrahmen p50 15–16 µs  p99 25–28 µs   Beitritt p50 1,56–2,05 ms  p95 2,18–2,80 ms
```

Kein messbarer Unterschied — die Streuung zwischen Läufen ist größer als der
Eingriff. Der Sprachpfad nimmt die Sperre ohnehin nicht über diese Stelle.

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
| Verlust 5 %, hörbare Aussetzer | keine | **vollständig ausgeglichen** (Paritätsrahmen, Test im Code); nicht im echten WAN gemessen |
| Echo-Dämpfung (Lautsprecher) | > 20 dB | **39 dB** (AEC3, Release 0,7 % Kern) |
| Bandbreite je Sprecher | ≤ 34 kbit/s | **32 kbit/s** eingestellt (Opus VBR, Kopf 6 Byte je 20 ms) |

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
(ganzer Sprachpfad: Ton, Raumtreue, Rückkanal, Laufzeit).

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
| Paketaufschlag | eigener Kopf | 4 Byte | bereits umgesetzt (vorher 8) |

Der Punkt, an dem wir **besser** werden können, ist die
Push-to-Talk-Verzögerung: 300 ms Vorgabe in TeamSpeak sind im Gespräch deutlich
hörbar. Ein Vorlaufpuffer liefert dieselbe Sicherheit ohne die Wartezeit.

## 4. Was am Server dafür schon getan ist

* **Sprachkopf von 8 auf 4 Byte** — der Raum steht nicht mehr im Datagramm,
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

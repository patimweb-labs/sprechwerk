# ADR 0005 — Video und Bildschirmfreigabe

**Status:** entworfen, **nicht** für Version 1 (2026-11-04)
**Betrifft:** `crates/protocol` (neue Rahmenarten), `crates/server` (Verteilung),
`crates/adapters` (Medienpfad), `ui/` (Darstellung)

## Zusammenhang

Gewünscht sind Bildschirmfreigabe, Video und Hintergrundeffekte. Der Server ist
heute ein **Verteiler**: er schickt dieselben Bytes an alle im Raum. Video
bricht diese Annahme, weil die Leitung jedes Empfängers unterschiedlich viel
verträgt.

## Was Video technisch verlangt

| Anforderung | Konsequenz |
|---|---|
| Unterschiedliche Bitrate je Empfänger | der Sender erzeugt **mehrere Qualitätsstufen** (Simulcast) oder der Server skaliert (Transcoding — teuer) |
| Verlust einzelner Bilder | Schlüsselbilder auf Anforderung (PLI/FIR), NACK für Teilstücke — ein eigener kleiner Wiederholungsmechanismus |
| Bildrate 15–30/s je Strom | mehr Datagramme als bei Sprache (Sprache: 50/s je Sprecher, Video: bis 30/s **je Empfänger und Stufe**) |
| Bildschirminhalt | anderer Codec-Zuschnitt (Textschärfe): VP9/AV1 mit Screen-Content-Werkzeugen, H.264 für Kompatibilität |
| Hardware | VAAPI (Linux), NVENC, VideoToolbox (macOS), Media Foundation (Windows) — je System ein anderer Weg |
| Effekte (Hintergrund, Weichzeichnen) | **im Client**, nicht im Server: Modell einbetten (ONNX/MediaPipe), GPU beschleunigen |
| Bandbreite | 720p ≈ 1–2,5 Mbit/s je Zuschauer; 10 Zuschauer = 10–25 Mbit/s **Upload des Servers** |

Daraus folgt die Kernzahl: bei *n* Sendern und *m* Zuschauern im Raum wächst der
Ausstoß des Servers mit **n × m**. Bei Sprache ist es dasselbe, nur mit 100-mal
kleineren Zahlen.

## Entscheidung

1. **Video kommt nach Audio** und erst, wenn die Sprachmessung steht. Grund: der
   Medienpfad (Ratenkontrolle, Puffer, Fehlerbehandlung) wird dort zum ersten
   Mal gebaut — dieselben Bausteine braucht Video, nur größer.
2. **Kein Transcoding.** Der Server leitet unverändert weiter und wählt je
   Empfänger eine **Stufe** (Simulcast). Damit bleibt er CPU-arm und die
   Latenz niedrig.
3. **Eigene Rahmenart im Protokoll** (`typ = 2`, Video), nicht in den
   Steuerstrom gemischt: `[version][codec][stufe][bildnummer][stück]` + Nutzlast.
   Schlüsselbilder dürfen fragmentiert werden; Teilstücke einzeln.
4. **Bildschirm ist ein eigener Strom** mit eigenem Codec-Zuschnitt, nicht
   „Video mit anderem Inhalt" — Text und Bewegung brauchen andere Einstellungen.
5. **Rechte:** `VideoSenden` als eigenes Recht in der Rollentabelle (Gast: nein),
   plus Obergrenze je Server (`video_max_kbit`), damit ein Knoten nicht
   ausgehungert wird.
6. **Effekte (Hintergrund, Teams-Optik): ausschließlich Client.** Der Server
   sieht sie nie. Kosten: Modellgröße, GPU-Abhängigkeit, ein weiterer Pfad je
   System — deshalb erst nach dem reinen Videopfad.

## Alternativen

| Alternative | Warum nicht |
|---|---|
| WebRTC/SFU von Anfang an | ein Vielfaches an Arbeit (DTLS-SRTP, ICE/TURN, Congestion Control je Empfänger); für Browser später der richtige Weg, für eigene Clients zu schwer |
| Transcoding im Server | CPU-Last pro Zuschauer; skaliert nicht auf einem Vereinsrechner |
| Nur Video ohne Bildschirm | verfehlt den Hauptnutzen (Präsentation, Support) |
| Effekte im Server | datenschutzrechtlich und technisch unsinnig (der Betreiber bekäme Bildinhalte im Klartext) |

## Kosten

* Protokollerweiterung + Verteilungslogik: Wochen, nicht Tage.
* Hardware-Dekoder je System: drei bis vier Baustellen.
* Bandbreite: ein Raum mit vier Sendern und zehn Zuschauern zieht leicht
  50 Mbit/s aus dem Server — das gehört in die Anleitung, bevor jemand klagt.
* Testbarkeit: ohne echte Kameras nur mit eingespeisten Dateien prüfbar
  (`/dev/video`-Ersatz, vorgefertigte YUV-Folgen).

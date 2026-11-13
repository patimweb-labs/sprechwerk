//! `sprechprobe` — Werkzeug zum Messen des Sprachpfads ohne Fenster.
//!
//! Zwei Betriebsarten, die zusammen einen Beweis ergeben:
//!
//! ```text
//! sprechprobe ton        --server 127.0.0.1:7443 --name Ton  --raum 2 --dauer 3
//! sprechprobe mitschnitt --server 127.0.0.1:7443 --name Ohr  --raum 2 --dauer 4 --wav /tmp/x.wav
//! ```
//!
//! Der eine spielt einen Ton in den Raum, der andere schreibt mit, was
//! ankommt. Damit lässt sich prüfen, ob Sprache wirklich **durch den Server**
//! und **nur im eigenen Raum** ankommt — ohne Mikrofon und ohne Kopfhörer.

use anyhow::{Context, Result};
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_audio::opus::{Kodierer, RAHMEN_SAMPLES};
use sprechwerk_audio::Empfänger;
use sprechwerk_protocol::Sprachkopf;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Aufruf:\n  sprechprobe ton --server ADDR --name N --raum R --dauer S [--hz 440]\n  \
             sprechprobe mitschnitt --server ADDR --name N --raum R --dauer S --wav DATEI\n  \
             sprechprobe rauschboden --server ADDR --name N --raum R --dauer S --wav DATEI"
        );
        std::process::exit(2);
    }
    let art = args[1].clone();

    // Betriebsart ohne Netz: nur der Pfad mit Gerät (Fehlersuche).
    if art == "pfad" {
        let mut mikrofon: Option<String> = None;
        let mut lautsprecher: Option<String> = None;
        let mut dauer = 3.0f64;
        let mut i = 2;
        while i < args.len() {
            let wert = args.get(i + 1).cloned().unwrap_or_default();
            match args[i].as_str() {
                "--mikrofon" => mikrofon = Some(wert),
                "--lautsprecher" => lautsprecher = Some(wert),
                "--dauer" => dauer = wert.parse()?,
                anderes => anyhow::bail!("unbekannt: {anderes}"),
            }
            i += 2;
        }
        let pfad = sprechwerk_audio::Pfad::starten(
            mikrofon.as_deref(),
            lautsprecher.as_deref(),
            sprechwerk_audio::Sendeeinstellungen::default(),
        )?;
        let mut datagramme = 0;
        let mut hoechster_pegel = 0u8;
        let ende = Instant::now() + Duration::from_secs_f64(dauer);
        while Instant::now() < ende {
            while pfad.naechstes_datagramm().is_some() {
                datagramme += 1;
            }
            hoechster_pegel = hoechster_pegel.max(pfad.pegel());
            std::thread::sleep(Duration::from_millis(5));
        }
        println!(
            "Datagramme: {datagramme}, höchster Pegel: {hoechster_pegel}, Blöcke: {}",
            sprechwerk_audio::Pfad::bloecke()
        );
        pfad.stoppen();
        return Ok(());
    }
    let mut server = "127.0.0.1:7443".to_string();
    let mut name = "Probe".to_string();
    let mut raum: Option<String> = None;
    let mut dauer = 3.0f64;
    let mut wav: Option<String> = None;
    let mut hz = 440.0f64;
    let mut schluessel = std::env::temp_dir().join("sprechprobe.schluessel");
    let mut neuer_raum: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        let wert = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--server" => server = wert,
            "--name" => name = wert,
            "--raum" => raum = Some(wert),
            "--dauer" => dauer = wert.parse().context("--dauer")?,
            "--wav" => wav = Some(wert),
            "--hz" => hz = wert.parse().context("--hz")?,
            "--schluessel" => schluessel = wert.into(),
            // Raum anlegen und dort eintreten (für Prüfungen mit zwei Räumen).
            "--neuer-raum" => neuer_raum = Some(wert),
            anderes => anyhow::bail!("unbekannt: {anderes}"),
        }
        i += 2;
    }

    let laufzeit = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    laufzeit.block_on(async move {
        let ziel: SocketAddr = server.parse().context("--server als IP:Port angeben")?;
        // Jede Probe bekommt einen eigenen Schlüssel, damit sich zwei Proben
        // nicht gegenseitig als derselbe Mensch anmelden.
        let pfad = schluessel.with_file_name(format!(
            "sprechprobe-{}-{}.schluessel",
            name,
            std::process::id()
        ));
        let s = Schluessel::laden_oder_erzeugen(&pfad)?;
        let mut verbindung = Verbindung::oeffnen(ziel, &s, &name, None)
            .await
            .context("anmelden")?;
        println!("angemeldet als {} ({})", name, verbindung.ich);

        if let Some(name) = neuer_raum {
            verbindung
                .sende(sprechwerk_protocol::ClientFrame::RaumAnlegen { name: name.clone() })
                .await?;
            // Antwort mit der neuen Raumliste abwarten.
            let mut neu = None;
            for _ in 0..10 {
                match verbindung.empfang().await? {
                    Some(sprechwerk_protocol::ServerFrame::Raeume { raeume }) => {
                        neu = raeume.iter().find(|r| r.name == name).map(|r| r.id);
                        break;
                    }
                    Some(sprechwerk_protocol::ServerFrame::Fehler { text, .. }) => {
                        anyhow::bail!("Raum anlegen abgelehnt: {text}")
                    }
                    Some(_) => continue,
                    None => break,
                }
            }
            let ziel = neu.context("neuer Raum nicht in der Liste")?;
            verbindung
                .sende(sprechwerk_protocol::ClientFrame::Betreten { raum: ziel })
                .await?;
            let _ = verbindung.empfang().await;
            println!("neuer Raum „{name}“ ({}), betreten", ziel);
        }

        if let Some(nummer) = raum {
            let ziel = verbindung
                .raum_nach_nummer(nummer.parse().unwrap_or(1))
                .context("Raum nicht gefunden")?;
            verbindung
                .sende(sprechwerk_protocol::ClientFrame::Betreten { raum: ziel })
                .await?;
            // Antwort abwarten (Betreten schickt den Verlauf mit).
            let _ = verbindung.empfang().await;
            println!("im Raum {nummer}");
        }

        let quelle = verbindung.datagramm_quelle();
        let leser = tokio::spawn(async move {
            let mut empfaenger = Empfänger::neu();
            let mut alle: Vec<i16> = Vec::new();
            let ende = Instant::now() + Duration::from_secs_f64(dauer + 1.0);
            let mut takt = Instant::now();
            while Instant::now() < ende {
                tokio::select! {
                    d = quelle.read_datagram() => {
                        if let Ok(d) = d {
                            // Ankunft mit Uhrzeit festhalten: alle Proben laufen
                            // auf demselben Rechner und teilen die Uhr, damit ist
                            // die Laufzeit messbar (nicht geschätzt).
                            if let Some((kopf, _)) = Sprachkopf::lesen(&d) {
                                let jetzt = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos())
                                    .unwrap_or(0);
                                println!("ANKUNFT {} {}", kopf.folge, jetzt);
                            }
                            empfaenger.nimm(&d);
                        }
                    }
                    _ = tokio::time::sleep_until((takt + Duration::from_millis(20)).into()) => {
                        takt += Duration::from_millis(20);
                        //Block abholen und sammeln (auch Stille, damit die Zeit stimmt)
                        alle.extend(empfaenger.naechster_block());
                    }
                }
            }
            (alle, empfaenger.zaehler())
        });

        if art == "ton" {
            let mut kodierer = Kodierer::neu()?;
            let mut folge: u16 = 0;
            let mut puffer = vec![0u8; 4000];
            let takt = Duration::from_millis(20);
            let mut nächster = Instant::now();
            let ende = Instant::now() + Duration::from_secs_f64(dauer);
            while Instant::now() < ende {
                let block: Vec<i16> = (0..RAHMEN_SAMPLES)
                    .map(|i| {
                        let t = (folge as f32 * RAHMEN_SAMPLES as f32 + i as f32)
                            / sprechwerk_audio::pcm::ABTASTRATE as f32;
                        let wert = 0.5 * (2.0 * std::f32::consts::PI * hz as f32 * t).sin();
                        (wert * i16::MAX as f32) as i16
                    })
                    .collect();
                let len = kodierer.kodiere(&block, &mut puffer)?;
                if folge == 0 {
                    let jetzt = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    println!("SENDEBEGINN {jetzt} RAHMENMS 20");
                }
                let datagramm = Sprachkopf::neu(folge).schreiben(&puffer[..len]);
                verbindung.sprache_senden(bytes::Bytes::from(datagramm))?;
                folge = folge.wrapping_add(1);
                nächster += takt;
                let jetzt = Instant::now();
                if nächster > jetzt {
                    tokio::time::sleep(nächster - jetzt).await;
                } else {
                    nächster = jetzt;
                }
            }
            println!("{dauer} s Ton ({hz} Hz) gesendet, {folge} Rahmen");
        }

        // Auch der Sender sammelt, damit die Dauer stimmt.
        let (alle, (angenommen, verworfen, sprecher)) = leser.await?;
        println!("angenommen: {angenommen}, verworfen: {verworfen}, Sprecher: {sprecher}");
        if let Some(pfad) = wav {
            sprechwerk_audio::pcm::wav_schreiben(&pfad, &alle)?;
            println!("Mitschnitt: {pfad} ({} Werte)", alle.len());
        }
        verbindung.schliessen();
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

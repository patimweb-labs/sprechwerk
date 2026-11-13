//! sprechwerk-cli — Referenzclient auf der Kommandozeile.
//!
//! Zwei Aufgaben: prüfen, dass der Server tut, was er soll, und Menschen ohne
//! Fensteranwendung einen Zugang geben.
//!
//! ```text
//! sprechwerk-cli --server 127.0.0.1:7443 --name Ada
//! ```
//!
//! Danach werden Zeilen gelesen: normaler Text geht in den Raum, `/befehle`
//! steuern den Client.

use anyhow::{Context, Result};
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_protocol::{ClientFrame, Fehlercode, ServerFrame};
use std::net::SocketAddr;

fn hilfe() {
    println!(
        r#"sprechwerk-cli — Referenzclient

  --server <adresse>   Adresse des Servers (Vorgabe 127.0.0.1:7443)
  --name <name>        Anzeigename (Vorgabe: Rechnername)
  --schluessel <datei> Schlüsseldatei (Vorgabe ~/.config/sprechwerk/schluessel)
  --passwort <wort>    Serverpasswort, falls nötig
  --raum <nummer>      Nach der Anmeldung diesen Raum betreten (1 = erster)
  --einmal <text>      Text senden, kurze Zeit zuhören, beenden (für Prüfläufe)
  --export             Nur den Schlüssel ausgeben (zum Mitnehmen) und beenden

Im laufenden Betrieb:
  /raeume              Raumliste
  /betreten <nummer>   Raum betreten
  /verlassen           Raum verlassen
  /anlegen <name>      Raum anlegen
  /protokoll           Prüfprotokoll der letzten Einträge
  /wer                 Wer ist da
  /hilfe               diese Übersicht
  /ende                beenden
"#
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut server: SocketAddr = "127.0.0.1:7443".parse().unwrap();
    let mut name = std::env::var("USER").unwrap_or_else(|_| "Gast".to_string());
    let mut schluessel_pfad = dirs_schluessel();
    let mut passwort: Option<String> = None;
    let mut raum: Option<usize> = None;
    let mut einmal: Option<String> = None;
    let mut nur_export = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => {
                server = args
                    .next()
                    .context("--server braucht einen Wert")?
                    .parse()?
            }
            "--name" => name = args.next().context("--name braucht einen Wert")?,
            "--schluessel" => {
                schluessel_pfad = std::path::PathBuf::from(args.next().context("Wert fehlt")?)
            }
            "--passwort" => passwort = args.next(),
            "--raum" => raum = Some(args.next().context("Wert fehlt")?.parse()?),
            "--einmal" => einmal = args.next(),
            "--export" => nur_export = true,
            "-h" | "--hilfe" => {
                hilfe();
                return Ok(());
            }
            anderes => anyhow::bail!("unbekannte Angabe: {anderes} (--hilfe zeigt die Übersicht)"),
        }
    }

    let schluessel = Schluessel::laden_oder_erzeugen(&schluessel_pfad)?;
    if nur_export {
        println!("{}", schluessel.export());
        eprintln!("# Kennung: {}", schluessel.kennung());
        return Ok(());
    }

    let mut verbindung = Verbindung::oeffnen(server, &schluessel, &name, passwort.as_deref())
        .await
        .context("Anmeldung")?;
    println!(
        "verbunden mit „{}“ als {} ({:?})",
        verbindung.server_name, name, verbindung.rolle
    );
    println!("Kennung: {}", schluessel.kennung());

    // Raumliste aus der Begrüßung zeigen.
    for (i, r) in verbindung.raeume.iter().enumerate() {
        println!("  {:>2}. {}", i + 1, r.name);
    }
    if let Some(nummer) = raum {
        let ziel = verbindung
            .raum_nach_nummer(nummer)
            .ok_or_else(|| anyhow::anyhow!("Raum {nummer} gibt es nicht"))?;
        verbindung
            .sende(ClientFrame::Betreten { raum: ziel })
            .await?;
        // Auf die Bestätigung warten, damit der Text im richtigen Raum landet.
        let frist = std::time::Duration::from_secs(3);
        while let Ok(Ok(Some(frame))) = tokio::time::timeout(frist, verbindung.empfang()).await {
            let fertig = matches!(
                frame,
                ServerFrame::Betreten { .. } | ServerFrame::Fehler { .. }
            );
            zeige(&frame);
            if fertig {
                break;
            }
        }
    }

    if let Some(text) = einmal {
        verbindung.sende(ClientFrame::Text { text }).await?;
        // Kurz zuhören, damit die eigene Nachricht zurückkommt.
        let frist = tokio::time::Instant::now() + std::time::Duration::from_millis(600);
        loop {
            let rest = frist.saturating_duration_since(tokio::time::Instant::now());
            if rest.is_zero() {
                break;
            }
            match tokio::time::timeout(rest, verbindung.empfang()).await {
                Ok(Ok(Some(frame))) => zeige(&frame),
                Ok(Ok(None)) => break,
                Ok(Err(fehler)) => return Err(fehler),
                Err(_) => break,
            }
        }
        verbindung.schliessen();
        return Ok(());
    }

    // Interaktiv: Eingabe und Empfang parallel.
    println!("Tippe Text oder /hilfe. Beenden mit /ende.");
    use tokio::io::AsyncBufReadExt;
    let mut zeilen = tokio::io::BufReader::new(tokio::io::stdin()).lines();

    loop {
        tokio::select! {
            zeile = zeilen.next_line() => {
                let Some(zeile) = zeile? else { break };
                let zeile = zeile.trim().to_string();
                if zeile.is_empty() { continue; }
                if zeile == "/ende" { break; }
                if zeile == "/hilfe" { hilfe(); continue; }
                if zeile == "/raeume" {
                    verbindung.sende(ClientFrame::Ping).await?;
                    println!("(Raumliste wird bei Änderungen geschickt)");
                    continue;
                }
                if let Some(rest) = zeile.strip_prefix("/betreten ") {
                    let nummer: usize = rest.trim().parse()?;
                    let raum = verbindung
                        .raum_nach_nummer(nummer)
                        .ok_or_else(|| anyhow::anyhow!("Raum {nummer} gibt es nicht"))?;
                    verbindung.sende(ClientFrame::Betreten { raum }).await?;
                    continue;
                }
                if zeile == "/verlassen" {
                    verbindung.sende(ClientFrame::Verlassen).await?;
                    continue;
                }
                if let Some(rest) = zeile.strip_prefix("/anlegen ") {
                    verbindung.sende(ClientFrame::RaumAnlegen { name: rest.trim().to_string() }).await?;
                    continue;
                }
                if zeile == "/protokoll" {
                    verbindung.sende(ClientFrame::Protokoll { ab: 0, anzahl: 20 }).await?;
                    continue;
                }
                if zeile == "/wer" { verbindung.sende(ClientFrame::Ping).await?; continue; }
                verbindung.sende(ClientFrame::Text { text: zeile }).await?;
            }
            rahmen = verbindung.empfang() => {
                match rahmen {
                    Ok(Some(frame)) => zeige(&frame),
                    Ok(None) => { println!("Server hat die Verbindung beendet"); break; }
                    Err(fehler) => { eprintln!("Fehler: {fehler}"); break; }
                }
            }
        }
    }

    verbindung.schliessen();
    Ok(())
}

fn zeige(frame: &ServerFrame) {
    match frame {
        ServerFrame::Schluesselpaket { von, raum, .. } => {
            println!("[Schlüsselpaket] von {von} für Raum {raum}");
        }
        ServerFrame::SchluesselAnfrage { von, raum } => {
            println!("[Schlüssel angefragt] von {von} für Raum {raum}");
        }
        ServerFrame::Sperrliste { sperren } => {
            println!("[Sperrliste] {} Einträge", sperren.len());
            for b in sperren {
                let wer = b
                    .schluessel
                    .clone()
                    .or_else(|| b.adresse.clone())
                    .unwrap_or_else(|| "?".into());
                println!("  {wer} — {} ({})", b.grund, b.handelnder_name);
            }
        }
        ServerFrame::Zaehler {
            sprachrahmen,
            verteilt,
            verloren,
            abgelehnt,
            verbundene,
        } => {
            println!(
                "[Zähler] {sprachrahmen} Rahmen, {verteilt} verteilt, {verloren} verloren, \
                 {abgelehnt} abgelehnt, {verbundene} verbunden"
            );
        }
        ServerFrame::Text { nachricht } => {
            let text = if nachricht.geloescht {
                "(gelöscht)".to_string()
            } else {
                nachricht.text.clone()
            };
            println!(
                "[{}] {}: {}",
                nachricht.zeit, nachricht.verfasser_name, text
            );
        }
        ServerFrame::Betreten {
            raum,
            verlauf,
            anwesend,
        } => {
            println!(
                "betreten: {raum} ({} anwesend, {} alte Nachrichten)",
                anwesend.len(),
                verlauf.len()
            );
        }
        ServerFrame::Anwesenheit {
            mitglied, anwesend, ..
        } => {
            println!(
                "{} {}",
                mitglied.name,
                if *anwesend { "kommt" } else { "geht" }
            );
        }
        ServerFrame::Raeume { raeume } => {
            println!("Räume:");
            for (i, r) in raeume.iter().enumerate() {
                println!(
                    "  {:>2}. {} ({} anwesend) {}",
                    i + 1,
                    r.name,
                    r.anwesend,
                    r.id
                );
            }
        }
        ServerFrame::MitgliedGeaendert { mitglied } => {
            println!("{} ist jetzt {:?}", mitglied.name, mitglied.rolle);
        }
        ServerFrame::Protokoll { eintraege } => {
            for e in eintraege {
                println!(
                    "{} {} {} {} → {}",
                    e.zeit, e.handelnder_name, e.aktion, e.ziel, e.ergebnis
                );
            }
        }
        ServerFrame::Fehler { code, text } => {
            let hinweis = match code {
                Fehlercode::KeinRecht => " (frag einen Moderator)",
                _ => "",
            };
            eprintln!("abgelehnt: {text}{hinweis}");
        }
        ServerFrame::Ok | ServerFrame::Pong => {}
        ServerFrame::NachrichtGeloescht { nachricht, hart } => {
            println!(
                "Nachricht {nachricht} gelöscht ({})",
                if *hart { "hart" } else { "Platzhalter" }
            );
        }
        ServerFrame::Willkommen { .. } | ServerFrame::Anforderung { .. } => {}
    }
}

fn dirs_schluessel() -> std::path::PathBuf {
    let basis = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let heim = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(heim).join(".config")
        });
    basis.join("sprechwerk").join("schluessel")
}

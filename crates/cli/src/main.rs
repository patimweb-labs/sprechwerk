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
use sprechwerk_adapters::serverschluessel::{Bund, Lesbar};
use sprechwerk_adapters::{Schluessel, Verbindung};
use sprechwerk_domain::Message;
use sprechwerk_protocol::inhalt::Inhalt;
use sprechwerk_protocol::{ClientFrame, Fehlercode, ServerFrame};
use std::net::SocketAddr;

/// Wie lange `--einmal` auf den Serverschlüssel wartet (er kommt von einem
/// anderen Mitglied, das online sein muss — ADR 0012).
const SCHLUESSEL_FRIST: std::time::Duration = std::time::Duration::from_secs(8);

/// Verbindung, Anmeldeschlüssel und Serverschlüssel zusammen: die
/// Schlüsselrahmen werden hier beantwortet, bevor etwas angezeigt wird.
struct Sitzung {
    verbindung: Verbindung,
    schluessel: Schluessel,
    bund: Bund,
    /// Verlauf, der noch nicht lesbar war: er kommt mit dem `Betreten`, der
    /// Serverschlüssel (ADR 0012) erst danach von einem anderen Mitglied.
    nachzutragen: Vec<Message>,
}

impl Sitzung {
    /// Nimmt einen Rahmen des Servers an: Schlüsselarbeit erledigen, zeigen.
    async fn nimm(&mut self, frame: &ServerFrame) -> Result<()> {
        let antworten = match frame {
            ServerFrame::Serverschluessel { stand } => self.bund.stand(stand.clone()),
            ServerFrame::ServerschluesselAnfrage { fuer, kennung, id } => {
                match self.bund.anfrage(*fuer, kennung, id, &self.schluessel) {
                    Ok(paket) => vec![paket],
                    Err(fehler) => {
                        eprintln!("Schlüssel nicht weitergegeben: {fehler}");
                        Vec::new()
                    }
                }
            }
            ServerFrame::ServerschluesselPaket {
                von_kennung,
                id,
                daten,
                unterschrift,
                ..
            } => match self
                .bund
                .paket(von_kennung, id, daten, unterschrift, &self.schluessel)
            {
                Ok(meldung) => {
                    println!("[Serverschlüssel erhalten]");
                    vec![meldung]
                }
                Err(fehler) => {
                    eprintln!("Schlüsselpaket abgelehnt: {fehler}");
                    Vec::new()
                }
            },
            _ => Vec::new(),
        };
        for a in antworten {
            self.verbindung.sende(a).await?;
        }
        if let Some(meldung) = self.bund.nachmelden() {
            self.verbindung.sende(meldung).await?;
        }
        zeige(frame, &self.bund);
        if let ServerFrame::Betreten { verlauf, .. } = frame {
            self.nachzutragen = verlauf.clone();
        }
        self.verlauf_nachreichen();
        Ok(())
    }

    /// Gibt den Verlauf aus, sobald der Serverschlüssel da ist.
    ///
    /// **Gefunden 2026-11-19:** der Client meldete „3 alte Nachrichten" und
    /// zeigte keine einzige — der Verlauf kam mit dem Betreten, der Schlüssel
    /// (ADR 0012) erst danach von einem anderen Mitglied. Bis dahin stünde bei
    /// jeder Zeile „Schlüssel fehlt noch", also wird nachgereicht, sobald
    /// nichts mehr fehlt.
    fn verlauf_nachreichen(&mut self) {
        if self.nachzutragen.is_empty() || !self.bund.alle_da() {
            return;
        }
        for nachricht in std::mem::take(&mut self.nachzutragen) {
            println!(
                "[{}] {}: {}",
                nachricht.zeit,
                nachricht.verfasser_name,
                lesbar(self.bund.lesen(&nachricht))
            );
        }
    }

    /// Verschlüsselt und sendet einen Text in den aktuellen Raum.
    async fn schreibe(&mut self, text: String) -> Result<()> {
        let raum = self
            .verbindung
            .raum
            .ok_or_else(|| anyhow::anyhow!("in keinem Raum"))?;
        let geheim = self
            .bund
            .verschluesseln(raum, self.verbindung.ich, &Inhalt::text(text))?;
        self.verbindung
            .sende(ClientFrame::Text {
                text: geheim,
                anhaenge: Vec::new(),
            })
            .await
    }

    /// Wartet, bis geschrieben werden kann (oder die Frist um ist).
    async fn warte_auf_schluessel(&mut self, frist: std::time::Duration) -> Result<()> {
        let ende = tokio::time::Instant::now() + frist;
        while !self.bund.kann_schreiben() {
            let rest = ende.saturating_duration_since(tokio::time::Instant::now());
            if rest.is_zero() {
                anyhow::bail!(
                    "kein Serverschlüssel: es ist kein Mitglied online, das ihn weitergeben kann"
                );
            }
            match tokio::time::timeout(
                rest.min(sprechwerk_adapters::serverschluessel::NACHMELDEN),
                self.verbindung.empfang(),
            )
            .await
            {
                Ok(Ok(Some(frame))) => self.nimm(&frame).await?,
                Ok(Ok(None)) => anyhow::bail!("Server hat die Verbindung beendet"),
                Ok(Err(fehler)) => return Err(fehler),
                Err(_) => {
                    if let Some(meldung) = self.bund.nachmelden() {
                        self.verbindung.sende(meldung).await?;
                    }
                }
            }
        }
        Ok(())
    }
}

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

    // Angehefteter Fingerabdruck, falls vorhanden (TOFU wie SSH):
    // `SPRECHWERK_FINGERABDRUCK=3ff0:5a78:…` — ohne den Wert wird angeheftet,
    // was der Server zeigt.
    let erwartet = std::env::var("SPRECHWERK_FINGERABDRUCK").ok();
    // Kein eigenes `context("Anmeldung")`: der Grund der Ablehnung („Passwort
    // falsch") ist die Meldung, die der Mensch lesen soll — er soll nicht
    // hinter einer Sammelüberschrift stehen.
    let mut verbindung = Verbindung::oeffnen_mit(
        server,
        &schluessel,
        &name,
        passwort.as_deref(),
        erwartet.as_deref(),
    )
    .await?;
    println!(
        "verbunden mit „{}“ als {} ({:?})",
        verbindung.server_name, name, verbindung.rolle
    );
    println!(
        "Zertifikat: {}",
        sprechwerk_protocol::gruppiere(&verbindung.zertifikat)
    );
    println!("Kennung: {}", schluessel.kennung());

    // Serverschlüssel: neben der Schlüsseldatei, je Server nach Fingerabdruck.
    let mut bund = Bund::laden(
        Some(schluessel_pfad.with_extension("serverschluessel.json")),
        &verbindung.zertifikat,
    );
    for rahmen in bund.stand(verbindung.serverschluessel.clone()) {
        verbindung.sende(rahmen).await?;
    }
    let raeume = verbindung.raeume.clone();
    let mut sitzung = Sitzung {
        verbindung,
        schluessel,
        bund,
        nachzutragen: Vec::new(),
    };

    // Raumliste aus der Begrüßung zeigen.
    for (i, r) in raeume.iter().enumerate() {
        println!("  {:>2}. {}", i + 1, r.name);
    }
    if let Some(nummer) = raum {
        let ziel = sitzung
            .verbindung
            .raum_nach_nummer(nummer)
            .ok_or_else(|| anyhow::anyhow!("Raum {nummer} gibt es nicht"))?;
        sitzung
            .verbindung
            .sende(ClientFrame::Betreten { raum: ziel })
            .await?;
        // Auf die Bestätigung warten, damit der Text im richtigen Raum landet.
        let frist = std::time::Duration::from_secs(3);
        while let Ok(Ok(Some(frame))) =
            tokio::time::timeout(frist, sitzung.verbindung.empfang()).await
        {
            let fertig = matches!(
                frame,
                ServerFrame::Betreten { .. } | ServerFrame::Fehler { .. }
            );
            sitzung.nimm(&frame).await?;
            if fertig {
                break;
            }
        }
    }

    if let Some(text) = einmal {
        sitzung.warte_auf_schluessel(SCHLUESSEL_FRIST).await?;
        sitzung.schreibe(text).await?;
        // Kurz zuhören, damit die eigene Nachricht zurückkommt — und Bitten um
        // den Schlüssel noch beantwortet werden.
        let frist = tokio::time::Instant::now() + std::time::Duration::from_millis(600);
        loop {
            let rest = frist.saturating_duration_since(tokio::time::Instant::now());
            if rest.is_zero() {
                break;
            }
            match tokio::time::timeout(rest, sitzung.verbindung.empfang()).await {
                Ok(Ok(Some(frame))) => sitzung.nimm(&frame).await?,
                Ok(Ok(None)) => break,
                Ok(Err(fehler)) => return Err(fehler),
                Err(_) => break,
            }
        }
        sitzung.verbindung.schliessen().await;
        beenden();
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
                    sitzung.verbindung.sende(ClientFrame::Ping).await?;
                    println!("(Raumliste wird bei Änderungen geschickt)");
                    continue;
                }
                if let Some(rest) = zeile.strip_prefix("/betreten ") {
                    let nummer: usize = rest.trim().parse()?;
                    let raum = sitzung.verbindung
                        .raum_nach_nummer(nummer)
                        .ok_or_else(|| anyhow::anyhow!("Raum {nummer} gibt es nicht"))?;
                    sitzung.verbindung.sende(ClientFrame::Betreten { raum }).await?;
                    continue;
                }
                if zeile == "/verlassen" {
                    sitzung.verbindung.sende(ClientFrame::Verlassen).await?;
                    continue;
                }
                if let Some(rest) = zeile.strip_prefix("/anlegen ") {
                    sitzung.verbindung.sende(ClientFrame::RaumAnlegen { name: rest.trim().to_string() }).await?;
                    continue;
                }
                if zeile == "/protokoll" {
                    sitzung.verbindung.sende(ClientFrame::Protokoll {
                        ab: 0,
                        anzahl: 20,
                        von_hinten: false,
                    }).await?;
                    continue;
                }
                if zeile == "/wer" { sitzung.verbindung.sende(ClientFrame::Ping).await?; continue; }
                if let Err(fehler) = sitzung.schreibe(zeile).await {
                    eprintln!("nicht gesendet: {fehler}");
                }
            }
            rahmen = sitzung.verbindung.empfang() => {
                match rahmen {
                    Ok(Some(frame)) => sitzung.nimm(&frame).await?,
                    Ok(None) => { println!("Server hat die Verbindung beendet"); break; }
                    Err(fehler) => { eprintln!("Fehler: {fehler}"); break; }
                }
            }
        }
    }

    sitzung.verbindung.schliessen().await;
    beenden();
}

/// Beendet den Client **nach** dem Abschied.
///
/// [`Verbindung::schliessen`] wartet bereits, bis das `CONNECTION_CLOSE` beim
/// Endpunkt hinaus ist. Danach bleibt nichts zu tun — und ein hartes Ende macht
/// sichtbar, dass der Abschied nicht davon abhängt, ob die Laufzeit beim
/// Aufräumen noch eine Runde dreht. Ohne diese Zeile ging das CLOSE-Paket auf
/// einer ausgelasteten Maschine verloren, und der Server hielt den Client die
/// vollen 30 s (Leerlauf-Timeout) für anwesend (gefunden 2026-11-19).
fn beenden() -> ! {
    std::process::exit(0)
}

fn zeige(frame: &ServerFrame, bund: &Bund) {
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
                println!("  {wer}: {} ({})", b.grund, b.handelnder_name);
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
                lesbar(bund.lesen(nachricht))
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
        ServerFrame::Serverschluessel { stand } => {
            if stand.aktuell.is_none() {
                println!("[Serverschlüssel wird erneuert]");
            }
        }
        ServerFrame::ServerschluesselAnfrage { .. } | ServerFrame::ServerschluesselPaket { .. } => {
        }
    }
}

/// Was von einer Nachricht angezeigt wird.
fn lesbar(l: Lesbar) -> String {
    match l {
        Lesbar::Inhalt(Inhalt::Text { text, .. }) => text,
        Lesbar::Inhalt(Inhalt::Reaktion { ziel, zeichen, weg }) => {
            format!(
                "{} {zeichen} auf {ziel}",
                if weg { "nimmt zurück:" } else { "reagiert" }
            )
        }
        // Der Referenzclient lädt keine Dateien; er sagt, dass welche da sind.
        Lesbar::Inhalt(Inhalt::Anhang { text, dateien }) => {
            let namen: Vec<String> = dateien
                .iter()
                .map(|d| format!("{} ({} Byte)", d.name, d.groesse))
                .collect();
            format!("{text} [Dateien: {}]", namen.join(", "))
                .trim()
                .to_string()
        }
        Lesbar::Unbekannt { art } => format!("(„{art}“: neuere Version nötig)"),
        Lesbar::SchluesselFehlt => "(verschlüsselt — Schlüssel fehlt noch)".into(),
        Lesbar::Unlesbar => "(unlesbar — verändert oder beschädigt)".into(),
        Lesbar::Leer => "(gelöscht)".into(),
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

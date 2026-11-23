// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Die Fassade für jede Oberfläche.
//!
//! Eine Oberfläche kennt zwei Dinge: den [`Zustand`] (was sie zeichnet) und
//! [`Aktion`] (was der Nutzer will). Alles dazwischen — Verbindung, TOFU,
//! Sprechpfad, Einstellungen — passiert hier. Ergebnisse kommen nicht als
//! Rückgabewert, sondern als [`Meldung`] über dieselbe Rinne wie der Zustand:
//! die Oberfläche blockiert nie und hält keinen eigenen Ablauf.

use crate::anwendung::Anwendung;
use crate::audio::{self, Geraeteliste};
use crate::dateien::Dateiablage;
use crate::einstellungen::Einstellungen;
use crate::spielnetz::{Spielbefehl, Spielnetzbetrieb, Standblick};
use crate::sprech::Sprechbetrieb;
use crate::verschluesselung::Schluesselverwaltung;
use sprechwerk_audio::Sendeeinstellungen;
use sprechwerk_ipc::{Antwort, Befehl, Verbindung, Zustand};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Was der Nutzer will. Ein Summentyp statt vierzig Funktionen: die
/// Oberfläche erzeugt Werte, die Fassade führt sie aus (AGENTS.md §2.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Aktion {
    /// Ein Befehl an den Server (Raum betreten, Text senden, …).
    Befehl(Befehl),
    /// Verbinden, mit TOFU und anschließendem Start des Sprechpfads.
    Verbinden {
        server: String,
        name: String,
        passwort: Option<String>,
    },
    /// Den angehefteten Fingerabdruck eines Servers vergessen.
    ZertifikatVergessen { server: String },
    /// Sprechpfad mit den gespeicherten Einstellungen starten.
    AudioStarten,
    /// Sprechpfad beenden.
    AudioStoppen,
    /// Push-to-Talk: Taste gedrückt (`true`) oder losgelassen.
    Sprechtaste(bool),
    /// Eigenes Mikrofon stumm.
    MikrofonStumm(bool),
    /// Eigenen Ton aus.
    HoerenAus(bool),
    /// Geräte auflisten.
    Geraete,
    /// Eingangspegel messen (Sprechtest).
    Pegel { geraet: String, millisekunden: u64 },
    /// Prüfton abspielen (Hörtest).
    Testton { geraet: String },
    /// Einstellungen speichern und auf den laufenden Pfad anwenden.
    EinstellungenSpeichern(Box<Einstellungen>),
    /// Dateien für den Entwurf wählen (Dialog des Systems).
    AnhaengeWaehlen,
    /// Eine Datei entschlüsselt speichern — wohin, fragt der Dialog.
    DateiSpeichernUnter {
        nachricht: sprechwerk_domain::MessageId,
        datei: String,
        name: String,
    },
    /// Eine Datei im Programm des Systems öffnen (Video, PDF, …).
    DateiExtern {
        nachricht: sprechwerk_domain::MessageId,
        datei: String,
        name: String,
    },
    /// Spielnetz (ADR 0016): Ports freigeben, beitreten, verlassen.
    Spielnetz(Spielbefehl),
}

impl Aktion {
    /// Kurzname für Meldungen („Verbinden fehlgeschlagen: …").
    pub fn name(&self) -> &'static str {
        match self {
            Aktion::Befehl(_) => "befehl",
            Aktion::Verbinden { .. } => "verbinden",
            Aktion::ZertifikatVergessen { .. } => "zertifikat_vergessen",
            Aktion::AudioStarten => "audio_starten",
            Aktion::AudioStoppen => "audio_stoppen",
            Aktion::Sprechtaste(_) => "sprechtaste",
            Aktion::MikrofonStumm(_) => "mikrofon_stumm",
            Aktion::HoerenAus(_) => "hoeren_aus",
            Aktion::Geraete => "geraete",
            Aktion::Pegel { .. } => "pegel",
            Aktion::Testton { .. } => "testton",
            Aktion::EinstellungenSpeichern(_) => "einstellungen_speichern",
            Aktion::AnhaengeWaehlen => "anhaenge_waehlen",
            Aktion::DateiSpeichernUnter { .. } => "datei_speichern",
            Aktion::DateiExtern { .. } => "datei_extern",
            Aktion::Spielnetz(befehl) => befehl.name(),
        }
    }
}

/// Was eine Aktion ergeben hat.
#[derive(Debug, Clone)]
pub enum Ergebnis {
    /// Ausgeführt; was sich geändert hat, steht im nächsten Zustand.
    Erledigt,
    /// Verbunden. `audio` ist ein Fehler des Sprechpfads — die Verbindung
    /// steht trotzdem, nur das Mikrofon bleibt still.
    Verbunden {
        server_name: String,
        audio: Option<String>,
    },
    Geraete(Geraeteliste),
    Pegel(u8),
}

/// Was die Fassade an die Oberfläche meldet.
#[derive(Debug, Clone)]
pub enum Meldung {
    /// Neuer vollständiger Stand (nach jeder Änderung).
    Zustand(Box<Zustand>),
    /// Eine Aktion ist fertig.
    Rueck {
        aktion: &'static str,
        ergebnis: Result<Ergebnis, String>,
    },
}

/// Stand des Sprechpfads (für die Anzeige im Takt).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct AudioStand {
    pub laeuft: bool,
    /// Mikrofonpegel 0–100.
    pub pegel: u8,
    pub stumm: bool,
    pub taub: bool,
}

type Melder = Arc<dyn Fn(Meldung) + Send + Sync>;

/// Der Client: eine Verbindung, ein Sprechpfad, eine Einstellungsdatei.
pub struct Client {
    laufzeit: tokio::runtime::Runtime,
    anwendung: Arc<Anwendung>,
    sprech: Arc<Sprechbetrieb>,
    ablage: Arc<Dateiablage>,
    einstellungen: PathBuf,
    /// Hierhin werden Dateien zum Öffnen im System entschlüsselt.
    geoeffnet: PathBuf,
    melde: Melder,
    /// Das Spielnetz (ADR 0016) — neben der Sitzung, nicht in ihr.
    spielnetz: Arc<Spielnetzbetrieb>,
    /// Die Einstellung Spielnetz, ohne bei jeder Zustandsmeldung die Datei zu
    /// lesen. Geschrieben wird sie nur in `EinstellungenSpeichern`.
    spielnetz_an: Arc<AtomicBool>,
}

/// Nur **eine** Pegelmessung gleichzeitig: zwei Aufnahmen am selben Gerät
/// bremsen den Audiopfad aus.
static MESSUNG: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Client {
    /// Startet den Client. `verzeichnis` hält Schlüssel und Einstellungen;
    /// `melde` bekommt jede [`Meldung`] (aus fremden Threads).
    ///
    /// `zwischenspeicher` hält die Anhänge — als Geheimtext, so wie sie vom
    /// Server kamen (ADR 0012 §3).
    pub fn starten<F>(verzeichnis: &Path, zwischenspeicher: &Path, melde: F) -> Self
    where
        F: Fn(Meldung) + Send + Sync + 'static,
    {
        let _ = std::fs::create_dir_all(verzeichnis);
        let ablage = Arc::new(Dateiablage::neu(zwischenspeicher.join("anhaenge")));
        // Was zum Öffnen entschlüsselt wurde, bleibt nicht liegen: beim
        // nächsten Start ist es weg.
        let geoeffnet = zwischenspeicher.join("geoeffnet");
        let _ = std::fs::remove_dir_all(&geoeffnet);
        let melde: Melder = Arc::new(melde);
        let sprech = Arc::new(Sprechbetrieb::neu());
        let verschluesselung = Arc::new(Schluesselverwaltung::neu());
        let einstellungen = verzeichnis.join("einstellungen.json");
        let laufzeit = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("sprechwerk-aktion")
            .enable_all()
            .build()
            .expect("Laufzeit für Aktionen");

        // ---- Spielnetz (ADR 0016) -------------------------------------
        //
        // Es hängt **neben** dem Sitzungsthread: der Sollzustand kommt aus der
        // Einstellung (hier einmal gelesen) und aus dem, was die Sitzung über
        // sich meldet. Ändert sich der Stand, wird der zuletzt gemeldete
        // Zustand mit neuem `spielnetz` noch einmal geschickt — die Oberfläche
        // zeichnet `render(modell)` und braucht dafür keine zweite Quelle.
        let spielnetz_an = Arc::new(AtomicBool::new(
            Einstellungen::laden(&einstellungen).spielnetz,
        ));
        let letzter: Arc<Mutex<Zustand>> = Arc::new(Mutex::new(Zustand::neu()));
        // Der Rückruf braucht den Stand, der Betrieb entsteht aber erst mit
        // ihm. Der Blick darauf wird deshalb nachgereicht: vorher gibt es
        // nichts zu melden (es wurde noch kein Zustand gemeldet).
        let blick: Arc<OnceLock<Standblick>> = Arc::new(OnceLock::new());
        let spielnetz =
            Spielnetzbetrieb::neu(laufzeit.handle().clone(), verzeichnis.join("schluessel"), {
                let blick = blick.clone();
                let letzter = letzter.clone();
                let melde = melde.clone();
                move || {
                    let Some(blick) = blick.get() else { return };
                    let stand = blick.stand();
                    let mut letzter = letzter.lock().expect("Sperre");
                    if letzter.spielnetz == stand {
                        return;
                    }
                    letzter.spielnetz = stand;
                    let zustand = letzter.clone();
                    drop(letzter);
                    melde(Meldung::Zustand(Box::new(zustand)));
                }
            });
        let _ = blick.set(spielnetz.blick());
        // Und gleich einmal abgleichen: ist die Einstellung an, steht der Stand
        // ab dem ersten Bild auf `Wartet` statt fälschlich auf `Aus`.
        spielnetz.abgleichen(spielnetz_an.load(Ordering::Relaxed), None);

        let an_zustand = melde.clone();
        let betrieb = spielnetz.clone();
        let an = spielnetz_an.clone();
        let letzter_anwendung = letzter.clone();
        let anwendung = Anwendung::starten(
            verzeichnis.join("schluessel"),
            sprech.clone(),
            verschluesselung,
            ablage.clone(),
            move |mut zustand| {
                // Sollzustand abgleichen, dann den Stand mitgeben. Beides vor
                // dem Melden: was die Oberfläche sieht, ist schon abgeglichen.
                betrieb.abgleichen(an.load(Ordering::Relaxed), sitzung_aus(&zustand));
                zustand.spielnetz = betrieb.stand();
                *letzter_anwendung.lock().expect("Sperre") = zustand.clone();
                an_zustand(Meldung::Zustand(Box::new(zustand)));
            },
        );
        Self {
            laufzeit,
            anwendung: Arc::new(anwendung),
            sprech,
            ablage,
            einstellungen,
            geoeffnet,
            melde,
            spielnetz,
            spielnetz_an,
        }
    }

    /// Der zuletzt gemeldete Stand (mit dem aktuellen Spielnetz).
    pub fn zustand(&self) -> Zustand {
        let mut zustand = self.anwendung.zustand();
        zustand.spielnetz = self.spielnetz.stand();
        zustand
    }

    /// Die gespeicherten Einstellungen.
    pub fn einstellungen(&self) -> Einstellungen {
        Einstellungen::laden(&self.einstellungen)
    }

    /// Stand des Sprechpfads — billig, darf in jedem Bild gelesen werden.
    pub fn audio_stand(&self) -> AudioStand {
        AudioStand {
            laeuft: self.sprech.laeuft(),
            pegel: self.sprech.pegel(),
            stumm: self.sprech.ist_stumm(),
            taub: self.sprech.ist_taub(),
        }
    }

    /// Eine Datei entschlüsselt im Speicher (für Vorschau und Betrachter).
    /// `None`, solange sie nicht geladen ist. Auf die Platte kommt dabei kein
    /// Klartext.
    pub fn datei(&self, id: &str) -> Option<Vec<u8>> {
        let h = self.ablage.antwort(&format!("/{id}"), None);
        (h.status == 200).then_some(h.inhalt)
    }

    /// Führt eine Aktion aus, ohne den Aufrufer zu blockieren. Das Ergebnis
    /// kommt als [`Meldung::Rueck`].
    pub fn ausfuehren(&self, aktion: Aktion) {
        let name = aktion.name();
        let anwendung = self.anwendung.clone();
        let sprech = self.sprech.clone();
        let pfad = self.einstellungen.clone();
        let geoeffnet = self.geoeffnet.clone();
        let melde = self.melde.clone();
        let spielnetz = self.spielnetz.clone();
        let spielnetz_an = self.spielnetz_an.clone();
        self.laufzeit.spawn(async move {
            let ergebnis = match aktion {
                Aktion::AnhaengeWaehlen => anhaenge_waehlen(&anwendung).await,
                Aktion::DateiSpeichernUnter {
                    nachricht,
                    datei,
                    name,
                } => speichern_unter(&anwendung, nachricht, datei, name).await,
                Aktion::DateiExtern {
                    nachricht,
                    datei,
                    name,
                } => extern_oeffnen(&anwendung, &geoeffnet, nachricht, datei, name).await,
                andere => {
                    ausfuehren(
                        andere,
                        &anwendung,
                        &sprech,
                        &pfad,
                        &spielnetz,
                        &spielnetz_an,
                    )
                    .await
                }
            };
            melde(Meldung::Rueck {
                aktion: name,
                ergebnis,
            });
        });
    }
}

async fn ausfuehren(
    aktion: Aktion,
    anwendung: &Anwendung,
    sprech: &Arc<Sprechbetrieb>,
    pfad: &Path,
    spielnetz: &Arc<Spielnetzbetrieb>,
    spielnetz_an: &Arc<AtomicBool>,
) -> Result<Ergebnis, String> {
    match aktion {
        Aktion::Befehl(befehl) => match anwendung.befehl(befehl).await {
            Antwort::Fehler { text } => Err(text),
            _ => Ok(Ergebnis::Erledigt),
        },
        Aktion::Spielnetz(befehl) => spielnetz.befehl(befehl).await.map(|()| Ergebnis::Erledigt),
        Aktion::Verbinden {
            server,
            name,
            passwort,
        } => verbinden(anwendung, sprech, pfad, server, name, passwort).await,
        Aktion::ZertifikatVergessen { server } => {
            let mut e = Einstellungen::laden(pfad);
            e.vergiss_vertrauen(&server);
            e.speichern(pfad).map(|_| Ergebnis::Erledigt)
        }
        Aktion::AudioStarten => {
            audio_starten_mit(sprech, &Einstellungen::laden(pfad)).map(|_| Ergebnis::Erledigt)
        }
        Aktion::AudioStoppen => {
            sprech.stoppen();
            Ok(Ergebnis::Erledigt)
        }
        Aktion::Sprechtaste(an) => {
            sprech.taste_setzen(an);
            Ok(Ergebnis::Erledigt)
        }
        Aktion::MikrofonStumm(an) => {
            sprech.stumm_setzen(an);
            Ok(Ergebnis::Erledigt)
        }
        Aktion::HoerenAus(an) => {
            sprech.taub_setzen(an);
            Ok(Ergebnis::Erledigt)
        }
        Aktion::Geraete => {
            blockierend(|| {
                Ok(Ergebnis::Geraete(Geraeteliste {
                    mikrofon: audio::mikrofon_geraete().map_err(|e| e.to_string())?,
                    lautsprecher: audio::lautsprecher_geraete().map_err(|e| e.to_string())?,
                }))
            })
            .await
        }
        Aktion::Pegel {
            geraet,
            millisekunden,
        } => {
            // Läuft der Pfad, steht der Wert schon bereit — kein zweiter
            // Aufnahmeprozess am selben Gerät.
            if sprech.laeuft() {
                return Ok(Ergebnis::Pegel(sprech.pegel()));
            }
            blockierend(move || {
                let Ok(_frei) = MESSUNG.try_lock() else {
                    return Err("Es läuft schon eine Messung".to_string());
                };
                audio::pegel(&geraet, millisekunden)
                    .map(Ergebnis::Pegel)
                    .map_err(|e| e.to_string())
            })
            .await
        }
        Aktion::Testton { geraet } => {
            blockierend(move || {
                audio::testton(&geraet)
                    .map(|_| Ergebnis::Erledigt)
                    .map_err(|e| e.to_string())
            })
            .await
        }
        Aktion::AnhaengeWaehlen
        | Aktion::DateiSpeichernUnter { .. }
        | Aktion::DateiExtern { .. } => {
            unreachable!("in Client::ausfuehren behandelt")
        }
        Aktion::EinstellungenSpeichern(einstellungen) => {
            einstellungen.speichern(pfad)?;
            if sprech.laeuft() {
                sprech.push_to_talk_setzen(einstellungen.push_to_talk);
                sprech.wende_an(
                    Some(einstellungen.mikrofon.clone()),
                    Some(einstellungen.lautsprecher.clone()),
                    sende_einstellungen(&einstellungen),
                )?;
            }
            // Spielnetz: an/aus übernehmen und sofort abgleichen (ADR 0016 §3).
            // Ohne das wartete ein Einschalten, bis die Sitzung das nächste Mal
            // etwas meldet.
            spielnetz_an.store(einstellungen.spielnetz, Ordering::Relaxed);
            spielnetz.abgleichen(einstellungen.spielnetz, sitzung_aus(&anwendung.zustand()));
            Ok(Ergebnis::Erledigt)
        }
    }
}

fn befehl_ergebnis(antwort: Antwort) -> Result<Ergebnis, String> {
    match antwort {
        Antwort::Fehler { text } => Err(text),
        _ => Ok(Ergebnis::Erledigt),
    }
}

/// Die Sitzung, mit der das Spielnetz verbunden wird: nur wenn die Verbindung
/// **steht** und das Zertifikat bekannt ist (TOFU, ADR 0002 §3) — ohne
/// Fingerabdruck gäbe es keinen Handschlag, dem man glauben kann.
fn sitzung_aus(zustand: &Zustand) -> Option<(String, String)> {
    let Verbindung::Verbunden { server, .. } = zustand.verbindung.as_ref()? else {
        return None;
    };
    let fingerabdruck = zustand.zertifikat.clone()?;
    Some((server.clone(), fingerabdruck))
}

/// Dateien wählen (Dialog des Systems) und an den Entwurf hängen. Abbrechen
/// ist kein Fehler.
async fn anhaenge_waehlen(anwendung: &Anwendung) -> Result<Ergebnis, String> {
    let Some(gewaehlt) = rfd::AsyncFileDialog::new().pick_files().await else {
        return Ok(Ergebnis::Erledigt);
    };
    let pfade = gewaehlt
        .iter()
        .map(|d| d.path().display().to_string())
        .collect();
    befehl_ergebnis(
        anwendung
            .befehl(Befehl::AnhaengeHinzufuegen { pfade })
            .await,
    )
}

/// Erst fragt der Dialog wohin (Name aus dem Umschlag als Vorschlag), dann
/// lädt und entschlüsselt der Verbindungsteil.
async fn speichern_unter(
    anwendung: &Anwendung,
    nachricht: sprechwerk_domain::MessageId,
    datei: String,
    name: String,
) -> Result<Ergebnis, String> {
    let Some(ziel) = rfd::AsyncFileDialog::new()
        .set_file_name(&name)
        .save_file()
        .await
    else {
        return Ok(Ergebnis::Erledigt);
    };
    befehl_ergebnis(
        anwendung
            .befehl(Befehl::DateiSpeichern {
                nachricht,
                datei,
                ziel: ziel.path().display().to_string(),
            })
            .await,
    )
}

/// Entschlüsselt die Datei in einen eigenen Ordner und öffnet sie im
/// Programm des Systems (Video-Player, PDF-Betrachter …). Der Ordner wird
/// beim nächsten Start geleert.
async fn extern_oeffnen(
    anwendung: &Anwendung,
    geoeffnet: &Path,
    nachricht: sprechwerk_domain::MessageId,
    datei: String,
    name: String,
) -> Result<Ergebnis, String> {
    let ordner = geoeffnet.join(&datei);
    std::fs::create_dir_all(&ordner).map_err(|f| f.to_string())?;
    // Nur der letzte Teil des Namens — er kommt von einem anderen Mitglied.
    let sicher = Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "datei".into());
    let ziel = ordner.join(sicher);
    befehl_ergebnis(
        anwendung
            .befehl(Befehl::DateiSpeichern {
                nachricht,
                datei,
                ziel: ziel.display().to_string(),
            })
            .await,
    )?;
    open::that_detached(&ziel).map_err(|f| f.to_string())?;
    Ok(Ergebnis::Erledigt)
}

/// Verbindet mit TOFU (ADR 0002 §3): **vorher** den angehefteten
/// Fingerabdruck mitgeben — er entscheidet im Handschlag, bevor Kennung oder
/// Passwort über die Leitung gehen; **nachher** beim ersten Kontakt anheften.
async fn verbinden(
    anwendung: &Anwendung,
    sprech: &Arc<Sprechbetrieb>,
    pfad: &Path,
    server: String,
    name: String,
    passwort: Option<String>,
) -> Result<Ergebnis, String> {
    let erwartet = Einstellungen::laden(pfad)
        .vertrauen_fuer(&server)
        .map(str::to_string);
    let server_name = anwendung
        .verbinden(server.clone(), name.clone(), passwort, erwartet)
        .await?;
    let mut e = Einstellungen::laden(pfad);
    // Derselbe Wert, der geprüft wurde — nicht ein zweimal berechneter.
    if let Some(fingerabdruck) = anwendung.zustand().zertifikat {
        e.merke_vertrauen(&server, &fingerabdruck);
    }
    e.merke_verbindung(&server);
    e.name = name;
    let _ = e.speichern(pfad);
    // Ein Fehler im Sprechpfad lässt das Verbinden nicht scheitern.
    let audio = audio_starten_mit(sprech, &e).err();
    Ok(Ergebnis::Verbunden { server_name, audio })
}

/// Geräteabfragen starten Prozesse (`pactl`) — nie auf einem Arbeitsfaden
/// der Laufzeit, der sonst andere Aktionen aufhält.
async fn blockierend<F>(f: F) -> Result<Ergebnis, String>
where
    F: FnOnce() -> Result<Ergebnis, String> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Startet den Pfad mit den übergebenen Einstellungen.
pub fn audio_starten_mit(sprech: &Sprechbetrieb, e: &Einstellungen) -> Result<(), String> {
    sprech.push_to_talk_setzen(e.push_to_talk);
    sprech.starten(
        Some(e.mikrofon.clone()),
        Some(e.lautsprecher.clone()),
        sende_einstellungen(e),
    )
}

/// Übersetzt die Einstellungen in die Sprache des Audiopfads.
pub fn sende_einstellungen(e: &Einstellungen) -> Sendeeinstellungen {
    Sendeeinstellungen {
        entrauschen: e.rauschunterdrueckung,
        noisegate: e.noisegate,
        schwelle: e.schwelle,
        vorlauf_ms: e.vorlauf_ms as u32,
        paritaet: true,
        echounterdrueckung: e.echounterdrueckung,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn verzeichnis(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sprechwerk-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn warte_auf_rueck(rx: &mpsc::Receiver<Meldung>) -> (&'static str, Result<Ergebnis, String>) {
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).expect("Meldung") {
                Meldung::Rueck { aktion, ergebnis } => return (aktion, ergebnis),
                Meldung::Zustand(_) => continue,
            }
        }
    }

    #[test]
    fn neuer_client_ist_getrennt() {
        let d = verzeichnis("neu");
        let client = Client::starten(&d, &d.join("zwischen"), |_| {});
        assert_eq!(
            client.zustand().verbindung,
            Some(sprechwerk_ipc::Verbindung::Getrennt)
        );
        assert!(!client.audio_stand().laeuft);
    }

    #[test]
    fn befehl_ohne_verbindung_meldet_fehler_statt_zu_haengen() {
        let d = verzeichnis("ohne");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let client = Client::starten(&d, &d.join("zwischen"), move |m| {
            let _ = tx.lock().unwrap().send(m);
        });
        client.ausfuehren(Aktion::Befehl(Befehl::Ping));
        let (aktion, ergebnis) = warte_auf_rueck(&rx);
        assert_eq!(aktion, "befehl");
        assert!(ergebnis.is_err(), "ohne Verbindung kein Ping: {ergebnis:?}");
    }

    #[test]
    fn einstellungen_speichern_und_lesen() {
        let d = verzeichnis("einst");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let client = Client::starten(&d, &d.join("zwischen"), move |m| {
            let _ = tx.lock().unwrap().send(m);
        });
        let mut e = client.einstellungen();
        e.name = "Ada".into();
        e.thema = "mono".into();
        client.ausfuehren(Aktion::EinstellungenSpeichern(Box::new(e)));
        let (_, ergebnis) = warte_auf_rueck(&rx);
        assert!(ergebnis.is_ok());
        assert_eq!(client.einstellungen().name, "Ada");
        assert_eq!(client.einstellungen().thema, "mono");
    }

    #[test]
    fn zertifikat_vergessen_entfernt_die_anheftung() {
        let d = verzeichnis("tofu");
        let pfad = d.join("einstellungen.json");
        std::fs::create_dir_all(&d).unwrap();
        let mut e = Einstellungen::default();
        e.merke_vertrauen("a:1", &"ab".repeat(32));
        e.speichern(&pfad).unwrap();
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let client = Client::starten(&d, &d.join("zwischen"), move |m| {
            let _ = tx.lock().unwrap().send(m);
        });
        assert!(client.einstellungen().vertrauen_fuer("a:1").is_some());
        client.ausfuehren(Aktion::ZertifikatVergessen {
            server: "a:1".into(),
        });
        let _ = warte_auf_rueck(&rx);
        assert!(client.einstellungen().vertrauen_fuer("a:1").is_none());
    }

    // ------------------------------------------------------------- Spielnetz

    /// Sammelt Meldungen, bis eine Aktion zurückkommt, und gibt die dabei
    /// gemeldeten Spielnetz-Stände mit zurück.
    fn staende_bis_rueck(rx: &mpsc::Receiver<Meldung>) -> Vec<sprechwerk_ipc::Spielnetz> {
        let mut staende = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).expect("Meldung") {
                Meldung::Zustand(zustand) => staende.push(zustand.spielnetz),
                Meldung::Rueck { ergebnis, .. } => {
                    assert!(ergebnis.is_ok(), "Aktion fehlgeschlagen: {ergebnis:?}");
                    return staende;
                }
            }
        }
    }

    #[test]
    fn spielnetz_ist_in_der_vorgabe_aus() {
        // ADR 0016 §3: ohne Zutun des Nutzers läuft kein Modul und kein Port.
        let d = verzeichnis("spielnetz-aus");
        let client = Client::starten(&d, &d.join("zwischen"), |_| {});
        assert_eq!(client.zustand().spielnetz, sprechwerk_ipc::Spielnetz::Aus);
        assert!(!client.einstellungen().spielnetz);
    }

    #[test]
    fn spielnetz_einschalten_wird_gemeldet_und_laesst_sich_ausschalten() {
        let d = verzeichnis("spielnetz-an");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let client = Client::starten(&d, &d.join("zwischen"), move |m| {
            let _ = tx.lock().unwrap().send(m);
        });

        let mut e = client.einstellungen();
        e.spielnetz = true;
        client.ausfuehren(Aktion::EinstellungenSpeichern(Box::new(e)));
        let staende = staende_bis_rueck(&rx);
        assert_eq!(
            staende.last(),
            Some(&sprechwerk_ipc::Spielnetz::Wartet),
            "eingeschaltet, aber ohne Sitzung: {staende:?}"
        );
        assert_eq!(
            client.zustand().spielnetz,
            sprechwerk_ipc::Spielnetz::Wartet
        );
        assert!(client.einstellungen().spielnetz, "die Einstellung bleibt");

        // Und wieder aus: der Stand folgt sofort, ohne Sitzung und ohne Port.
        let mut e = client.einstellungen();
        e.spielnetz = false;
        client.ausfuehren(Aktion::EinstellungenSpeichern(Box::new(e)));
        let staende = staende_bis_rueck(&rx);
        assert_eq!(
            staende.last(),
            Some(&sprechwerk_ipc::Spielnetz::Aus),
            "ausgeschaltet: {staende:?}"
        );
        assert_eq!(client.zustand().spielnetz, sprechwerk_ipc::Spielnetz::Aus);
    }

    #[test]
    fn ohne_sitzung_gibt_es_keinen_spielnetz_befehl() {
        let d = verzeichnis("spielnetz-befehl");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let client = Client::starten(&d, &d.join("zwischen"), move |m| {
            let _ = tx.lock().unwrap().send(m);
        });
        client.ausfuehren(Aktion::Spielnetz(Spielbefehl::Beitreten { angebot: 1 }));
        let (aktion, ergebnis) = warte_auf_rueck(&rx);
        assert_eq!(aktion, "beitreten");
        let fehler = ergebnis.expect_err("ohne Verbindung kein Beitreten");
        assert!(fehler.contains("nicht verbunden"), "{fehler}");
    }

    #[test]
    fn sitzung_braucht_verbindung_und_zertifikat() {
        use sprechwerk_domain::{MemberId, Role};
        let mut zustand = Zustand::neu();
        assert!(sitzung_aus(&zustand).is_none(), "getrennt: keine Sitzung");
        zustand.verbindung = Some(Verbindung::Verbunden {
            server: "192.0.2.10:7443".into(),
            server_name: "Verein".into(),
            ich: MemberId::aus_roh(1),
            name: "Ada".into(),
            rolle: Role::Mitglied,
            kennung: "ab".repeat(32),
        });
        assert!(
            sitzung_aus(&zustand).is_none(),
            "ohne angeheftetes Zertifikat kein Handschlag, dem man glauben kann"
        );
        zustand.zertifikat = Some("3ff0".repeat(16));
        assert_eq!(
            sitzung_aus(&zustand),
            Some(("192.0.2.10:7443".to_string(), "3ff0".repeat(16)))
        );
    }
}

// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Das Modell des Fensters: der Zustand des Clients plus das, was nur die
//! Oberfläche betrifft (welche Ansicht, welcher Dialog, was im Eingabefeld
//! steht). Alles als Summentypen (AGENTS.md §2.2); die Ansichten lesen nur.

use crate::texte::{t, Sprache};
use crate::thema::Thema;
use sprechwerk_client::audio::Geraeteliste;
use sprechwerk_client::einstellungen::{Einstellungen, Favorit};
use sprechwerk_client::sitzung::{Aktion, Ergebnis, Meldung};
use sprechwerk_client::AudioStand;
use sprechwerk_domain::{MemberId, Role, RoomId};
use sprechwerk_ipc::{Befehl, Zustand};

/// Welche Ansicht offen ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ansicht {
    #[default]
    Server,
    Raeume,
    Protokoll,
    Sperren,
    Einstellungen,
}

/// Der offene Dialog — höchstens einer.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Dialog {
    #[default]
    Keiner,
    ServerHinzufuegen {
        name: String,
        adresse: String,
    },
    ServerEntfernen {
        server: String,
        name: String,
    },
    RaumAnlegen {
        name: String,
    },
    RaumUmbenennen {
        raum: RoomId,
        name: String,
    },
    RaumLoeschen {
        raum: RoomId,
        name: String,
    },
    Verschieben {
        mitglied: MemberId,
        name: String,
        ziel: Option<RoomId>,
    },
    Rolle {
        mitglied: MemberId,
        name: String,
        rolle: Role,
    },
    Trennen {
        mitglied: MemberId,
        name: String,
        grund: String,
    },
    Sperren {
        mitglied: MemberId,
        name: String,
        grund: String,
    },
}

/// Etwas, das geladen wird.
#[derive(Debug, Clone, Default)]
pub enum Laden<T> {
    #[default]
    Nie,
    Laedt,
    Fertig(T),
    Fehler(String),
}

/// Hinweis, der nicht aus dem Server kommt (lokale Rückmeldung).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hinweis {
    Fehler(String),
    Info(String),
}

/// Das ganze Modell des Fensters.
#[derive(Debug, Clone, Default)]
pub struct Oberflaeche {
    /// Stand des Clients — die einzige Wahrheit über Server, Räume, Chat.
    pub zustand: Zustand,
    /// Einstellungen (Entwurf; gespeichert wird mit `Speichern`).
    pub einstellungen: Einstellungen,
    pub ansicht: Ansicht,
    pub dialog: Dialog,
    /// Text im Eingabefeld des Chats.
    pub entwurf: String,
    /// Zuletzt an den Client gemeldeter Link des Entwurfs.
    pub entwurf_link: Option<String>,
    pub geraete: Laden<Geraeteliste>,
    /// Läuft der Sprechtest (Aufnahme ohne Verbindung, nur für den Pegel)?
    pub sprechtest: bool,
    pub audio: AudioStand,
    /// Wartet die Einstellungsseite auf die neue Sprechtaste?
    pub taste_aufnehmen: bool,
    /// Ist die Sprechtaste gerade gedrückt?
    pub spricht: bool,
    pub hinweis: Option<Hinweis>,
}

impl Oberflaeche {
    pub fn neu(einstellungen: Einstellungen, zustand: Zustand) -> Self {
        Self {
            zustand,
            einstellungen,
            ..Default::default()
        }
    }

    pub fn sprache(&self) -> Sprache {
        Sprache::aus_code(&self.einstellungen.sprache)
    }

    pub fn thema(&self) -> Thema {
        Thema::aus_name(&self.einstellungen.thema)
    }

    pub fn verbunden(&self) -> bool {
        self.zustand
            .verbindung
            .as_ref()
            .is_some_and(|v| v.ist_verbunden())
    }

    /// Verarbeitet eine Meldung des Clients. Reine Funktion über dem Modell.
    pub fn melden(&mut self, meldung: Meldung) {
        let sprache = self.sprache();
        match meldung {
            Meldung::Zustand(neu) => {
                let vorher = self
                    .zustand
                    .raeume
                    .iter()
                    .find(|r| r.hier)
                    .map(|r| r.verschluesselt);
                let nachher = neu.raeume.iter().find(|r| r.hier).map(|r| r.verschluesselt);
                if let (Some(a), Some(b)) = (vorher, nachher) {
                    if a != b {
                        let k = if b {
                            "room.encrypted_now"
                        } else {
                            "room.unencrypted_now"
                        };
                        self.hinweis = Some(Hinweis::Info(t(sprache, k)));
                    }
                }
                let war_verbunden = self.verbunden();
                self.zustand = *neu;
                if war_verbunden && !self.verbunden() {
                    self.ansicht = Ansicht::Server;
                    self.dialog = Dialog::Keiner;
                }
            }
            Meldung::Rueck { aktion, ergebnis } => match ergebnis {
                Err(text) => {
                    match aktion {
                        "geraete" => self.geraete = Laden::Fehler(text.clone()),
                        "audio_starten" => self.sprechtest = false,
                        _ => {}
                    }
                    self.hinweis = Some(Hinweis::Fehler(text));
                }
                Ok(Ergebnis::Verbunden { audio, .. }) => {
                    self.ansicht = Ansicht::Raeume;
                    self.hinweis = audio.map(Hinweis::Fehler);
                }
                Ok(Ergebnis::Geraete(liste)) => self.geraete = Laden::Fertig(liste),
                Ok(Ergebnis::Pegel(_)) => {}
                Ok(Ergebnis::Erledigt) => match aktion {
                    "testton" => self.hinweis = Some(Hinweis::Info(t(sprache, "set.speaker.done"))),
                    "zertifikat_vergessen" => {
                        self.hinweis =
                            Some(Hinweis::Info(t(sprache, "server.certificate_forgotten")))
                    }
                    // Ein gelungener Befehl räumt eine alte Fehlermeldung weg.
                    _ => {
                        if matches!(self.hinweis, Some(Hinweis::Fehler(_))) {
                            self.hinweis = None;
                        }
                    }
                },
            },
        }
    }

    /// Sprechtest an oder aus. Mit Verbindung läuft die Aufnahme ohnehin —
    /// dann gibt es nichts zu tun.
    pub fn sprechtest(&mut self, an: bool) -> Option<Aktion> {
        if self.verbunden() || self.sprechtest == an {
            return None;
        }
        self.sprechtest = an;
        Some(if an {
            Aktion::AudioStarten
        } else {
            Aktion::AudioStoppen
        })
    }

    /// Wer die Einstellungen verlässt, beendet den Sprechtest: ohne
    /// Verbindung soll das Mikrofon nicht offen bleiben.
    pub fn sprechtest_aufraeumen(&mut self) -> Option<Aktion> {
        if self.sprechtest && (self.ansicht != Ansicht::Einstellungen || self.verbunden()) {
            self.sprechtest = false;
            if !self.verbunden() {
                return Some(Aktion::AudioStoppen);
            }
        }
        None
    }

    /// Eine Ansicht öffnen. Was sie zeigt, wird dabei angefordert — die
    /// Antwort landet im Modell, nicht in der Ansicht.
    pub fn oeffnen(&mut self, ansicht: Ansicht) -> Option<Aktion> {
        self.ansicht = ansicht;
        match ansicht {
            Ansicht::Protokoll => Some(Aktion::Befehl(Befehl::Protokoll { ab: 0, anzahl: 50 })),
            Ansicht::Sperren => Some(Aktion::Befehl(Befehl::Sperrliste)),
            Ansicht::Einstellungen if matches!(self.geraete, Laden::Nie) => {
                self.geraete = Laden::Laedt;
                Some(Aktion::Geraete)
            }
            _ => None,
        }
    }

    /// Verbinden mit dem gewählten Server.
    pub fn verbinden(&mut self, server: &str) -> Aktion {
        self.einstellungen.server = server.to_string();
        self.hinweis = None;
        Aktion::Verbinden {
            server: server.to_string(),
            name: self.einstellungen.name.trim().to_string(),
            passwort: Some(self.einstellungen.passwort.clone()).filter(|p| !p.is_empty()),
        }
    }

    /// Den Entwurf abschicken. `None`, wenn nichts zu senden ist.
    pub fn senden(&mut self) -> Option<Aktion> {
        let text = self.entwurf.trim().to_string();
        // Nur Dateien, ohne Text, ist auch eine Nachricht.
        if text.is_empty() && self.zustand.anhaenge.is_empty() {
            return None;
        }
        self.entwurf.clear();
        self.entwurf_link = None;
        Some(Aktion::Befehl(Befehl::Text { text }))
    }

    /// Nach jeder Änderung des Entwurfs: hat sich der erste Link geändert?
    /// Dann muss der Client die Vorschau neu holen (oder verwerfen).
    pub fn entwurf_geaendert(&mut self) -> Option<Aktion> {
        let link = if self.einstellungen.link_vorschau {
            crate::logik::erster_link(&self.entwurf)
        } else {
            None
        };
        if link == self.entwurf_link {
            return None;
        }
        self.entwurf_link = link.clone();
        Some(Aktion::Befehl(Befehl::Vorschau { url: link }))
    }

    /// Favorit hinzufügen oder umbenennen.
    pub fn favorit_merken(&mut self, name: &str, adresse: &str) -> Option<Aktion> {
        let adresse = adresse.trim();
        if adresse.is_empty() {
            return None;
        }
        let name = if name.trim().is_empty() {
            adresse
        } else {
            name.trim()
        };
        match self
            .einstellungen
            .favoriten
            .iter_mut()
            .find(|f| f.server == adresse)
        {
            Some(f) => f.name = name.to_string(),
            None => self.einstellungen.favoriten.push(Favorit {
                name: name.to_string(),
                server: adresse.to_string(),
                zuletzt: 0,
            }),
        }
        Some(self.speichern())
    }

    pub fn favorit_entfernen(&mut self, server: &str) -> Aktion {
        self.einstellungen.favoriten.retain(|f| f.server != server);
        self.speichern()
    }

    pub fn speichern(&self) -> Aktion {
        Aktion::EinstellungenSpeichern(Box::new(self.einstellungen.clone()))
    }

    /// Der bestätigte Dialog als Aktion; schließt den Dialog.
    pub fn dialog_bestaetigen(&mut self) -> Option<Aktion> {
        let dialog = std::mem::take(&mut self.dialog);
        let befehl = match dialog {
            Dialog::Keiner => return None,
            Dialog::ServerHinzufuegen { name, adresse } => {
                return self.favorit_merken(&name, &adresse)
            }
            Dialog::ServerEntfernen { server, .. } => return Some(self.favorit_entfernen(&server)),
            Dialog::RaumAnlegen { name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                Befehl::RaumAnlegen { name }
            }
            Dialog::RaumUmbenennen { raum, name } => Befehl::RaumUmbenennen {
                raum,
                name: name.trim().to_string(),
            },
            Dialog::RaumLoeschen { raum, .. } => Befehl::RaumLoeschen { raum },
            Dialog::Verschieben { mitglied, ziel, .. } => Befehl::Verschieben {
                mitglied,
                raum: ziel?,
            },
            Dialog::Rolle {
                mitglied, rolle, ..
            } => Befehl::RolleSetzen { mitglied, rolle },
            Dialog::Trennen {
                mitglied, grund, ..
            } => Befehl::MitgliedTrennen { mitglied, grund },
            Dialog::Sperren {
                mitglied, grund, ..
            } => Befehl::Sperren { mitglied, grund },
        };
        Some(Aktion::Befehl(befehl))
    }

    /// Die Sprechtaste wurde gedrückt oder losgelassen. Nur ein Wechsel ergibt
    /// eine Aktion — gehaltene Tasten wiederholen sich sonst.
    pub fn sprechen(&mut self, an: bool) -> Option<Aktion> {
        if !self.einstellungen.push_to_talk || self.spricht == an {
            return None;
        }
        self.spricht = an;
        Some(Aktion::Sprechtaste(an))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprechwerk_ipc::{Raum, Verbindung};

    fn verbunden() -> Zustand {
        let mut z = Zustand::neu();
        z.verbindung = Some(Verbindung::Verbunden {
            server: "a:1".into(),
            server_name: "A".into(),
            ich: MemberId::aus_roh(1),
            name: "Ada".into(),
            rolle: Role::Administrator,
            kennung: String::new(),
        });
        z
    }

    #[test]
    fn oeffnen_fordert_an_was_die_ansicht_zeigt() {
        let mut o = Oberflaeche::default();
        assert_eq!(
            o.oeffnen(Ansicht::Protokoll),
            Some(Aktion::Befehl(Befehl::Protokoll { ab: 0, anzahl: 50 }))
        );
        assert_eq!(o.oeffnen(Ansicht::Einstellungen), Some(Aktion::Geraete));
        assert_eq!(o.oeffnen(Ansicht::Einstellungen), None, "Geräte nur einmal");
        assert_eq!(o.oeffnen(Ansicht::Server), None);
        assert_eq!(o.ansicht, Ansicht::Server);
    }

    #[test]
    fn sprechtest_oeffnet_und_schliesst_das_mikrofon() {
        let mut o = Oberflaeche {
            ansicht: Ansicht::Einstellungen,
            ..Default::default()
        };
        assert_eq!(o.sprechtest(true), Some(Aktion::AudioStarten));
        assert_eq!(o.sprechtest(true), None, "läuft schon");
        assert_eq!(o.sprechtest_aufraeumen(), None, "noch in den Einstellungen");
        o.ansicht = Ansicht::Server;
        assert_eq!(
            o.sprechtest_aufraeumen(),
            Some(Aktion::AudioStoppen),
            "verlassen"
        );
        assert!(!o.sprechtest);
        // Mit Verbindung läuft die Aufnahme sowieso: kein Start, kein Stopp.
        o.zustand = verbunden();
        assert_eq!(o.sprechtest(true), None);
    }

    #[test]
    fn verbunden_fuehrt_in_die_raeume() {
        let mut o = Oberflaeche::default();
        o.melden(Meldung::Rueck {
            aktion: "verbinden",
            ergebnis: Ok(Ergebnis::Verbunden {
                server_name: "A".into(),
                audio: None,
            }),
        });
        assert_eq!(o.ansicht, Ansicht::Raeume);
        assert_eq!(o.hinweis, None);
    }

    #[test]
    fn trennen_fuehrt_zur_serverliste() {
        let mut o = Oberflaeche::default();
        o.melden(Meldung::Zustand(Box::new(verbunden())));
        o.ansicht = Ansicht::Protokoll;
        o.dialog = Dialog::RaumAnlegen { name: "x".into() };
        o.melden(Meldung::Zustand(Box::new(Zustand::neu())));
        assert_eq!(o.ansicht, Ansicht::Server);
        assert_eq!(o.dialog, Dialog::Keiner);
    }

    #[test]
    fn fehler_wird_sichtbar_und_erfolg_raeumt_ihn_weg() {
        let mut o = Oberflaeche::default();
        o.melden(Meldung::Rueck {
            aktion: "befehl",
            ergebnis: Err("kein Recht".into()),
        });
        assert_eq!(o.hinweis, Some(Hinweis::Fehler("kein Recht".into())));
        o.melden(Meldung::Rueck {
            aktion: "befehl",
            ergebnis: Ok(Ergebnis::Erledigt),
        });
        assert_eq!(o.hinweis, None);
    }

    #[test]
    fn verschluesselung_umschalten_wird_gemeldet() {
        let mut o = Oberflaeche::default();
        let mut z = verbunden();
        z.raeume = vec![Raum {
            id: RoomId::aus_roh(1),
            name: "R".into(),
            anwesend: 1,
            hier: true,
            verschluesselt: false,
        }];
        o.melden(Meldung::Zustand(Box::new(z.clone())));
        z.raeume[0].verschluesselt = true;
        o.melden(Meldung::Zustand(Box::new(z)));
        assert!(matches!(o.hinweis, Some(Hinweis::Info(_))));
    }

    #[test]
    fn senden_leert_den_entwurf() {
        let mut o = Oberflaeche {
            entwurf: "  hallo  ".into(),
            ..Default::default()
        };
        assert_eq!(
            o.senden(),
            Some(Aktion::Befehl(Befehl::Text {
                text: "hallo".into(),
            }))
        );
        assert!(o.entwurf.is_empty());
        assert_eq!(o.senden(), None);
    }

    #[test]
    fn vorschau_nur_bei_neuem_link_und_wenn_erlaubt() {
        let mut o = Oberflaeche::default();
        o.einstellungen.link_vorschau = true;
        o.entwurf = "schau https://a.de".into();
        assert!(o.entwurf_geaendert().is_some());
        o.entwurf.push_str(" toll");
        assert!(o.entwurf_geaendert().is_none(), "derselbe Link");
        o.einstellungen.link_vorschau = false;
        assert_eq!(
            o.entwurf_geaendert(),
            Some(Aktion::Befehl(Befehl::Vorschau { url: None }))
        );
    }

    #[test]
    fn favoriten_ohne_doppelte() {
        let mut o = Oberflaeche::default();
        assert!(o.favorit_merken("", " ").is_none());
        o.favorit_merken("Verein", "v.example:7443");
        o.favorit_merken("Neu", "v.example:7443");
        assert_eq!(o.einstellungen.favoriten.len(), 1);
        assert_eq!(o.einstellungen.favoriten[0].name, "Neu");
        o.favorit_entfernen("v.example:7443");
        assert!(o.einstellungen.favoriten.is_empty());
    }

    #[test]
    fn dialog_wird_zum_befehl() {
        let mut o = Oberflaeche {
            dialog: Dialog::RaumAnlegen {
                name: " Probe ".into(),
            },
            ..Default::default()
        };
        assert_eq!(
            o.dialog_bestaetigen(),
            Some(Aktion::Befehl(Befehl::RaumAnlegen {
                name: "Probe".into()
            }))
        );
        assert_eq!(o.dialog, Dialog::Keiner);
        o.dialog = Dialog::Verschieben {
            mitglied: MemberId::aus_roh(2),
            name: "B".into(),
            ziel: None,
        };
        assert_eq!(o.dialog_bestaetigen(), None, "ohne Ziel kein Verschieben");
    }

    #[test]
    fn sprechtaste_nur_bei_wechsel_und_push_to_talk() {
        let mut o = Oberflaeche::default();
        assert_eq!(o.sprechen(true), None, "ohne Push-to-Talk");
        o.einstellungen.push_to_talk = true;
        assert_eq!(o.sprechen(true), Some(Aktion::Sprechtaste(true)));
        assert_eq!(o.sprechen(true), None, "gehalten");
        assert_eq!(o.sprechen(false), Some(Aktion::Sprechtaste(false)));
    }
}

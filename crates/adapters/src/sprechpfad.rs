//! Der Sprechpfad: Vorlauf, Jitter-Puffer, Rahmenzählung.
//!
//! **Hier steckt die Logik, nicht die Soundkarte.** Alles in diesem Modul ist
//! reine Rechnerei über Rahmen fester Länge (20 ms Opus) und ohne Netz, Uhr
//! oder Gerät — deshalb ist es prüfbar, ohne dass ein Mikrofon angeschlossen
//! ist. Die Anbindung an ALSA/PipeWire und den Codec macht `crates/audio`.
//!
//! # Warum ein Vorlauf
//!
//! TeamSpeak wartet bei Push-to-Talk standardmäßig **300 ms**
//! (`delay_ptt_msecs=300` in der Clientkonfiguration), damit das erste Wort
//! nicht abgeschnitten wird. Das hört man im Gespräch. Dieselbe Sicherheit
//! gibt es ohne Wartezeit: die letzten Rahmen werden ständig mitgeschnitten und
//! beim Tastendruck **vor** dem Live-Ton verschickt. Verzögerung 0 ms,
//! abgeschnitten wird trotzdem nichts.

use std::collections::BTreeMap;

/// Länge eines Opus-Rahmens in Millisekunden (Vorgabe des Projekts).
pub const RAHMEN_MS: u32 = 20;

/// Ein Sprachrahmen: Folgenummer und Nutzlast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rahmen {
    pub folge: u16,
    pub daten: Vec<u8>,
}

/// Ringpuffer für den Vorlauf.
///
/// Es liegen immer die letzten `kapazitaet` Rahmen bereit. Beim Drücken von
/// Push-to-Talk werden sie in richtiger Reihenfolge **vor** dem ersten
/// Live-Rahmen ausgeliefert.
#[derive(Debug)]
pub struct Vorlauf {
    kapazitaet: usize,
    puffer: Vec<Rahmen>,
    naechste_folge: u16,
}

impl Vorlauf {
    /// `millisekunden` Vorlauf; auf ganze Rahmen abgerundet, mindestens einer.
    pub fn neu(millisekunden: u32) -> Self {
        let anzahl = (millisekunden / RAHMEN_MS).max(1) as usize;
        Self {
            kapazitaet: anzahl,
            puffer: Vec::with_capacity(anzahl),
            naechste_folge: 0,
        }
    }

    /// Anzahl der gehaltenen Rahmen.
    pub fn laenge(&self) -> usize {
        self.puffer.len()
    }

    /// Nimmt einen Rahmen auf (läuft auch ohne Tastendruck mit).
    pub fn aufnehmen(&mut self, daten: Vec<u8>) -> u16 {
        let folge = self.naechste_folge;
        self.naechste_folge = self.naechste_folge.wrapping_add(1);
        if self.puffer.len() == self.kapazitaet {
            self.puffer.remove(0);
        }
        self.puffer.push(Rahmen {
            folge,
            daten,
        });
        folge
    }

    /// Liefert den Vorlauf **einmalig** und leert ihn danach.
    ///
    /// Wird beim Drücken der Taste aufgerufen. Nach dem Leeren füllt sich der
    /// Puffer erst wieder, wenn die Taste losgelassen wurde — sonst würde
    /// derselbe Ton zweimal gesendet.
    pub fn abholen(&mut self) -> Vec<Rahmen> {
        std::mem::take(&mut self.puffer)
    }

    /// Verwirft den Vorlauf (Taste losgelassen, Raum gewechselt).
    pub fn leeren(&mut self) {
        self.puffer.clear();
    }
}

/// Jitter-Puffer für die Wiedergabe.
///
/// Rahmen kommen ungeordnet und manchmal gar nicht. Der Puffer hält ein
/// kleines Fenster, sortiert und gibt in Reihenfolge aus. Zu spät Gekommenes
/// wird verworfen — ein verspäteter Rahmen ist wertlos.
#[derive(Debug)]
pub struct JitterPuffer {
    /// Wie viele Rahmen vorgehalten werden, bevor ausgegeben wird.
    tiefe: usize,
    /// Rahmen, die noch nicht ausgegeben wurden.
    offen: BTreeMap<u16, Vec<u8>>,
    /// Nächste erwartete Folgenummer.
    naechste: Option<u16>,
    /// Verworfene Rahmen (zu spät oder doppelt).
    verworfen: u64,
    /// Ausgegebene Rahmen.
    ausgegeben: u64,
    /// Lücken, die übersprungen wurden.
    luecken: u64,
}

impl JitterPuffer {
    /// `tiefe` = wie viele Rahmen Vorlauf (bei 20 ms sind 3 Rahmen 60 ms).
    pub fn neu(tiefe: usize) -> Self {
        Self {
            tiefe: tiefe.max(1),
            offen: BTreeMap::new(),
            naechste: None,
            verworfen: 0,
            ausgegeben: 0,
            luecken: 0,
        }
    }

    /// Nimmt einen Rahmen auf. Gibt zurück, ob er angenommen wurde.
    pub fn aufnehmen(&mut self, folge: u16, daten: Vec<u8>) -> bool {
        if let Some(naechste) = self.naechste {
            // Alles, was älter ist als erwartet, ist zu spät.
            if folge_before(folge, naechste) {
                self.verworfen += 1;
                return false;
            }
        }
        // `BTreeMap::insert` **überschreibt** einen vorhandenen Wert. Ein
        // doppelt ankommender Rahmen (Wiederholung im Netz) würde also die
        // guten Daten durch die Kopie ersetzen — deshalb vorher prüfen.
        if self.offen.contains_key(&folge) {
            self.verworfen += 1;
            return false;
        }
        self.offen.insert(folge, daten);
        // Die erwartete Folgenummer ergibt sich aus dem **niedrigsten**
        // anliegenden Rahmen, nicht aus dem ersten ankommenden: sonst gilt ein
        // früher angekommener höherer Rahmen als „der Anfang" und alles
        // Niedrigere wäre angeblich zu spät.
        true
    }

    /// Gibt den nächsten Rahmen aus, sobald genug vorgehalten ist.
    ///
    /// `None` heißt: noch warten (oder nichts mehr da).
    pub fn ausgeben(&mut self) -> Option<Rahmen> {
        if self.naechste.is_none() {
            self.naechste = self.offen.keys().next().copied();
        }
        let naechste = self.naechste?;
        // Erst ausgeben, wenn der Puffer tief genug ist oder eine Lücke
        // erkennbar ist (dann nicht ewig warten).
        let tief_genug = self.offen.len() >= self.tiefe;
        let luecke = !self.offen.contains_key(&naechste) && self.offen.keys().next() > Some(&naechste);
        if !tief_genug && !luecke {
            return None;
        }
        match self.offen.remove(&naechste) {
            Some(daten) => {
                self.naechste = Some(naechste.wrapping_add(1));
                self.ausgegeben += 1;
                Some(Rahmen {
                    folge: naechste,
                    daten,
                })
            }
            None => {
                // Rahmen fehlt: überspringen, weiterzählen.
                self.luecken += 1;
                self.naechste = Some(naechste.wrapping_add(1));
                None
            }
        }
    }

    /// Gibt alles aus, was noch wartet — in Reihenfolge.
    ///
    /// Wird aufgerufen, wenn der Sprecher aufhört: die letzten Rahmen eines
    /// Wortbeitrags sollen nicht im Puffer verfallen, nur weil kein weiterer
    /// mehr kommt.
    pub fn leeren(&mut self) -> Vec<Rahmen> {
        let mut aus = Vec::new();
        while let Some(r) = self.ausgeben_sofort() {
            aus.push(r);
        }
        self.offen.clear();
        self.naechste = None;
        aus
    }

    /// Nimmt den nächsten Rahmen ohne Rücksicht auf die Fülltiefe.
    fn ausgeben_sofort(&mut self) -> Option<Rahmen> {
        if self.naechste.is_none() {
            self.naechste = self.offen.keys().next().copied();
        }
        let naechste = self.naechste?;
        match self.offen.remove(&naechste) {
            Some(daten) => {
                self.naechste = Some(naechste.wrapping_add(1));
                self.ausgegeben += 1;
                Some(Rahmen {
                    folge: naechste,
                    daten,
                })
            }
            None => {
                self.naechste = self.offen.keys().next().copied();
                None
            }
        }
    }

    /// Zähler für Diagnose.
    pub fn zaehler(&self) -> (u64, u64, u64) {
        (self.ausgegeben, self.verworfen, self.luecken)
    }

    /// Wie viele Rahmen warten.
    pub fn offen(&self) -> usize {
        self.offen.len()
    }
}

/// Ist `a` vor `b` in der Folgenummernordnung (mit Überlauf bei u16)?
fn folge_before(a: u16, b: u16) -> bool {
    // Halber Wertebereich als Grenze: 0 nach 65535 ist „danach", nicht „davor".
    let diff = b.wrapping_sub(a);
    diff != 0 && diff < 32768
}

/// Zustand der Sprechtaste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sprechtaste {
    /// Taste nicht gedrückt, Mikrofon zu.
    Los,
    /// Taste gedrückt, Vorlauf ging raus, Live-Ton läuft.
    Gedrueckt,
}

/// Steuert, ob gesendet wird: Push-to-Talk oder offenes Mikrofon mit Schwelle.
#[derive(Debug)]
pub struct Sprechsteuerung {
    push_to_talk: bool,
    taste: Sprechtaste,
    /// Sprechschwelle 0–100 für das offene Mikrofon.
    schwelle: u8,
    /// Letzter gemessener Pegel 0–100 (vom Gerät gemeldet).
    pegel: u8,
}

impl Sprechsteuerung {
    pub fn neu(push_to_talk: bool, schwelle: u8) -> Self {
        Self {
            push_to_talk,
            taste: Sprechtaste::Los,
            schwelle: schwelle.min(100),
            pegel: 0,
        }
    }

    /// Meldet den aktuellen Pegel (0–100).
    pub fn pegel_melden(&mut self, pegel: u8) {
        self.pegel = pegel.min(100);
    }

    /// Tastendruck.
    pub fn taste_druecken(&mut self) {
        self.taste = Sprechtaste::Gedrueckt;
    }

    /// Taste losgelassen.
    pub fn taste_loesen(&mut self) {
        self.taste = Sprechtaste::Los;
    }

    /// Soll der aktuelle Rahmen gesendet werden?
    pub fn sendet(&self) -> bool {
        if self.push_to_talk {
            self.taste == Sprechtaste::Gedrueckt
        } else {
            self.pegel >= self.schwelle && self.schwelle > 0
        }
    }

    pub fn taste(&self) -> Sprechtaste {
        self.taste
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorlauf_haelt_die_letzten_rahmen() {
        let mut v = Vorlauf::neu(60); // 3 Rahmen
        assert_eq!(v.laenge(), 0);
        for i in 0..5u8 {
            v.aufnehmen(vec![i]);
        }
        assert_eq!(v.laenge(), 3, "nur die letzten drei bleiben");
        let raus = v.abholen();
        let inhalte: Vec<u8> = raus.iter().map(|r| r.daten[0]).collect();
        assert_eq!(inhalte, vec![2, 3, 4], "ältester zuerst");
        assert_eq!(raus[2].folge, 4);
        assert_eq!(v.laenge(), 0, "nach dem Abholen ist er leer");
    }

    #[test]
    fn vorlauf_wird_nicht_zweimal_gesendet() {
        let mut v = Vorlauf::neu(60);
        v.aufnehmen(vec![1]);
        v.aufnehmen(vec![2]);
        assert_eq!(v.abholen().len(), 2);
        assert!(v.abholen().is_empty(), "zweiter Aufruf liefert nichts");
    }

    #[test]
    fn vorlauf_null_millisekunden_haelt_einen_rahmen() {
        let mut v = Vorlauf::neu(0);
        v.aufnehmen(vec![1]);
        v.aufnehmen(vec![2]);
        assert_eq!(v.laenge(), 1);
    }

    #[test]
    fn jitter_puffer_sortiert() {
        let mut j = JitterPuffer::neu(1);
        assert!(j.aufnehmen(3, vec![3]));
        assert!(j.aufnehmen(1, vec![1]));
        assert!(j.aufnehmen(2, vec![2]));
        let reihenfolge: Vec<u8> = std::iter::from_fn(|| j.ausgeben())
            .map(|r| r.daten[0])
            .collect();
        assert_eq!(reihenfolge, vec![1, 2, 3]);
        assert_eq!(j.zaehler(), (3, 0, 0));
    }

    #[test]
    fn jitter_puffer_verwirft_zu_spaetes() {
        let mut j = JitterPuffer::neu(1);
        j.aufnehmen(5, vec![5]);
        let erster = j.ausgeben().expect("erster Rahmen");
        assert_eq!(erster.folge, 5);
        // Rahmen 4 kommt nach der Ausgabe von 5: zu spät.
        assert!(!j.aufnehmen(4, vec![4]));
        assert_eq!(j.zaehler(), (1, 1, 0));
    }

    #[test]
    fn jitter_puffer_ueberspringt_luecken() {
        let mut j = JitterPuffer::neu(1);
        j.aufnehmen(1, vec![1]);
        j.aufnehmen(3, vec![3]); // 2 fehlt
        let mut raus = Vec::new();
        for _ in 0..3 {
            if let Some(r) = j.ausgeben() {
                raus.push(r.folge);
            }
        }
        raus.extend(j.leeren().into_iter().map(|r| r.folge));
        assert_eq!(raus, vec![1, 3], "die Lücke wird übersprungen, nicht blockiert");
        let (ausgegeben, _, luecken) = j.zaehler();
        assert_eq!(ausgegeben, 2);
        assert_eq!(luecken, 1);
    }

    #[test]
    fn jitter_puffer_gibt_den_rest_beim_aufhoeren_aus() {
        // Ein Sprecher hört auf: der letzte Rahmen darf nicht verfallen, nur
        // weil die Fülltiefe nicht mehr erreicht wird.
        let mut j = JitterPuffer::neu(3);
        j.aufnehmen(1, vec![1]);
        j.aufnehmen(2, vec![2]);
        assert!(j.ausgeben().is_none(), "noch nicht tief genug");
        let rest = j.leeren();
        let folgen: Vec<u16> = rest.iter().map(|r| r.folge).collect();
        assert_eq!(folgen, vec![1, 2]);
        assert!(j.leeren().is_empty());
    }

    #[test]
    fn jitter_puffer_verwirft_doppelte() {
        let mut j = JitterPuffer::neu(1);
        assert!(j.aufnehmen(1, vec![1]));
        assert!(!j.aufnehmen(1, vec![9]), "derselbe Rahmen zählt nicht zweimal");
        let r = j.ausgeben().expect("erster Rahmen");
        assert_eq!(r.daten, vec![1], "der erste Inhalt gewinnt");
        assert_eq!(j.zaehler(), (1, 1, 0));
    }

    #[test]
    fn folgenummern_laufen_um() {
        assert!(folge_before(65535, 0), "0 kommt nach 65535");
        assert!(!folge_before(0, 65535));
        assert!(folge_before(1, 2));
        assert!(!folge_before(2, 1));
    }

    #[test]
    fn push_to_talk_sendet_nur_bei_gedrueckter_taste() {
        let mut s = Sprechsteuerung::neu(true, 0);
        s.pegel_melden(100);
        assert!(!s.sendet(), "offenes Mikrofon zählt bei PTT nicht");
        s.taste_druecken();
        assert!(s.sendet());
        s.taste_loesen();
        assert!(!s.sendet());
    }

    #[test]
    fn offenes_mikrofon_sendet_ueber_der_schwelle() {
        let mut s = Sprechsteuerung::neu(false, 30);
        s.pegel_melden(10);
        assert!(!s.sendet());
        s.pegel_melden(30);
        assert!(s.sendet(), "Schwelle erreicht genügt");
        s.pegel_melden(90);
        assert!(s.sendet());
    }

    #[test]
    fn schwelle_null_bleibt_stumm() {
        // Ohne Schwelle würde ein offenes Mikrofon dauerhaft senden; das ist
        // nie gewollt.
        let mut s = Sprechsteuerung::neu(false, 0);
        s.pegel_melden(100);
        assert!(!s.sendet());
    }
}

//! Verlust im Netz: Paritätsrahmen und ein prüfbarer Verlusttest.
//!
//! # Warum nicht nur Opus-FEC
//!
//! Opus trägt in jedem Rahmen eine **grobe Fassung des vorherigen** mit
//! (in-band FEC). Das hilft bei **Einzelverlusten** und klingt bei 5 % Verlust
//! schon deutlich besser als Stille. Es hat aber zwei Grenzen:
//!
//! * Es hilft nur dem **vorherigen** Rahmen — bei zwei Verlusten hintereinander
//!   ist der erste weg.
//! * Die grobe Fassung ist hörbar schlechter (weniger Bänder, mehr Artefakte).
//!
//! Deshalb zusätzlich **Paritätsrahmen**: über je vier Rahmen geht ein fünfter,
//! der die XOR-Summe trägt. Fehlt genau einer der vier, ist er daraus exakt
//! wiederherstellbar (bitgenau, nicht „grob"). Fehlen zwei, greift wieder nur
//! FEC. Aufschlag: **25 %** der Sprachlast — bei 32 kbit/s sind das 8 kbit/s.
//!
//! # Warum das plattformunabhängig ist
//!
//! Verlust zu prüfen hieß früher `tc netem` — ein Linux-Werkzeug, das es auf
//! Windows und macOS nicht gibt. Hier wird der Verlust **im Empfänger**
//! simuliert: derselbe Code, dieselben Tests, auf jeder Plattform. Was im Netz
//! passiert (Reihenfolge, Doppelung, Verlust) ist genau das, was dieses Modul
//! abbildet.

use sprechwerk_protocol::{Sprachkopf, SPRACHKOPF_VERSION};
use std::collections::BTreeMap;

/// Wie viele Datenrahmen eine Paritätsgruppe umfasst.
pub const GRUPPE: u16 = 4;

/// Ein Paritätsrahmen: XOR über `GRUPPE` Datenrahmen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paritaet {
    /// Folgenummer des **ersten** Rahmens der Gruppe.
    pub ab: u16,
    /// Wie viele Rahmen die Gruppe umfasst.
    pub anzahl: u16,
}

impl Paritaet {
    /// Bildet die XOR-Summe über die (gleich langen) Rahmen.
    ///
    /// Unterschiedliche Längen kommen vor (Opus liefert je nach Inhalt 20–120
    /// Byte); dann wird auf die größte Länge mit Nullen aufgefüllt. Die Länge
    /// jedes Rahmens steht im Kopf des wiederhergestellten Rahmens — hier wird
    /// nur die Nutzlast gebraucht.
    pub fn summe(rahmen: &[Vec<u8>]) -> Vec<u8> {
        let laenge = rahmen.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut summe = vec![0u8; laenge];
        for r in rahmen {
            for (i, byte) in r.iter().enumerate() {
                summe[i] ^= byte;
            }
        }
        summe
    }

    /// Stellt den fehlenden Rahmen wieder her: Summe XOR alle vorhandenen.
    pub fn wiederherstellen(summe: &[u8], vorhanden: &[Vec<u8>]) -> Vec<u8> {
        let mut fehlend = summe.to_vec();
        for r in vorhanden {
            for (i, byte) in r.iter().enumerate() {
                fehlend[i] ^= byte;
            }
        }
        fehlend
    }
}

/// Ein Paritäts-Datagramm bauen (Kopf + Kennzeichnung + Längen + Summe).
///
/// Die Kennzeichnung steht im **Codec-Feld** des Sprachkopfs: `0` = Opus,
/// `2` = Parität. So braucht der Weg keine neue Rahmenart, und ein Client ohne
/// Verlustausgleich verwirft Parität einfach (unbekannter Codec).
///
/// **Die Längen gehören mit hinein.** Opus liefert je nach Inhalt 20–120 Byte;
/// die XOR-Summe hat die Länge des längsten Rahmens. Ohne die Einzellängen
/// käme ein wiederhergestellter Rahmen zu lang (mit Nullen am Ende) heraus —
/// und ein Opus-Rahmen mit angehängten Nullen ist Müll.
pub fn paritaets_datagramm(ab: u16, laengen: &[u16], summe: &[u8]) -> Vec<u8> {
    let mut kopf = Sprachkopf::neu(ab);
    kopf.codec = CODEC_PARITAET;
    // `von` setzt der Server beim Weiterleiten (wie bei Sprachrahmen auch) —
    // hier bleibt es 0 und wird erst auf der Leitung ergänzt.
    let mut nutzlast = Vec::with_capacity(2 + laengen.len() * 2 + summe.len());
    nutzlast.extend_from_slice(&(laengen.len() as u16).to_be_bytes());
    for laenge in laengen {
        nutzlast.extend_from_slice(&laenge.to_be_bytes());
    }
    nutzlast.extend_from_slice(summe);
    kopf.schreiben(&nutzlast)
}

/// Codec-Kennung für einen Paritätsrahmen.
pub const CODEC_PARITAET: u8 = 2;

/// Was ein empfangenes Datagramm ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Empfangen {
    /// Ein normaler Opus-Rahmen.
    Sprache { folge: u16, daten: Vec<u8> },
    /// Ein Paritätsrahmen samt Längen der vier Rahmen.
    Paritaet {
        ab: u16,
        laengen: Vec<u16>,
        summe: Vec<u8>,
    },
    /// Unbrauchbar (falscher Kopf, unbekannte Version).
    Verworfen,
}

/// Liest ein Datagramm und entscheidet, was es ist.
pub fn lese(datagramm: &[u8]) -> Empfangen {
    let Some((kopf, nutzlast)) = Sprachkopf::lesen(datagramm) else {
        return Empfangen::Verworfen;
    };
    if kopf.version != SPRACHKOPF_VERSION {
        return Empfangen::Verworfen;
    }
    match kopf.codec {
        0 => Empfangen::Sprache {
            folge: kopf.folge,
            daten: nutzlast.to_vec(),
        },
        CODEC_PARITAET => {
            if nutzlast.len() < 2 {
                return Empfangen::Verworfen;
            }
            let anzahl = u16::from_be_bytes([nutzlast[0], nutzlast[1]]) as usize;
            let kopf_laenge = 2 + anzahl * 2;
            if anzahl == 0 || anzahl > 16 || nutzlast.len() < kopf_laenge {
                return Empfangen::Verworfen;
            }
            let laengen: Vec<u16> = (0..anzahl)
                .map(|i| {
                    let p = 2 + i * 2;
                    u16::from_be_bytes([nutzlast[p], nutzlast[p + 1]])
                })
                .collect();
            Empfangen::Paritaet {
                ab: kopf.folge,
                laengen,
                summe: nutzlast[kopf_laenge..].to_vec(),
            }
        }
        // Unbekannter Codec: verwerfen, nicht abstürzen.
        _ => Empfangen::Verworfen,
    }
}

/// Sammelt Rahmen und füllt Lücken aus Paritätsgruppen.
///
/// **Gruppenweise Ausgabe.** Ein Rahmen wird erst weitergegeben, wenn seine
/// Gruppe entschieden ist: entweder weil ihre Parität da ist (dann ist eine
/// Lücke füllbar) oder weil eine deutlich spätere Gruppe zeigt, dass die
/// Parität verloren ging. Der Preis ist ein Aufschub von **einer Gruppe**
/// (80 ms bei vier Rahmen) — er ersetzt den Jitter-Puffer, nicht zusätzlich zu
/// ihm.
#[derive(Debug)]
pub struct Wiederhersteller {
    /// Empfangene Rahmen, die auf ihre Gruppe warten.
    offen: BTreeMap<u16, Vec<u8>>,
    /// Gruppen, die entschieden sind (ausgebbar).
    entschieden: std::collections::BTreeSet<u16>,
    /// Höchste gesehene Folgenummer (zum Erkennen verlorener Paritäten).
    hoechste: Option<u16>,
    /// Bis hierher wurde alles ausgegeben (gegen Doppelte und Altes).
    ausgegeben_bis: Option<u16>,
    wiederhergestellt: u64,
    verloren: u64,
}

impl Default for Wiederhersteller {
    fn default() -> Self {
        Self::neu()
    }
}

/// Zu welcher Gruppe gehört eine Folgenummer?
fn gruppe_von(folge: u16) -> u16 {
    folge / GRUPPE * GRUPPE
}

impl Wiederhersteller {
    pub fn neu() -> Self {
        Self {
            offen: BTreeMap::new(),
            entschieden: std::collections::BTreeSet::new(),
            hoechste: None,
            ausgegeben_bis: None,
            wiederhergestellt: 0,
            verloren: 0,
        }
    }

    /// Ist dieser Rahmen schon vorbei (Doppelter oder zu spät)?
    ///
    /// Folgenummern laufen um; der Vergleich ist deshalb ein Abstandsvergleich
    /// und kein `<`.
    pub fn zu_alt(&self, folge: u16) -> bool {
        match self.ausgegeben_bis {
            Some(bis) => folge.wrapping_sub(bis) as i16 <= 0,
            None => false,
        }
    }

    /// Nimmt ein Datagramm auf. Liefert die Rahmen, die jetzt **in Reihenfolge**
    /// weitergegeben werden können.
    pub fn nimm(&mut self, datagramm: &[u8]) -> Vec<(u16, Vec<u8>)> {
        match lese(datagramm) {
            Empfangen::Sprache { folge, daten } => {
                if self.zu_alt(folge) {
                    // Doppelter oder verspäteter Rahmen: verwerfen.
                    return Vec::new();
                }
                self.hoechste = Some(match self.hoechste {
                    Some(h) if h >= folge => h,
                    _ => folge,
                });
                self.offen.insert(folge, daten);
            }
            Empfangen::Paritaet { ab, laengen, summe } => {
                let anzahl = laengen.len() as u16;
                let bis = ab.wrapping_add(anzahl);
                let fehlend: Vec<u16> = (ab..bis).filter(|f| !self.offen.contains_key(f)).collect();
                if fehlend.len() == 1 {
                    let luecke = (fehlend[0] - ab) as usize;
                    let vorhanden: Vec<Vec<u8>> = (ab..bis)
                        .filter(|f| self.offen.contains_key(f))
                        .filter_map(|f| self.offen.get(&f).cloned())
                        .collect();
                    let mut wieder = Paritaet::wiederherstellen(&summe, &vorhanden);
                    // Auf die echte Länge zurückschneiden: der Sender hat mit
                    // Nullen aufgefüllt, und angehängte Nullen wären im
                    // Opus-Rahmen Müll.
                    let laenge = *laengen.get(luecke).unwrap_or(&(wieder.len() as u16)) as usize;
                    wieder.truncate(laenge);
                    self.offen.insert(fehlend[0], wieder);
                    self.wiederhergestellt += 1;
                }
                self.entschieden.insert(ab);
            }
            Empfangen::Verworfen => return Vec::new(),
        }

        // Ohne Parität muss eine Gruppe trotzdem irgendwann entschieden
        // werden: wenn zwei Gruppen weiter bereits Rahmen liegen, ist die
        // Parität dieser Gruppe verloren. (Dann bleibt es beim Aufschub von
        // zwei Gruppen — der Rückfallweg ohne Verlustausgleich.)
        if let Some(h) = self.hoechste {
            let unterste = self.offen.keys().next().copied();
            if let Some(u) = unterste {
                let g = gruppe_von(u);
                if h >= g.saturating_add(2 * GRUPPE) {
                    self.entschieden.insert(g);
                }
            }
        }

        self.abgeben()
    }

    /// Zähler: (wiederhergestellt, endgültig verloren).
    pub fn zaehler(&self) -> (u64, u64) {
        (self.wiederhergestellt, self.verloren)
    }

    /// Wie viele Rahmen warten auf ihre Gruppe? (Für Prüfungen.)
    pub fn wartend(&self) -> usize {
        self.offen.len()
    }

    /// Gibt entschiedene Gruppen in Reihenfolge aus.
    fn abgeben(&mut self) -> Vec<(u16, Vec<u8>)> {
        let mut raus = Vec::new();
        while let Some(&unterste) = self.offen.keys().next() {
            let g = gruppe_von(unterste);
            if !self.entschieden.contains(&g) {
                break;
            }
            for folge in g..g.saturating_add(GRUPPE) {
                match self.offen.remove(&folge) {
                    Some(daten) => {
                        raus.push((folge, daten));
                        self.ausgegeben_bis = Some(folge);
                    }
                    None => self.verloren += 1,
                }
            }
            self.entschieden.remove(&g);
        }
        raus
    }

    /// Räumt alles auf (Raumwechsel, neue Verbindung).
    pub fn leeren(&mut self) {
        self.offen.clear();
        self.entschieden.clear();
        self.hoechste = None;
        self.ausgegeben_bis = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rahmen(folge: u16, inhalt: &[u8]) -> Vec<u8> {
        Sprachkopf::neu(folge).schreiben(inhalt)
    }

    /// Baut eine Gruppe mit Parität, lässt einen Rahmen weg.
    fn gruppe_mit_verlust(verlust: u16) -> (Vec<Vec<u8>>, Vec<u8>) {
        let daten: Vec<Vec<u8>> = (0..GRUPPE)
            .map(|i| vec![(i as u8) + 1; 40 + i as usize])
            .collect();
        let summe = Paritaet::summe(&daten);
        let laengen: Vec<u16> = daten.iter().map(|d| d.len() as u16).collect();
        let mut datagramme = Vec::new();
        for (i, d) in daten.iter().enumerate() {
            if i as u16 != verlust {
                datagramme.push(rahmen(i as u16, d));
            }
        }
        let mut paritaet = paritaets_datagramm(0, &laengen, &summe);
        paritaet[4..6].copy_from_slice(&1u16.to_be_bytes());
        (datagramme, paritaet)
    }

    #[test]
    fn paritaet_stellt_genau_einen_rahmen_wieder_her() {
        let (datagramme, paritaet) = gruppe_mit_verlust(2);
        let mut w = Wiederhersteller::neu();
        let mut alle = Vec::new();
        for d in &datagramme {
            alle.extend(w.nimm(d));
        }
        assert!(
            alle.is_empty(),
            "ausgegeben wird erst, wenn die Gruppe entschieden ist"
        );
        alle.extend(w.nimm(&paritaet));
        assert_eq!(alle.len(), 4, "Parität füllt die Lücke");
        assert_eq!(alle[2].0, 2, "auch die Folgenummer stimmt");
        assert_eq!(
            alle[2].1,
            vec![3u8; 42],
            "Inhalt bitgenau wiederhergestellt"
        );
        assert_eq!(w.zaehler(), (1, 0));
    }

    #[test]
    fn zwei_verluste_sind_nicht_wiederherstellbar() {
        let daten: Vec<Vec<u8>> = (0..GRUPPE).map(|i| vec![i as u8 + 1; 40]).collect();
        let summe = Paritaet::summe(&daten);
        let mut w = Wiederhersteller::neu();
        w.nimm(&rahmen(0, &daten[0]));
        // 1 und 2 fehlen
        w.nimm(&rahmen(3, &daten[3]));
        let laengen = vec![40u16; GRUPPE as usize];
        let raus = w.nimm(&paritaets_datagramm(0, &laengen, &summe));
        assert_eq!(raus.len(), 2, "nur die vorhandenen kommen durch");
        assert_eq!(
            w.zaehler(),
            (0, 2),
            "nichts falsch wiederhergestellt, zwei gelten als verloren"
        );
    }

    #[test]
    fn unbekannter_codec_wird_verworfen() {
        let mut kopf = Sprachkopf::neu(1);
        kopf.codec = 99;
        let d = kopf.schreiben(&[1, 2, 3]);
        assert_eq!(lese(&d), Empfangen::Verworfen);
        let mut w = Wiederhersteller::neu();
        assert!(w.nimm(&d).is_empty());
    }

    #[test]
    fn reihenfolge_bleibt_erhalten() {
        let mut w = Wiederhersteller::neu();
        // Verkehrt herum einspeisen, Parität schließt die Gruppe ab.
        w.nimm(&rahmen(2, &[3; 10]));
        w.nimm(&rahmen(0, &[1; 10]));
        w.nimm(&rahmen(1, &[2; 10]));
        w.nimm(&rahmen(3, &[4; 10]));
        let summe = Paritaet::summe(&[vec![1; 10], vec![2; 10], vec![3; 10], vec![4; 10]]);
        let laengen = vec![10u16; GRUPPE as usize];
        let raus: Vec<u16> = w
            .nimm(&paritaets_datagramm(0, &laengen, &summe))
            .iter()
            .map(|(f, _)| *f)
            .collect();
        assert_eq!(raus, vec![0, 1, 2, 3], "in Reihenfolge, nicht in Ankunft");
    }

    /// Die eigentliche Zusage: **5 % Verlust hört man nicht als Aussetzer.**
    ///
    /// Der Verlust wird hier im Code erzeugt (nicht mit `tc`): derselbe Test
    /// läuft damit auf Linux, Windows und macOS.
    #[test]
    fn fuenf_prozent_verlust_werden_vollstaendig_ausgeglichen() {
        // 100 Gruppen zu 4 Rahmen; jeder 20. Rahmen geht verloren (5 %).
        let mut w = Wiederhersteller::neu();
        let mut angekommen = 0;
        let mut gesendet = 0;
        for gruppe in 0..100u16 {
            let ab = gruppe * GRUPPE;
            let daten: Vec<Vec<u8>> = (0..GRUPPE)
                .map(|i| vec![(ab as u8).wrapping_add(i as u8); 40])
                .collect();
            let summe = Paritaet::summe(&daten);
            for i in 0..GRUPPE {
                let folge = ab + i;
                gesendet += 1;
                // Jeder 20. Rahmen geht verloren — und zwar nie zwei in einer
                // Gruppe, so wie es bei unabhängigem Verlust fast immer ist.
                if folge % 20 == 7 {
                    continue;
                }
                angekommen += w.nimm(&rahmen(folge, &daten[i as usize])).len();
            }
            let laengen = vec![40u16; GRUPPE as usize];
            angekommen += w.nimm(&paritaets_datagramm(ab, &laengen, &summe)).len();
        }
        let (wiederhergestellt, verloren) = w.zaehler();
        assert_eq!(verloren, 0);
        assert_eq!(
            wiederhergestellt, 20,
            "20 Verluste müssen alle ersetzt werden"
        );
        assert_eq!(
            angekommen as u16, gesendet,
            "am Ende müssen alle {gesendet} Rahmen angekommen sein"
        );
    }
}

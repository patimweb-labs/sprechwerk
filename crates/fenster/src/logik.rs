// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann

//! Reine Funktionen für die Oberfläche (AGENTS.md §2.4): rechnen, filtern,
//! formatieren — ohne egui, mit Tests. Die Ansichten rufen nur hier auf.

use crate::texte::{t, tf, Sprache};
use sprechwerk_ipc::{Mitglied, Nachricht, NachrichtInhalt, Zustand};

/// Emoji zum Einfügen und Reagieren, nach Gruppen (Schlüssel in
/// `texte.json`). Eine feste Auswahl — dieselbe Schrift (Noto Emoji) auf
/// jedem System, kein Zeichen, das irgendwo als Kästchen erscheint.
pub const EMOJI: [(&str, &[&str]); 4] = [
    (
        "emoji.smileys",
        &[
            "😀", "😃", "😄", "😁", "😆", "😅", "😂", "😉", "😊", "😇", "😍", "😘", "😋", "😛",
            "😜", "😐", "😑", "😶", "😏", "😬", "😌", "😴", "😷", "😎", "😕", "😟", "😮", "😲",
            "😳", "😢", "😭", "😱", "😤", "😠", "😡",
        ],
    ),
    (
        "emoji.gesten",
        &[
            "👍", "👎", "👌", "✌️", "👏", "🙌", "👐", "🙏", "💪", "👋", "👀",
        ],
    ),
    (
        "emoji.herzen",
        &[
            "❤️", "💛", "💚", "💙", "💜", "💔", "💯", "🔥", "✨", "⭐", "🎉",
        ],
    ),
    (
        "emoji.dinge",
        &[
            "☕", "🍺", "🍕", "🎂", "🎮", "🎧", "🎤", "⚽", "🏆", "♟️", "🚀", "💡", "📌", "✅",
            "❌", "⚠️", "❓", "❗",
        ],
    ),
];

/// Zum Anzeigen: ohne das unsichtbare Variantenzeichen U+FE0F („als Emoji
/// zeigen"), das die eingebaute Schrift nicht kennt und sonst als Kästchen
/// erschiene (❤️, ✌️ …). Gesendet wird weiter das Original.
pub fn darstellbar(text: &str) -> String {
    text.replace('\u{FE0F}', "")
}

/// Fügt `neu` an der Zeichenstelle `stelle` ein (Zeichen, nicht Bytes) und
/// gibt die Stelle dahinter zurück — dort steht der Cursor danach.
pub fn einfuegen(text: &mut String, stelle: usize, neu: &str) -> usize {
    let stelle = stelle.min(text.chars().count());
    let byte = text
        .char_indices()
        .nth(stelle)
        .map_or(text.len(), |(b, _)| b);
    text.insert_str(byte, neu);
    stelle + neu.chars().count()
}

/// Die sechs Reaktionen, die das Fenster anbietet.
pub const REAKTIONEN: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];

/// Kennung in Vierergruppen, gekürzt: `ab12:cd34:…`.
pub fn kennung_gruppiert(kennung: &str, zeichen: usize) -> String {
    let roh: String = kennung.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if roh.is_empty() {
        return String::new();
    }
    let teil: String = roh.chars().take(zeichen).collect();
    let mut aus = gruppen(&teil);
    if roh.len() > zeichen {
        aus.push('…');
    }
    aus
}

/// Fingerabdruck vollständig in Vierergruppen, klein geschrieben.
pub fn fingerabdruck_gruppiert(fingerabdruck: &str) -> String {
    let roh: String = fingerabdruck
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if roh.is_empty() {
        return String::new();
    }
    gruppen(&roh)
}

fn gruppen(roh: &str) -> String {
    roh.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(":")
}

/// „vor 3 Minuten" — `None`, wenn noch nie.
pub fn zuletzt_text(sprache: Sprache, ms: i64, jetzt: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    let sekunden = ((jetzt - ms) as f64 / 1000.0).round().max(0.0);
    if sekunden < 90.0 {
        return Some(t(sprache, "time.now"));
    }
    let minuten = (sekunden / 60.0).round();
    if minuten < 60.0 {
        return Some(tf(sprache, "time.minutes", &[&minuten]));
    }
    let stunden = (minuten / 60.0).round();
    if stunden < 24.0 {
        return Some(tf(sprache, "time.hours", &[&stunden]));
    }
    let tage = (stunden / 24.0).round();
    if tage < 30.0 {
        return Some(tf(sprache, "time.days", &[&tage]));
    }
    let monate = (tage / 30.0).round();
    if monate < 12.0 {
        return Some(tf(sprache, "time.months", &[&monate]));
    }
    Some(tf(sprache, "time.years", &[&(monate / 12.0).round()]))
}

/// „8 ms" oder „1,2 s".
pub fn latenz_text(ms: Option<u32>) -> Option<String> {
    let ms = ms?;
    if ms < 1000 {
        Some(format!("{ms} ms"))
    } else {
        Some(format!("{:.1} s", ms as f64 / 1000.0).replace('.', ","))
    }
}

/// Uhrzeit `HH:MM` in Ortszeit ist ohne Zeitzonen-Datenbank nicht zu haben;
/// das Fenster zeigt UTC-Versatz-freie Zeit über das System.
pub fn uhrzeit(ms: i64, versatz_sekunden: i64) -> String {
    let s = ms.div_euclid(1000) + versatz_sekunden;
    let tag = s.rem_euclid(86_400);
    format!("{:02}:{:02}", tag / 3600, (tag % 3600) / 60)
}

/// Mindestbreite mit offenem Chat: Baum und Chat nebeneinander.
pub const BREITE_MIT_CHAT: f32 = sprechwerk_oberflaeche::thema::SPACE_8 * 9.0;
/// Mindestbreite kompakt (Chat eingeklappt): nur der Kanalbaum.
pub const BREITE_KOMPAKT_MIN: f32 = sprechwerk_oberflaeche::thema::SPACE_8 * 5.0;
/// Breite, auf die das Fenster beim Einklappen schrumpft.
pub const BREITE_KOMPAKT: f32 = sprechwerk_oberflaeche::thema::SPACE_8 * 5.5;
/// Mindesthöhe in beiden Fällen.
pub const HOEHE_MIN: f32 = sprechwerk_oberflaeche::thema::SPACE_8 * 6.0;

/// Wie schmal das Fenster werden darf.
pub fn mindestbreite(chat_eingeklappt: bool) -> f32 {
    if chat_eingeklappt {
        BREITE_KOMPAKT_MIN
    } else {
        BREITE_MIT_CHAT
    }
}

/// Neue Fensterbreite nach dem Ein- oder Ausklappen des Chats: jeweils die
/// Breite, die das Fenster in diesem Zustand zuletzt hatte — beim ersten
/// Mal eine sinnvolle Vorgabe, nie schmaler als nötig.
pub fn breite_nach_wechsel(eingeklappt: bool, mit_chat: Option<f32>, kompakt: Option<f32>) -> f32 {
    if eingeklappt {
        kompakt.unwrap_or(BREITE_KOMPAKT).max(BREITE_KOMPAKT_MIN)
    } else {
        mit_chat
            .unwrap_or(BREITE_MIT_CHAT * 1.5)
            .max(BREITE_MIT_CHAT)
    }
}

/// Abstand, ab dem eine Nachricht derselben Person einen neuen Block beginnt.
const BLOCKPAUSE_MS: i64 = 5 * 60_000;

/// Beginnt `n` einen neuen Block (Name und Uhrzeit darüber)? Ja, wenn jemand
/// anderes schreibt oder seit der letzten Nachricht Zeit vergangen ist — so
/// steht der Name nicht über jeder Zeile.
pub fn neuer_block(vorher: Option<&Nachricht>, n: &Nachricht) -> bool {
    match vorher {
        None => true,
        Some(v) => v.verfasser != n.verfasser || n.zeit - v.zeit > BLOCKPAUSE_MS,
    }
}

/// Was eine Person gerade tut — ein Wert, ein Icon im Baum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personenstatus {
    Spricht,
    Still,
    Stumm,
    Taub,
}

/// Spreche ich gerade? Bei mir selbst weiß es das Fenster schneller als der
/// Server: offenes Mikrofon über der Schwelle, oder Sprechtaste gedrückt und
/// Ton da. So flackert das eigene Quadrat mit der Stimme.
pub fn ich_spreche(
    audio: &sprechwerk_client::AudioStand,
    schwelle: u8,
    push_to_talk: bool,
    taste: bool,
) -> bool {
    if !audio.laeuft || audio.stumm || audio.pegel == 0 {
        return false;
    }
    if push_to_talk {
        taste
    } else {
        audio.pegel >= schwelle
    }
}

/// Taub geht vor stumm geht vor sprechen: wer nichts hört, soll es sehen.
/// `spricht_selbst` ersetzt für die eigene Zeile die Meldung des Servers.
pub fn personenstatus(m: &Mitglied, spricht_selbst: Option<bool>) -> Personenstatus {
    if m.taub {
        Personenstatus::Taub
    } else if m.stumm {
        Personenstatus::Stumm
    } else if spricht_selbst.unwrap_or(m.spricht) {
        Personenstatus::Spricht
    } else {
        Personenstatus::Still
    }
}

/// Dateigröße lesbar: „512 B", „3,4 KB", „1,2 MB", „2,0 GB".
pub fn groesse_text(byte: u64) -> String {
    const EINHEITEN: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if byte < 1024 {
        return format!("{byte} B");
    }
    let mut wert = byte as f64 / 1024.0;
    let mut i = 0;
    while wert >= 1024.0 && i < EINHEITEN.len() - 1 {
        wert /= 1024.0;
        i += 1;
    }
    format!("{wert:.1} {}", EINHEITEN[i]).replace('.', ",")
}

/// Ein Stück Chattext: Text oder Link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stueck {
    Text(String),
    Link(String),
}

/// Zerlegt Text in Text und `http(s)`-Links. Satzzeichen am Ende gehören
/// nicht zum Link; eine schließende Klammer nur, wenn im Link eine offen ist.
pub fn teile(text: &str) -> Vec<Stueck> {
    let mut stuecke = Vec::new();
    let mut zuletzt = 0;
    let klein = text.to_ascii_lowercase();
    let mut such = 0;
    while let Some(anfang) = naechster_link(&klein, such) {
        let ende = text[anfang..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .map(|i| anfang + i)
            .unwrap_or(text.len());
        let mut url = &text[anfang..ende];
        let ohne = url.trim_end_matches([
            '.', ',', ';', ':', '!', '?', '\'', '’', '“', '”', '»', ')', ']',
        ]);
        if ohne.len() < url.len() {
            let offen = ohne.matches('(').count() as i64 - ohne.matches(')').count() as i64;
            let rest = &url[ohne.len()..];
            url = if offen > 0 && rest.starts_with(')') {
                &url[..ohne.len() + 1]
            } else {
                ohne
            };
        }
        such = anfang + url.len().max(1);
        if !hat_host(url) {
            continue;
        }
        if anfang > zuletzt {
            stuecke.push(Stueck::Text(text[zuletzt..anfang].to_string()));
        }
        stuecke.push(Stueck::Link(url.to_string()));
        zuletzt = anfang + url.len();
    }
    if zuletzt < text.len() {
        stuecke.push(Stueck::Text(text[zuletzt..].to_string()));
    }
    stuecke
}

fn naechster_link(klein: &str, ab: usize) -> Option<usize> {
    let rest = klein.get(ab..)?;
    let a = rest.find("http://");
    let b = rest.find("https://");
    match (a, b) {
        (Some(x), Some(y)) => Some(ab + x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(ab + x),
        (None, None) => None,
    }
}

fn hat_host(url: &str) -> bool {
    let klein = url.to_ascii_lowercase();
    let rest = klein
        .strip_prefix("https://")
        .or_else(|| klein.strip_prefix("http://"))
        .unwrap_or("");
    rest.chars()
        .next()
        .is_some_and(|c| !matches!(c, '/' | '?' | '#'))
}

/// Der erste Link im Text (für die Vorschau des Entwurfs).
pub fn erster_link(text: &str) -> Option<String> {
    teile(text).into_iter().find_map(|s| match s {
        Stueck::Link(u) => Some(u),
        Stueck::Text(_) => None,
    })
}

/// Reaktionen erscheinen am Ziel, nicht als eigener Beitrag.
pub fn sichtbar(nachrichten: &[Nachricht]) -> impl Iterator<Item = &Nachricht> {
    nachrichten
        .iter()
        .filter(|n| !matches!(n.inhalt, NachrichtInhalt::Reaktion { .. }))
}

/// Wer sitzt in diesem Raum?
pub fn anwesende(z: &Zustand, raum: sprechwerk_domain::RoomId) -> impl Iterator<Item = &Mitglied> {
    z.mitglieder.iter().filter(move |m| m.raum == Some(raum))
}

/// Hat der Client den Schlüssel dieses Raums? `None`: Frage sinnlos (nicht
/// verschlüsselt oder unbekannter Raum).
pub fn schluessel_vorhanden(z: &Zustand, raum: sprechwerk_domain::RoomId) -> Option<bool> {
    let r = z.raeume.iter().find(|r| r.id == raum)?;
    if !r.verschluesselt {
        return None;
    }
    Some(z.schluessel.iter().any(|s| s.raum == raum))
}

/// Statuszeile: „keine Rahmen verloren" (gut) oder „3 Rahmen verloren".
pub fn qualitaet(sprache: Sprache, z: &Zustand) -> Option<(String, bool)> {
    let g = z.guete.as_ref()?;
    Some(if g.verloren == 0 {
        (t(sprache, "app.quality_ok"), true)
    } else {
        (tf(sprache, "app.quality_loss", &[&g.verloren]), false)
    })
}

/// Darf ich Sperren sehen und aufheben? (nur Administrator)
pub fn darf_sperren(z: &Zustand) -> bool {
    z.meine_rolle() == Some(sprechwerk_domain::Role::Administrator)
}

/// Darf ich in andere eingreifen (stumm, verschieben, trennen)?
pub fn darf_eingreifen(z: &Zustand) -> bool {
    matches!(
        z.meine_rolle(),
        Some(sprechwerk_domain::Role::Administrator | sprechwerk_domain::Role::Moderator)
    )
}

/// Eine Sprechtaste: Tastatur oder Maus 4/5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sprechtaste {
    Taste(eframe::egui::Key),
    Maus4,
    Maus5,
}

impl Sprechtaste {
    /// Aus dem gespeicherten Namen (wie die alte Oberfläche ihn schrieb).
    pub fn aus_name(name: &str) -> Option<Self> {
        use eframe::egui::Key;
        Some(match name {
            "Maus 4" => Sprechtaste::Maus4,
            "Maus 5" => Sprechtaste::Maus5,
            "Leertaste" => Sprechtaste::Taste(Key::Space),
            "Tab" => Sprechtaste::Taste(Key::Tab),
            "Eingabe" => Sprechtaste::Taste(Key::Enter),
            "Rücktaste" => Sprechtaste::Taste(Key::Backspace),
            andere => Sprechtaste::Taste(Key::from_name(andere)?),
        })
    }

    /// Name für die Einstellungsdatei und die Anzeige.
    pub fn name(self) -> String {
        use eframe::egui::Key;
        match self {
            Sprechtaste::Maus4 => "Maus 4".into(),
            Sprechtaste::Maus5 => "Maus 5".into(),
            Sprechtaste::Taste(Key::Space) => "Leertaste".into(),
            Sprechtaste::Taste(Key::Enter) => "Eingabe".into(),
            Sprechtaste::Taste(Key::Backspace) => "Rücktaste".into(),
            Sprechtaste::Taste(k) => k.name().to_string(),
        }
    }

    /// Schreibt die Taste ein Zeichen? Dann zählt sie im Eingabefeld nicht
    /// als Sprechtaste — sonst sendet man beim Tippen.
    pub fn schreibt(self) -> bool {
        use eframe::egui::Key;
        match self {
            Sprechtaste::Maus4 | Sprechtaste::Maus5 => false,
            Sprechtaste::Taste(k) => !matches!(
                k,
                Key::F1
                    | Key::F2
                    | Key::F3
                    | Key::F4
                    | Key::F5
                    | Key::F6
                    | Key::F7
                    | Key::F8
                    | Key::F9
                    | Key::F10
                    | Key::F11
                    | Key::F12
                    | Key::Insert
                    | Key::Pause
                    | Key::Escape
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kennung_wird_gruppiert_und_gekuerzt() {
        assert_eq!(
            kennung_gruppiert(&"ab".repeat(32), 16),
            "abab:abab:abab:abab…"
        );
        assert_eq!(kennung_gruppiert("ab12", 16), "ab12");
        assert_eq!(kennung_gruppiert("", 16), "");
    }

    #[test]
    fn fingerabdruck_vollstaendig() {
        assert_eq!(fingerabdruck_gruppiert("AB12CD34ef"), "ab12:cd34:ef");
    }

    #[test]
    fn zeit_in_worten() {
        let j = 10_000_000_000;
        assert_eq!(zuletzt_text(Sprache::De, 0, j), None);
        assert_eq!(
            zuletzt_text(Sprache::De, j - 10_000, j).unwrap(),
            "gerade eben"
        );
        assert_eq!(
            zuletzt_text(Sprache::De, j - 5 * 60_000, j).unwrap(),
            "vor 5 Minuten"
        );
        assert_eq!(
            zuletzt_text(Sprache::De, j - 3 * 3_600_000, j).unwrap(),
            "vor 3 Stunden"
        );
    }

    #[test]
    fn latenz_lesbar() {
        assert_eq!(latenz_text(None), None);
        assert_eq!(latenz_text(Some(8)).unwrap(), "8 ms");
        assert_eq!(latenz_text(Some(1240)).unwrap(), "1,2 s");
    }

    #[test]
    fn uhrzeit_mit_versatz() {
        assert_eq!(uhrzeit(0, 0), "00:00");
        assert_eq!(uhrzeit(13 * 3_600_000 + 5 * 60_000, 3600), "14:05");
    }

    fn nachricht(von: u128, zeit: i64) -> Nachricht {
        Nachricht {
            id: sprechwerk_domain::MessageId::aus_roh(zeit as u128),
            verfasser: sprechwerk_domain::MemberId::aus_roh(von),
            verfasser_name: "x".into(),
            inhalt: NachrichtInhalt::Geloescht,
            roh: None,
            zeit,
            geloescht: false,
            loeschbar: false,
            reaktionen: vec![],
        }
    }

    #[test]
    fn fenster_wird_kompakt_und_wieder_breit() {
        assert!(mindestbreite(true) < mindestbreite(false));
        assert_eq!(
            breite_nach_wechsel(true, None, None),
            BREITE_KOMPAKT,
            "erstes Mal"
        );
        assert_eq!(
            breite_nach_wechsel(true, None, Some(400.0)),
            400.0,
            "kompakte Breite von vorher"
        );
        assert_eq!(
            breite_nach_wechsel(false, Some(1100.0), None),
            1100.0,
            "Breite mit Chat von vorher"
        );
        assert_eq!(
            breite_nach_wechsel(false, Some(300.0), None),
            BREITE_MIT_CHAT,
            "nie schmaler als mit Chat nötig"
        );
        const { assert!(BREITE_KOMPAKT >= BREITE_KOMPAKT_MIN) };
    }

    #[test]
    fn bloecke_nach_person_und_pause() {
        let a = nachricht(1, 0);
        let b = nachricht(1, 60_000);
        let c = nachricht(2, 120_000);
        let d = nachricht(2, 120_000 + 6 * 60_000);
        assert!(neuer_block(None, &a));
        assert!(!neuer_block(Some(&a), &b), "dieselbe Person, gleich danach");
        assert!(neuer_block(Some(&b), &c), "andere Person");
        assert!(neuer_block(Some(&c), &d), "nach einer Pause");
    }

    #[test]
    fn status_taub_vor_stumm_vor_spricht() {
        let mut m = Mitglied {
            id: sprechwerk_domain::MemberId::aus_roh(1),
            name: "A".into(),
            rolle: sprechwerk_domain::Role::Mitglied,
            raum: None,
            spricht: true,
            stumm: false,
            taub: false,
            ich: false,
        };
        assert_eq!(personenstatus(&m, None), Personenstatus::Spricht);
        assert_eq!(
            personenstatus(&m, Some(false)),
            Personenstatus::Still,
            "eigene Messung zählt"
        );
        m.stumm = true;
        assert_eq!(personenstatus(&m, None), Personenstatus::Stumm);
        m.taub = true;
        assert_eq!(personenstatus(&m, None), Personenstatus::Taub);
    }

    #[test]
    fn ich_spreche_ueber_schwelle_oder_mit_taste() {
        let a = |pegel, stumm| sprechwerk_client::AudioStand {
            laeuft: true,
            pegel,
            stumm,
            taub: false,
        };
        assert!(ich_spreche(&a(40, false), 30, false, false));
        assert!(
            !ich_spreche(&a(20, false), 30, false, false),
            "unter der Schwelle"
        );
        assert!(!ich_spreche(&a(40, true), 30, false, false), "stumm");
        assert!(
            !ich_spreche(&a(40, false), 30, true, false),
            "Taste nicht gedrückt"
        );
        assert!(
            ich_spreche(&a(5, false), 30, true, true),
            "Taste gedrückt, Ton da"
        );
        assert!(
            !ich_spreche(&a(0, false), 30, true, true),
            "Taste, aber kein Ton"
        );
    }

    #[test]
    fn emoji_an_der_cursorstelle() {
        let mut t = "Hallo Welt".to_string();
        let danach = einfuegen(&mut t, 5, " 👋");
        assert_eq!(t, "Hallo 👋 Welt");
        assert_eq!(danach, 7);
        let mut u = "Grüße".to_string();
        assert_eq!(einfuegen(&mut u, 99, "!"), 6, "hinter dem Ende: angehängt");
        assert_eq!(u, "Grüße!");
    }

    #[test]
    fn variantenzeichen_fallen_beim_anzeigen_weg() {
        assert_eq!(darstellbar("❤️ und ✌️"), "❤ und ✌");
        assert_eq!(darstellbar("ohne"), "ohne");
    }

    #[test]
    fn emoji_auswahl_ohne_doppelte() {
        let mut alle: Vec<&str> = EMOJI.iter().flat_map(|(_, e)| e.iter().copied()).collect();
        let n = alle.len();
        alle.sort();
        alle.dedup();
        assert_eq!(alle.len(), n);
        for r in REAKTIONEN {
            assert!(
                n > 0 && EMOJI.iter().any(|(_, e)| e.contains(&r)),
                "{r} fehlt in der Auswahl"
            );
        }
    }

    #[test]
    fn groessen_lesbar() {
        assert_eq!(groesse_text(512), "512 B");
        assert_eq!(groesse_text(3 * 1024 + 400), "3,4 KB");
        assert_eq!(groesse_text(1_258_291), "1,2 MB");
        assert_eq!(groesse_text(2 * 1024 * 1024 * 1024), "2,0 GB");
    }

    #[test]
    fn links_im_text() {
        assert_eq!(
            teile("schau https://example.org/a. gut"),
            vec![
                Stueck::Text("schau ".into()),
                Stueck::Link("https://example.org/a".into()),
                Stueck::Text(". gut".into()),
            ]
        );
        assert_eq!(
            teile("(siehe https://de.wikipedia.org/wiki/X_(Y))"),
            vec![
                Stueck::Text("(siehe ".into()),
                Stueck::Link("https://de.wikipedia.org/wiki/X_(Y)".into()),
                Stueck::Text(")".into()),
            ]
        );
        assert_eq!(
            teile("nur http:// ohne host"),
            vec![Stueck::Text("nur http:// ohne host".into())]
        );
        assert_eq!(
            erster_link("a HTTPS://x.de b").as_deref(),
            Some("HTTPS://x.de")
        );
        assert_eq!(erster_link("kein link"), None);
    }

    #[test]
    fn umlaute_vor_links_brechen_nichts() {
        let s = teile("Grüße: https://ö.example/ä");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1], Stueck::Link("https://ö.example/ä".into()));
    }

    #[test]
    fn sprechtaste_namen() {
        use eframe::egui::Key;
        for name in ["Maus 4", "Maus 5", "Leertaste", "F8", "A"] {
            let t = Sprechtaste::aus_name(name).unwrap();
            assert_eq!(t.name(), name);
        }
        assert!(Sprechtaste::Taste(Key::A).schreibt());
        assert!(!Sprechtaste::Taste(Key::F8).schreibt());
        assert!(!Sprechtaste::Maus4.schreibt());
    }
}

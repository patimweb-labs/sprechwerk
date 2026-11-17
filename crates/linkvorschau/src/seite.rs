//! Titel, Beschreibung und Bild aus dem Kopf einer HTML-Seite lesen.
//!
//! Rein und ohne Netz: Text hinein, [`Seite`] heraus. Kein vollständiger
//! HTML-Leser — gelesen werden nur `<title>` und `<meta>` im Kopf, und dafür
//! reicht ein sorgfältiger Durchlauf. Ein ganzer HTML-Parser wäre eine große
//! Abhängigkeit für vier Felder.

use url::Url;

/// Was aus einer Seite gelesen wurde.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seite {
    pub titel: Option<String>,
    pub beschreibung: Option<String>,
    /// Absolute Adresse des Vorschaubilds (`http(s)`).
    pub bild: Option<Url>,
    /// `og:site_name` — Ersatz, wenn der Titel fehlt.
    pub seitenname: Option<String>,
}

/// Liest den Kopf einer Seite. `basis` ist die Adresse, unter der sie geladen
/// wurde (nach Weiterleitungen) — relative Bildadressen gelten gegen sie.
pub fn lesen(html: &str, basis: &Url) -> Seite {
    // Nur der Kopf: dort stehen die Angaben, und ein `<title>` in einem SVG
    // weiter unten ist nicht der Titel der Seite.
    let kopf = match finde_ohne_gross(html, "</head") {
        Some(ende) => &html[..ende],
        None => html,
    };

    let mut og_titel = None;
    let mut tw_titel = None;
    let mut titel = None;
    let mut og_beschreibung = None;
    let mut tw_beschreibung = None;
    let mut beschreibung = None;
    let mut og_bild = None;
    let mut tw_bild = None;
    let mut seitenname = None;

    let mut rest = kopf;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '!' || *c == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        match name.as_str() {
            "title" => {
                let Some(auf) = rest.find('>') else { break };
                let inhalt = &rest[auf + 1..];
                let zu = finde_ohne_gross(inhalt, "</title").unwrap_or(inhalt.len());
                if titel.is_none() {
                    titel = sauber(&inhalt[..zu]);
                }
                rest = &inhalt[zu..];
            }
            "meta" => {
                let (attribute, weiter) = attribute_lesen(&rest[name.len()..]);
                rest = weiter;
                let schluessel = attribute
                    .iter()
                    .find(|(n, _)| n == "property" || n == "name")
                    .map(|(_, w)| w.to_ascii_lowercase());
                let wert = attribute
                    .iter()
                    .find(|(n, _)| n == "content")
                    .and_then(|(_, w)| sauber(w));
                let (Some(schluessel), Some(wert)) = (schluessel, wert) else {
                    continue;
                };
                let ziel = match schluessel.as_str() {
                    "og:title" => &mut og_titel,
                    "twitter:title" => &mut tw_titel,
                    "og:description" => &mut og_beschreibung,
                    "twitter:description" => &mut tw_beschreibung,
                    "description" => &mut beschreibung,
                    "og:image" | "og:image:url" | "og:image:secure_url" => &mut og_bild,
                    "twitter:image" | "twitter:image:src" => &mut tw_bild,
                    "og:site_name" => &mut seitenname,
                    _ => continue,
                };
                if ziel.is_none() {
                    *ziel = Some(wert);
                }
            }
            // Kommentare, Skripte und Stile überspringen: was darin nach
            // einem Tag aussieht, ist keiner.
            n if n.starts_with("!--") => {
                rest = match rest.find("-->") {
                    Some(e) => &rest[e + 3..],
                    None => "",
                };
            }
            "script" | "style" => {
                let ende = format!("</{name}");
                rest = match finde_ohne_gross(rest, &ende) {
                    Some(e) => &rest[e + ende.len()..],
                    None => "",
                };
            }
            _ => {}
        }
    }

    let bild = og_bild.or(tw_bild).and_then(|b| {
        let u = basis.join(&b).ok()?;
        matches!(u.scheme(), "http" | "https").then_some(u)
    });
    Seite {
        titel: og_titel.or(tw_titel).or(titel),
        beschreibung: og_beschreibung.or(tw_beschreibung).or(beschreibung),
        bild,
        seitenname,
    }
}

/// Liest die Attribute eines Tags bis `>`; gibt sie (Namen klein) und den
/// Rest hinter dem Tag zurück.
fn attribute_lesen(text: &str) -> (Vec<(String, String)>, &str) {
    let mut attribute = Vec::new();
    let mut zeichen = text.char_indices().peekable();
    loop {
        // Leerraum und `/` überspringen
        while let Some(&(_, c)) = zeichen.peek() {
            if c.is_whitespace() || c == '/' {
                zeichen.next();
            } else {
                break;
            }
        }
        let Some(&(anfang, c)) = zeichen.peek() else {
            return (attribute, "");
        };
        if c == '>' {
            return (attribute, &text[anfang + 1..]);
        }
        let mut name = String::new();
        while let Some(&(_, c)) = zeichen.peek() {
            if c.is_whitespace() || c == '=' || c == '>' || c == '/' {
                break;
            }
            name.push(c.to_ascii_lowercase());
            zeichen.next();
        }
        while let Some(&(_, c)) = zeichen.peek() {
            if c.is_whitespace() {
                zeichen.next();
            } else {
                break;
            }
        }
        let mut wert = String::new();
        if let Some(&(_, '=')) = zeichen.peek() {
            zeichen.next();
            while let Some(&(_, c)) = zeichen.peek() {
                if c.is_whitespace() {
                    zeichen.next();
                } else {
                    break;
                }
            }
            match zeichen.peek() {
                Some(&(_, q)) if q == '"' || q == '\'' => {
                    zeichen.next();
                    for (_, c) in zeichen.by_ref() {
                        if c == q {
                            break;
                        }
                        wert.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = zeichen.peek() {
                        if c.is_whitespace() || c == '>' {
                            break;
                        }
                        wert.push(c);
                        zeichen.next();
                    }
                }
            }
        }
        if name.is_empty() {
            // Ein Zeichen, das kein Name ist: weiter, sonst hängt die Schleife.
            zeichen.next();
            continue;
        }
        attribute.push((name, wert));
    }
}

/// Sucht `nadel` (ASCII, klein geschrieben) ohne Rücksicht auf Groß/klein.
fn finde_ohne_gross(text: &str, nadel: &str) -> Option<usize> {
    let heu = text.as_bytes();
    let n = nadel.as_bytes();
    if n.is_empty() || heu.len() < n.len() {
        return None;
    }
    (0..=heu.len() - n.len()).find(|&i| heu[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Entschlüsselt Zeichenreferenzen, fasst Leerraum zusammen; leer → `None`.
fn sauber(text: &str) -> Option<String> {
    let klar = zeichenreferenzen(text);
    let zusammen = klar.split_whitespace().collect::<Vec<_>>().join(" ");
    (!zusammen.is_empty()).then_some(zusammen)
}

fn zeichenreferenzen(text: &str) -> String {
    let mut aus = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        aus.push_str(&rest[..i]);
        rest = &rest[i..];
        let ende = rest[1..].find(';').map(|e| e + 1).filter(|e| *e <= 12);
        let ersatz = ende.and_then(|e| referenz(&rest[1..e]));
        match (ende, ersatz) {
            (Some(e), Some(c)) => {
                aus.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                aus.push('&');
                rest = &rest[1..];
            }
        }
    }
    aus.push_str(rest);
    aus
}

fn referenz(name: &str) -> Option<char> {
    if let Some(zahl) = name.strip_prefix('#') {
        let wert = match zahl.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => zahl.parse().ok()?,
        };
        return char::from_u32(wert).filter(|c| !c.is_control() || c.is_whitespace());
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "bdquo" => '„',
        "ldquo" => '“',
        "rdquo" => '”',
        "lsquo" => '‘',
        "rsquo" => '’',
        "auml" => 'ä',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "Auml" => 'Ä',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "szlig" => 'ß',
        "euro" => '€',
        "copy" => '©',
        "reg" => '®',
        "middot" => '·',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis() -> Url {
        Url::parse("https://example.org/artikel/eins").unwrap()
    }

    #[test]
    fn open_graph_geht_vor() {
        let html = r#"<!doctype html><html><head>
            <title>Schlichter Titel</title>
            <meta name="description" content="Kurz">
            <meta property="og:title" content="Großer Titel &amp; mehr">
            <meta property="og:description" content="Lang  und
                 umbrochen">
            <meta property="og:image" content="/bilder/vorschau.jpg">
            <meta property="og:site_name" content="Beispiel">
        </head><body><svg><title>falsch</title></svg></body></html>"#;
        let s = lesen(html, &basis());
        assert_eq!(s.titel.as_deref(), Some("Großer Titel & mehr"));
        assert_eq!(s.beschreibung.as_deref(), Some("Lang und umbrochen"));
        assert_eq!(
            s.bild.unwrap().as_str(),
            "https://example.org/bilder/vorschau.jpg"
        );
        assert_eq!(s.seitenname.as_deref(), Some("Beispiel"));
    }

    #[test]
    fn ohne_open_graph_der_titel() {
        let html = "<HTML><HEAD><TITLE>  Verein &#8211; Startseite </TITLE>\
                    <META NAME='description' CONTENT='Wir &#x1F44B;'></HEAD></HTML>";
        let s = lesen(html, &basis());
        assert_eq!(s.titel.as_deref(), Some("Verein – Startseite"));
        assert_eq!(s.beschreibung.as_deref(), Some("Wir 👋"));
        assert!(s.bild.is_none());
    }

    #[test]
    fn reihenfolge_der_attribute_ist_egal_und_ungequotet_geht() {
        let html = r#"<meta content="Titel" property="og:title"/><meta property=og:image content=bild.png>"#;
        let s = lesen(html, &basis());
        assert_eq!(s.titel.as_deref(), Some("Titel"));
        assert_eq!(
            s.bild.unwrap().as_str(),
            "https://example.org/artikel/bild.png"
        );
    }

    #[test]
    fn skripte_und_kommentare_zaehlen_nicht() {
        let html = r#"<head>
            <!-- <meta property="og:title" content="aus dem Kommentar"> -->
            <script>var s = '<meta property="og:title" content="aus dem Skript">';</script>
            <meta property="og:title" content="echt">
        </head>"#;
        assert_eq!(lesen(html, &basis()).titel.as_deref(), Some("echt"));
    }

    #[test]
    fn kein_bild_ausser_http() {
        for b in [
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "file:///x.png",
        ] {
            let html = format!(r#"<meta property="og:image" content="{b}">"#);
            assert!(lesen(&html, &basis()).bild.is_none(), "{b}");
        }
    }

    #[test]
    fn kaputtes_html_haelt_nicht_an() {
        for html in [
            "<",
            "<meta",
            "<meta property=\"og:title",
            "<title>ohne Ende",
            "<!-- offen",
            "<script>offen",
            "&&&;&#xZZ;&#99999999;",
            "<meta = = => <meta ==>",
        ] {
            let _ = lesen(html, &basis());
        }
        assert_eq!(
            lesen("<title>ohne Ende", &basis()).titel.as_deref(),
            Some("ohne Ende")
        );
        assert_eq!(
            zeichenreferenzen("a & b &unbekannt; c"),
            "a & b &unbekannt; c"
        );
    }
}

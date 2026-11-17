// Reine Hilfen für den Chat: Links im Text finden, sichtbare Beiträge wählen,
// Teilchen für Effekte würfeln. Keine Vue, kein DOM — geprüft in
// `pruefung/chat.test.ts`.
import type { Effekt, Nachricht } from "./api";

/** Die Zeichen im Reaktionsmenü (Reihenfolge = Anzeige). */
export const REAKTIONEN = ["👍", "❤️", "😂", "😮", "😢", "🎉"] as const;

/** Die Effekte, die man einer Nachricht mitgeben kann. */
export const EFFEKTE: readonly Effekt[] = ["konfetti", "herzen", "beben"];

/** Ein Stück Text: schlicht oder ein Link. */
export type Stueck = { art: "text"; text: string } | { art: "link"; text: string; url: string };

// `http(s)://` bis zum nächsten Leerraum oder einer spitzen Klammer.
const LINK = /https?:\/\/[^\s<>"]+/gi;
// Was am Ende eines Links meist zum Satz gehört, nicht zur Adresse.
const SATZZEICHEN = /[.,;:!?'’“”»)\]]+$/;

/**
 * Zerlegt Text in Stücke mit anklickbaren Links. Satzzeichen am Ende gehören
 * zum Satz („siehe https://a.example/x." → Link ohne Punkt); eine schließende
 * Klammer bleibt, wenn der Link selbst eine geöffnet hat (Wikipedia).
 */
export function teile(text: string): Stueck[] {
  const stuecke: Stueck[] = [];
  let zuletzt = 0;
  for (const treffer of text.matchAll(LINK)) {
    const anfang = treffer.index ?? 0;
    let url = treffer[0];
    const rest = url.match(SATZZEICHEN)?.[0] ?? "";
    if (rest) {
      url = url.slice(0, url.length - rest.length);
      // „(…)" im Link: die Klammer zurückholen, die zu ihm gehört.
      const offen = (url.match(/\(/g) ?? []).length - (url.match(/\)/g) ?? []).length;
      if (offen > 0 && rest.startsWith(")")) url += ")";
    }
    if (!/^https?:\/\/[^/?#]+/i.test(url)) continue;
    if (anfang > zuletzt) stuecke.push({ art: "text", text: text.slice(zuletzt, anfang) });
    stuecke.push({ art: "link", text: url, url });
    zuletzt = anfang + url.length;
  }
  if (zuletzt < text.length) stuecke.push({ art: "text", text: text.slice(zuletzt) });
  return stuecke;
}

/** Der erste Link im Text — für die Vorschau des Entwurfs. */
export function ersterLink(text: string): string | null {
  for (const s of teile(text)) if (s.art === "link") return s.url;
  return null;
}

/** Was im Verlauf als Beitrag steht: Reaktionen hängen an ihrem Ziel. */
export function sichtbar(nachrichten: Nachricht[]): Nachricht[] {
  return nachrichten.filter((n) => n.inhalt.zustand !== "reaktion");
}

/** Ein Teilchen eines Effekts. Werte in Prozent, Sekunden und Grad. */
export interface Teilchen {
  /** Waagrechte Lage in Prozent der Breite. */
  x: number;
  /** Verzögerung in Sekunden. */
  verzug: number;
  /** Dauer in Sekunden. */
  dauer: number;
  /** Drehung am Ende in Grad. */
  drehung: number;
  /** Seitliche Drift in Prozent der Breite. */
  drift: number;
  /** Welche der fünf Farben (0–4). */
  farbe: number;
}

/**
 * Würfelt die Teilchen eines Auftritts — **reproduzierbar** aus `nr`: derselbe
 * Auftritt sieht bei jedem Zeichnen gleich aus, ein neuer anders.
 */
export function teilchen(nr: number, anzahl: number): Teilchen[] {
  // mulberry32: klein, schnell, gut genug für Konfetti.
  let s = (nr * 2654435761) >>> 0;
  const zufall = () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return Array.from({ length: anzahl }, () => ({
    x: Math.round(zufall() * 1000) / 10,
    verzug: Math.round(zufall() * 60) / 100,
    dauer: Math.round((1.6 + zufall() * 1.4) * 100) / 100,
    drehung: Math.round(zufall() * 720 - 360),
    drift: Math.round((zufall() * 20 - 10) * 10) / 10,
    farbe: Math.floor(zufall() * 5),
  }));
}

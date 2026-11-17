// Push-to-Talk: welche Taste zählt — und wann nicht.
//
// Hier und nicht in `App.vue`/`EinstellungenView.vue`, damit es prüfbar ist
// (AGENTS.md §2.4) — und damit Aufnehmen und Erkennen **dieselbe** Tabelle
// benutzen. Vorher kannte die Erkennung nur drei Tasten; jede andere, die man
// in den Einstellungen aufnehmen konnte („Strg rechts", „F", „F13"), funkte nie.

/** Was von einem Tastenereignis gebraucht wird (prüfbar ohne DOM-Ereignis). */
export interface Taste {
  code: string;
  /** Wohin getippt wird: das fokussierte Element. */
  ziel: { tagName?: string; isContentEditable?: boolean; type?: string } | null;
}

/**
 * Gespeicherter Name ↔ `KeyboardEvent.code`.
 *
 * Die Namen stehen so in den gespeicherten Einstellungen; sie zu ändern hieße,
 * bestehende Belegungen zu verlieren.
 */
const NAMEN: Record<string, string> = {
  Space: "Leertaste",
  ControlLeft: "Strg links",
  ControlRight: "Strg rechts",
  AltLeft: "Alt links",
  AltRight: "Alt rechts",
  ShiftLeft: "Umschalt links",
  ShiftRight: "Umschalt rechts",
  MetaLeft: "Meta links",
  MetaRight: "Meta rechts",
  CapsLock: "Feststelltaste",
  Tab: "Tab",
  Enter: "Eingabe",
  Backspace: "Rücktaste",
};

const CODES: Record<string, string> = Object.fromEntries(
  Object.entries(NAMEN).map(([code, name]) => [name, code]),
);

/** Maustasten: Name → `MouseEvent.button` (Zählung ab 0, „Maus 4" ist 3). */
export const MAUSTASTEN: Record<string, number> = { "Maus 4": 3, "Maus 5": 4 };

/** Name, unter dem eine Taste gespeichert wird. */
export function tasteName(code: string): string {
  if (NAMEN[code]) return NAMEN[code];
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  // F-Tasten, Ziffernblock, Satzzeichen …: der Code selbst ist eindeutig.
  return code;
}

/** Umkehrung von `tasteName`. */
export function codeFuer(name: string): string {
  if (CODES[name]) return CODES[name];
  if (/^[A-Z]$/.test(name)) return `Key${name}`;
  if (/^\d$/.test(name)) return `Digit${name}`;
  return name;
}

/** Tasten, die in einem Textfeld **nichts** schreiben. */
function schreibtNicht(code: string): boolean {
  return /^(Control|Alt|Shift|Meta)(Left|Right)$/.test(code) || code === "CapsLock" || /^F\d{1,2}$/.test(code);
}

/** Eingabearten von `<input>`, in die man nichts schreibt. */
const OHNE_TEXT = new Set(["button", "checkbox", "radio", "range", "submit", "reset", "color", "file"]);

/** Liegt der Fokus in einem Feld, in das man schreibt? */
export function schreibfeld(ziel: Taste["ziel"]): boolean {
  if (!ziel) return false;
  if (ziel.isContentEditable) return true;
  const tag = (ziel.tagName ?? "").toUpperCase();
  if (tag === "TEXTAREA") return true;
  if (tag === "INPUT") return !OHNE_TEXT.has((ziel.type ?? "text").toLowerCase());
  return false;
}

/** Was aus den Einstellungen zählt. */
export interface Belegung {
  /** Push-to-Talk eingeschaltet? Aus heißt: keine Taste wird abgefangen. */
  aktiv: boolean;
  taste: string;
}

/**
 * Wird mit diesem Tastendruck gesprochen?
 *
 * * Push-to-Talk aus → nie. Sonst schluckte die Leertaste auch ohne
 *   Push-to-Talk jeden Druck: kein Leerzeichen, kein Knopf per Tastatur.
 * * Eine Taste, die Text schreibt (Leertaste, Buchstaben), gehört dem
 *   Textfeld, solange dort geschrieben wird. Strg/Alt/F-Tasten schreiben nichts
 *   und funken auch aus dem Feld heraus.
 */
export function istSprechtaste(belegung: Belegung, taste: Taste): boolean {
  if (!belegung.aktiv || belegung.taste in MAUSTASTEN) return false;
  if (taste.code !== codeFuer(belegung.taste)) return false;
  return schreibtNicht(taste.code) || !schreibfeld(taste.ziel);
}

/**
 * Wird mit diesem Loslassen aufgehört?
 *
 * Ohne Rücksicht auf das Ziel: wer außerhalb drückt und im Feld loslässt
 * (Fokus wechselt per Maus), soll nicht weiter senden.
 */
export function istSprechtasteLos(belegung: Belegung, code: string): boolean {
  return belegung.aktiv && !(belegung.taste in MAUSTASTEN) && code === codeFuer(belegung.taste);
}

/** Maustaste dieser Belegung, oder `null`. */
export function sprechMaustaste(belegung: Belegung): number | null {
  if (!belegung.aktiv) return null;
  return MAUSTASTEN[belegung.taste] ?? null;
}

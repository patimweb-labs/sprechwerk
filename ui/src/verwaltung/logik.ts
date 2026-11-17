// Verwaltung im Browser: Modell und reine Funktionen.
//
// Die Ansicht (`VerwaltungApp.vue`) rechnet nichts (AGENTS.md §2.4); was hier
// steht, hat Tests in `pruefung/verwaltung.test.ts`.

export type Rolle = "administrator" | "moderator" | "mitglied" | "gast";
export const ROLLEN: Rolle[] = ["administrator", "moderator", "mitglied", "gast"];

/** Ein Mitglied, wie `GET /api/stand` es liefert (`verwaltung.rs`). */
export interface Mitgliedszeile {
  id: string;
  name: string;
  kennung: string;
  rolle: Rolle;
  verbunden: boolean;
  raum: string | null;
  zuletzt_gesehen: number;
}

export interface Stand {
  name: string;
  version: string;
  adresse: string;
  port: number;
  fingerabdruck: string;
  gestartet: number;
  jetzt: number;
  raeume: number;
  verbundene: number;
  erster_wird_administrator: boolean;
  sicherung: { aktiv: boolean; verzeichnis: string; letzte: number | null };
  mitglieder: Mitgliedszeile[];
}

export interface Protokollzeile {
  nr: number;
  text: string;
}

/** Was die Seite zeigt — ein Summentyp statt Booleans (AGENTS.md §2.2). */
export type Ansicht =
  | { art: "laedt" }
  | { art: "anmelden"; fehler: string | null; sendet: boolean }
  | { art: "fertig"; stand: Stand }
  | { art: "fehler"; text: string };

/** Schwere einer Protokollzeile, aus dem Text von `tracing`. */
export type Stufe = "fehler" | "warnung" | "info" | "leise";

export function stufe(zeile: string): Stufe {
  // `tracing` schreibt die Stufe nach dem Zeitstempel, rechtsbündig auf fünf
  // Zeichen: `2026-11-17T… ERROR …`, `…  WARN …`, `…  INFO …`.
  const m = /\b(ERROR|WARN|INFO|DEBUG|TRACE)\b/.exec(zeile);
  switch (m?.[1]) {
    case "ERROR":
      return "fehler";
    case "WARN":
      return "warnung";
    case "INFO":
      return "info";
    case undefined:
      return "info";
    default:
      return "leise";
  }
}

/** Höchstens so viele Zeilen hält die Seite (der Server hält mehr). */
export const ZEILEN_IM_FENSTER = 2000;

/**
 * Hängt neue Zeilen an, ohne doppelte (eine Antwort kann sich mit der
 * vorigen überschneiden, wenn zwei Abfragen sich überholen) und ohne endloses
 * Wachstum.
 */
export function anhaengen(
  alt: Protokollzeile[],
  neu: Protokollzeile[],
  grenze = ZEILEN_IM_FENSTER,
): Protokollzeile[] {
  const letzte = alt.length ? alt[alt.length - 1].nr : -1;
  const frisch = neu.filter((z) => z.nr > letzte);
  if (!frisch.length) return alt;
  const alles = alt.concat(frisch);
  return alles.length > grenze ? alles.slice(alles.length - grenze) : alles;
}

/** „3 h 12 min", „5 min", „2 T 4 h" — Laufzeit seit dem Start. */
export function dauer(ms: number): string {
  const minuten = Math.max(0, Math.floor(ms / 60_000));
  const tage = Math.floor(minuten / 1440);
  const stunden = Math.floor((minuten % 1440) / 60);
  const rest = minuten % 60;
  if (tage > 0) return `${tage} d ${stunden} h`;
  if (stunden > 0) return `${stunden} h ${rest} min`;
  return `${rest} min`;
}

/** Datum und Uhrzeit in der Sprache des Browsers. */
export function zeitpunkt(ms: number | null, sprache = "de"): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(sprache, {
    dateStyle: "short",
    timeStyle: "short",
  });
}

/** Wie viele Administratoren gibt es? Der letzte lässt sich nicht herabstufen. */
export function administratoren(stand: Stand): number {
  return stand.mitglieder.filter((m) => m.rolle === "administrator").length;
}

/**
 * Darf diese Rolle für dieses Mitglied gewählt werden? Dieselbe Regel wie im
 * Server (`rolle_umsetzen`): der letzte Administrator bleibt. Der Server prüft
 * trotzdem — hier geht es nur darum, keine Auswahl anzubieten, die scheitert.
 */
export function rolleWaehlbar(stand: Stand, m: Mitgliedszeile, rolle: Rolle): boolean {
  if (m.rolle !== "administrator" || rolle === "administrator") return true;
  return administratoren(stand) > 1;
}

// Verwaltung: Zustand und Gespräch mit dem Server (`crates/server/src/verwaltung.rs`).
//
// Kein WebSocket, keine Ereignisse: die Seite fragt im Takt. Für einen
// Betreiber, der ab und zu hinsieht, ist das genug — und es übersteht einen
// Neustart des Servers ohne eigene Wiederverbindungslogik.
//
// Angemeldet wird über einen Keks, den der Server setzt (`HttpOnly`); diese
// Datei sieht den Code nur in dem Moment, in dem er eingegeben wird.
import { reactive, ref } from "vue";
import { setSprache, sprache } from "../i18n";
import {
  anhaengen,
  type Ansicht,
  type Protokollzeile,
  type Rolle,
  type Stand,
} from "./logik";

export const ansicht = ref<Ansicht>({ art: "laedt" });

export const protokoll = reactive<{ zeilen: Protokollzeile[]; weiterAb: number }>({
  zeilen: [],
  weiterAb: 0,
});

/** Letzte Rückmeldung einer Handlung (Rolle gesetzt, abgelehnt). */
export const meldung = ref<{ ton: "ok" | "error"; schluessel: string; name?: string } | null>(
  null,
);

/** Takt der Abfragen in Millisekunden. */
export const TAKT_STAND = 3000;
export const TAKT_PROTOKOLL = 1000;

class NichtAngemeldet extends Error {}

async function holen<T>(pfad: string, art?: { methode: "POST"; daten?: unknown }): Promise<T> {
  const antwort = await fetch(pfad, {
    method: art?.methode ?? "GET",
    credentials: "same-origin",
    headers: art ? { "Content-Type": "application/json" } : undefined,
    body: art?.daten === undefined ? undefined : JSON.stringify(art.daten),
  });
  if (antwort.status === 401) throw new NichtAngemeldet();
  if (antwort.status === 204) return undefined as T;
  const text = await antwort.text();
  const daten = text ? JSON.parse(text) : undefined;
  if (!antwort.ok) {
    throw new Error((daten as { fehler?: string } | undefined)?.fehler ?? `HTTP ${antwort.status}`);
  }
  return daten as T;
}

function abgemeldet(fehler: string | null = null) {
  ansicht.value = { art: "anmelden", fehler, sendet: false };
  protokoll.zeilen = [];
  protokoll.weiterAb = 0;
}

/** Holt den Stand. Unterscheidet „nicht angemeldet" von „Server weg". */
export async function standLaden(): Promise<void> {
  try {
    const stand = await holen<Stand>("/api/stand");
    ansicht.value = { art: "fertig", stand };
  } catch (fehler) {
    if (fehler instanceof NichtAngemeldet) {
      if (ansicht.value.art !== "anmelden") abgemeldet();
    } else {
      ansicht.value = { art: "fehler", text: String(fehler) };
    }
  }
}

export async function protokollLaden(): Promise<void> {
  if (ansicht.value.art !== "fertig") return;
  try {
    const auszug = await holen<{ zeilen: Protokollzeile[]; weiter_ab: number }>(
      `/api/protokoll?ab=${protokoll.weiterAb}`,
    );
    protokoll.zeilen = anhaengen(protokoll.zeilen, auszug.zeilen);
    protokoll.weiterAb = auszug.weiter_ab;
  } catch {
    // Der nächste `standLaden` meldet, was los ist.
  }
}

export async function anmelden(code: string): Promise<void> {
  ansicht.value = { art: "anmelden", fehler: null, sendet: true };
  try {
    await holen("/api/anmelden", { methode: "POST", daten: { code } });
    await standLaden();
    await protokollLaden();
  } catch (fehler) {
    abgemeldet(fehler instanceof NichtAngemeldet ? "admin.login.wrong" : "admin.error.server");
  }
}

export async function abmelden(): Promise<void> {
  try {
    await holen("/api/abmelden", { methode: "POST" });
  } finally {
    abgemeldet();
  }
}

export async function rolleSetzen(mitglied: string, name: string, rolle: Rolle): Promise<void> {
  try {
    const stand = await holen<Stand>("/api/rolle", { methode: "POST", daten: { mitglied, rolle } });
    ansicht.value = { art: "fertig", stand };
    meldung.value = { ton: "ok", schluessel: "admin.role.set", name };
  } catch (fehler) {
    if (fehler instanceof NichtAngemeldet) return abgemeldet();
    const text = String((fehler as Error).message);
    meldung.value = {
      ton: "error",
      schluessel: text === "letzter_administrator" ? "admin.role.last" : "admin.role.failed",
      name,
    };
  }
}

// ---- Thema und Sprache -------------------------------------------------

const THEMEN = ["dark", "light", "mono"] as const;
export type Thema = (typeof THEMEN)[number];
export const thema = ref<Thema>("dark");

/** Gemerkt je Browser — die Verwaltung hat keine Einstellungen am Server. */
export function wendeThema(neu: string) {
  thema.value = (THEMEN as readonly string[]).includes(neu) ? (neu as Thema) : "dark";
  document.documentElement.dataset.theme = thema.value;
  try {
    localStorage.setItem("sprechwerk-verwaltung-thema", thema.value);
  } catch {
    // privates Fenster: dann eben nicht gemerkt
  }
}

export function vorlieben() {
  let gemerkt: string | null = null;
  try {
    gemerkt = localStorage.getItem("sprechwerk-verwaltung-thema");
  } catch {
    gemerkt = null;
  }
  wendeThema(gemerkt ?? "dark");
  setSprache((navigator.language ?? "de").slice(0, 2));
  document.documentElement.lang = sprache.value;
}

// ---- Takt --------------------------------------------------------------

let zeitgeber: number[] = [];

export function starten() {
  stoppen();
  void standLaden().then(protokollLaden);
  zeitgeber = [
    window.setInterval(() => void standLaden(), TAKT_STAND),
    window.setInterval(() => void protokollLaden(), TAKT_PROTOKOLL),
  ];
}

export function stoppen() {
  for (const z of zeitgeber) window.clearInterval(z);
  zeitgeber = [];
}

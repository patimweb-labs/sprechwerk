// Ein Ort für den Zustand. Die Ansichten lesen hier und rufen Aktionen auf —
// keine Ansicht hält eigene Kopien der Wahrheit (AGENTS.md §2.1).
import { computed, reactive, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api, type Einstellungen, type Zustand } from "./api";

export const modell = reactive<Zustand>({
  verbindung: null,
  aktueller_raum: null,
  raeume: [],
  mitglieder: [],
  nachrichten: [],
  protokoll: [],
  meldung: null,
});

export const einstellungen = reactive<Einstellungen>({
  server: "127.0.0.1:7443",
  name: "",
  thema: "dark",
  sprache: "de",
  passwort: "",
});

export const laeuft = ref(false);

/** Verbunden? */
export const verbunden = computed(() => modell.verbindung?.art === "verbunden");

/** Eigene Rolle, falls verbunden. */
export const meineRolle = computed(() =>
  modell.verbindung?.art === "verbunden" ? modell.verbindung.rolle : null,
);

/** Darf ich Räume verwalten? Die Schaltfläche erscheint nur dann. */
export const darfRaeumeVerwalten = computed(() =>
  ["administrator", "moderator"].includes(meineRolle.value ?? ""),
);

/** Darf ich Rollen vergeben? */
export const darfRollenVergeben = computed(() => meineRolle.value === "administrator");

/** Darf ich das Prüfprotokoll lesen? */
export const darfProtokollLesen = computed(() =>
  ["administrator", "moderator"].includes(meineRolle.value ?? ""),
);

/** Raum, in dem ich sitze. */
export const meinRaum = computed(() => modell.raeume.find((r) => r.hier) ?? null);

/** Thems umschalten (nur dark, light, mono — AGENTS.md §2.7). */
export function wendeThema(thema: string) {
  const erlaubt = ["dark", "light", "mono"].includes(thema) ? thema : "dark";
  document.documentElement.dataset.theme = erlaubt;
  einstellungen.thema = erlaubt;
}

/** Lädt die Einstellungen vor dem ersten Bild. */
export async function ladeEinstellungen() {
  try {
    const geladen = await api.einstellungenLesen();
    Object.assign(einstellungen, geladen);
    if (!einstellungen.name) einstellungen.name = "";
  } catch {
    // Ohne Einstellungen gelten die Vorgaben.
  }
  wendeThema(einstellungen.thema);
}

/** Holt den aktuellen Stand vom Rust-Teil. */
export async function ladeZustand() {
  try {
    Object.assign(modell, await api.zustand());
  } catch (fehler) {
    modell.meldung = String(fehler);
  }
}

/** Abonniert die Meldungen des Rust-Teils. */
export async function verbindeEreignisse() {
  await listen<Zustand>("zustand", (ereignis) => {
    Object.assign(modell, ereignis.payload);
  });
  await ladeZustand();
}

export async function verbinden() {
  laeuft.value = true;
  modell.meldung = null;
  try {
    await api.verbinden(einstellungen.server, einstellungen.name, einstellungen.passwort);
    await api.einstellungenSpeichern({ ...einstellungen });
  } catch (fehler) {
    modell.meldung = String(fehler);
  } finally {
    laeuft.value = false;
  }
}

export async function trennen() {
  await api.trennen();
}

export async function betreten(raum: string) {
  await api.betreten(raum);
}

export async function verlassen() {
  await api.verlassen();
}

export async function sende(text: string) {
  const bereinigt = text.trim();
  if (!bereinigt) return;
  await api.textSenden(bereinigt);
}

export async function loesche(nachricht: string) {
  await api.nachrichtLoeschen(nachricht);
}

export async function legeRaumAn(name: string, eltern: string | null = null) {
  const bereinigt = name.trim();
  if (!bereinigt) return;
  await api.raumAnlegen(bereinigt, eltern);
}

export async function holeProtokoll() {
  await api.protokoll(0, 50);
}

/** Zeit als „14:07" für die Nachrichtenliste. */
export function uhrzeit(ms: number): string {
  return new Date(ms).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
}

// Ein Ort für den Zustand. Die Ansichten lesen hier und rufen Aktionen auf —
// keine Ansicht hält eigene Kopien der Wahrheit (AGENTS.md §2.1).
import { computed, reactive, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api, istDummy, type Einstellungen, type Zustand } from "./api";
import { abonnieren } from "./mock";
import { rolle, setSprache, sprache, t } from "./i18n";

export const modell = reactive<Zustand>({
  verbindung: null,
  aktueller_raum: null,
  raeume: [],
  mitglieder: [],
  nachrichten: [],

  meldung: null,
});

export const einstellungen = reactive<Einstellungen>({
  server: "127.0.0.1:7443",
  name: "",
  thema: "dark",
  sprache: "de",
  passwort: "",
  mikrofon: "",
  lautsprecher: "",
  push_to_talk: false,
  ptt_taste: "Leertaste",
  schwelle: 30,
  vorlauf_ms: 60,
  favoriten: [],
  kompakt: false,
  rauschunterdrueckung: true,
  noisegate: true,
  echounterdrueckung: true,
});

/** Welche Ansicht ist offen? (Die Leiste links schreibt hier hinein.) */
// Startseite ist die Serverliste; im Klickdummy ist man schon verbunden und
// landet direkt im Gespräch.
export const ansicht = ref<"server" | "raeume" | "einstellungen">(istDummy ? "raeume" : "server");

export const laeuft = ref(false);

/** Mikrofonpegel 0–100 (eigenes Gerät, kommt 10 × je Sekunde). */
export const pegel = ref(0);

/** Läuft der Sprechpfad (Mikrofon und Wiedergabe offen)? */
export const hoerbar = ref(false);

/** Eigenes Mikrofon lokal stumm (wirkt sofort, ohne Serverrunde). */
export const lokalStumm = ref(false);

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

/** Raum, in dem ich sitze. */
export const meinRaum = computed(() => modell.raeume.find((r) => r.hier) ?? null);

/** Thems umschalten (nur dark, light, mono — AGENTS.md §2.7). */
export function wendeThema(thema: string) {
  const erlaubt = ["dark", "light", "mono"].includes(thema) ? thema : "dark";
  document.documentElement.dataset.theme = erlaubt;
  einstellungen.thema = erlaubt;
}

/** Setzt die Sprache und speichert sie. */
export async function waehleSprache(neu: string) {
  setSprache(neu);
  einstellungen.sprache = sprache.value;
  await speichere();
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
  setSprache(einstellungen.sprache);
}

/** Holt den aktuellen Stand vom Rust-Teil. */
export async function ladeZustand() {
  try {
    Object.assign(modell, await api.zustand());
  } catch (fehler) {
    modell.meldung = `Zustand nicht lesbar: ${String(fehler)}`;
  }
}

/** Abonniert die Meldungen des Rust-Teils. */
export async function verbindeEreignisse() {
  // Reihenfolge zählt: erst der Zustand (damit das Fenster etwas anzeigt),
  // dann die Ereignisse. Schlägt das Abonnieren fehl, muss das sichtbar
  // werden — eine stumme Ablehnung sieht aus wie ein leerer Bildschirm.
  await ladeZustand();
  try {
    if (istDummy) {
      // Klickdummy: der Mock meldet Änderungen selbst.
      abonnieren((stand) => Object.assign(modell, stand));
      return;
    }
    await listen<Zustand>("zustand", (ereignis) => {
      Object.assign(modell, ereignis.payload);
    });
    // Pegel ist ein eigener Strom: er ändert sich ständig und würde den
    // Modellstand fluten.
    await listen<number>("pegel", (ereignis) => {
      pegel.value = ereignis.payload;
    });
  } catch (fehler) {
    modell.meldung = `Ereignisse nicht abonnierbar: ${String(fehler)}`;
  }
}

/** Speichert die Einstellungen (ohne Verbindung). */
export async function speichere() {
  try {
    await api.einstellungenSpeichern({ ...einstellungen });
    // Ton-Einstellungen wirken sofort, nicht erst beim nächsten Verbinden.
    await wendeAudioEinstellungen();
  } catch (fehler) {
    modell.meldung = `Einstellungen nicht speicherbar: ${String(fehler)}`;
  }
}

/** Bereich wechseln (Reiter in der Kopfzeile). */
export function geheZu(bereich: "server" | "raeume" | "einstellungen") {
  ansicht.value = bereich;
}

/** Eigenes Mikrofon stumm? (Der Server führt den Zustand.) */
export const eigeneStumm = computed(() => {
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  return modell.mitglieder.find((m) => m.id === id)?.stumm ?? false;
});

/** Eigener Ton stumm? */
export const eigeneTaub = computed(() => {
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  return modell.mitglieder.find((m) => m.id === id)?.taub ?? false;
});

/**
 * Eigenes Mikrofon stummschalten.
 *
 * Zwei Dinge zugleich: der **Sprechpfad** hört sofort auf zu senden (ohne
 * Serverrunde, also ohne Verzögerung), und der **Server** führt den Zustand,
 * damit die anderen das Symbol sehen.
 */
export async function eigenesMikrofon(an: boolean) {
  lokalStumm.value = an;
  if (!istDummy) await api.mikrofonStumm(an).catch(() => undefined);
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  if (id) await api.stummschalten(id, false, an).catch(() => undefined);
}

/** Eigenen Ton abschalten (taub): nichts wird abgespielt. */
export async function eigenesHoeren(an: boolean) {
  if (!istDummy) await api.hoerenAus(an).catch(() => undefined);
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  if (id) await api.stummschalten(id, true, an).catch(() => undefined);
}

/** Push-to-Talk: Taste gedrückt oder losgelassen. */
export async function sprechtaste(an: boolean) {
  if (!verbunden.value || !einstellungen.push_to_talk || istDummy) return;
  await api.sprechtaste(an).catch(() => undefined);
}

/** Stand des Sprechpfads holen (nach dem Verbinden, nach Gerätewechsel). */
export async function holeAudioStand() {
  if (istDummy) {
    hoerbar.value = verbunden.value;
    pegel.value = 0;
    return;
  }
  try {
    const stand = await api.audioStand();
    hoerbar.value = stand.laeuft;
    pegel.value = stand.pegel;
    lokalStumm.value = stand.stumm;
  } catch {
    hoerbar.value = false;
  }
}

/** Wendet geänderte Audioeinstellungen an (Schwelle, Gate, Geräte). */
export async function wendeAudioEinstellungen() {
  if (istDummy) return;
  try {
    await api.audioEinstellen({ ...einstellungen });
    await holeAudioStand();
  } catch (fehler) {
    modell.meldung = `Ton: ${String(fehler)}`;
  }
}

/** Verbindet zu einem gemerkten Server (aus der Serverliste). */
export async function verbindeMit(server: string, name?: string) {
  if (server) einstellungen.server = server;
  if (name) einstellungen.name = name;
  ansicht.value = "raeume";
  await verbinden();
}

/** Nimmt einen Server in die Favoriten auf. */
export async function favoritHinzufuegen(favorit: { name: string; server: string }) {
  const vorhanden = einstellungen.favoriten.find((f) => f.server === favorit.server);
  if (vorhanden) {
    vorhanden.name = favorit.name;
  } else {
    einstellungen.favoriten.push(favorit);
  }
  await speichere();
}

/** Entfernt einen Favoriten. */
export async function favoritEntfernen(server: string) {
  einstellungen.favoriten = einstellungen.favoriten.filter((f) => f.server !== server);
  await speichere();
}

export async function verbinden() {
  laeuft.value = true;
  modell.meldung = null;
  try {
    await api.verbinden(einstellungen.server, einstellungen.name, einstellungen.passwort);
    await api.einstellungenSpeichern({ ...einstellungen });
    await holeAudioStand();
  } catch (fehler) {
    modell.meldung = String(fehler);
  } finally {
    laeuft.value = false;
  }
}

export async function trennen() {
  await api.trennen();
  if (!istDummy) await api.audioStoppen().catch(() => undefined);
  hoerbar.value = false;
  pegel.value = 0;
  // Nach dem Trennen gehört der Blick der Serverliste.
  ansicht.value = "server";
}

export async function betreten(raum: string) {
  await api.betreten(raum);
}

export async function sende(text: string) {
  const bereinigt = text.trim();
  if (!bereinigt) return;
  await api.textSenden(bereinigt);
}

export async function setzeRolle(mitglied: string, rolle: string) {
  await api.rolleSetzen(mitglied, rolle as never);
}

/** Darf ich in Räume und Personen eingreifen? (Sichtbarkeit folgt aus dem Modell) */
export const darfEingreifen = computed(() =>
  ["administrator", "moderator"].includes(meineRolle.value ?? ""),
);

/** Server-Stummschaltung (Mikrofon oder Ton). */
export async function stummschalten(mitglied: string, taub: boolean, an: boolean) {
  await api.stummschalten(mitglied, taub, an);
}

export async function verschieben(mitglied: string, raum: string) {
  await api.verschieben(mitglied, raum);
}

/** Ende-zu-Ende-Verschlüsselung eines Raums umschalten. */
export async function verschluesseln(raum: string, an: boolean) {
  await api.raumVerschluesseln(raum, an);
}

export async function ausRaumWerfen(mitglied: string) {
  await api.ausRaumWerfen(mitglied);
}

export async function trenneMitglied(mitglied: string, grund: string) {
  await api.trenneMitglied(mitglied, grund);
}

export async function sperreMitglied(mitglied: string, grund: string) {
  await api.sperreMitglied(mitglied, grund);
}

/** Merkt sich das Mitglied, dessen Rolle geändert werden soll (Dialog im Fenster). */
let rollenZiel: string | null = null;
export function eingreifen(mitglied: string) {
  rollenZiel = mitglied;
  ansicht.value = "einstellungen";
}
export function rollenZielLesen() {
  return rollenZiel;
}

export async function raumUmbenennen(raum: string, name: string) {
  await api.raumUmbenennen(raum, name);
}

/** „3 anwesend · 12 Nachrichten" für die Kopfzeile des Raums. */
export function anwesenheitText(): string {
  const raum = modell.raeume.find((r) => r.hier);
  if (!raum) return "";
  const n = modell.mitglieder.filter((m) => m.raum === raum.id).length;
  return `${n} anwesend · ${modell.nachrichten.length} Nachrichten`;
}

export async function loesche(nachricht: string) {
  await api.nachrichtLoeschen(nachricht);
}

export async function legeRaumAn(name: string) {
  const bereinigt = name.trim();
  if (!bereinigt) return;
  await api.raumAnlegen(bereinigt);
}

/** Kurzform für Texte in den Ansichten. */
export { rolle, sprache, t };

/** Zeit als „14:07" für die Nachrichtenliste. */
export function uhrzeit(ms: number): string {
  return new Date(ms).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
}

// Ein Ort für den Zustand. Die Ansichten lesen hier und rufen Aktionen auf —
// keine Ansicht hält eigene Kopien der Wahrheit (AGENTS.md §2.1).
import { computed, reactive, ref, watch } from "vue";
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
  latenz_ms: null,
  schluessel: [],
  guete: null,
  sperren: [],
  protokoll: [],
  // Erst nach dem Verbinden gesetzt: der Wert, der geprüft wurde.
  zertifikat: null,

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
  chat_eingeklappt: false,
  rauschunterdrueckung: true,
  noisegate: true,
  echounterdrueckung: true,
  vertrauen: [],
});

/** Welche Ansicht ist offen? (Die Leiste links schreibt hier hinein.) */
// Startseite ist die Serverliste; im Klickdummy ist man schon verbunden und
// landet direkt im Gespräch.
export const ansicht = ref<
  "server" | "raeume" | "protokoll" | "sperren" | "einstellungen"
>(
  istDummy ? "raeume" : "server",
);

export const laeuft = ref(false);

/** Mikrofonpegel 0–100 (eigenes Gerät, kommt 10 × je Sekunde). */
export const pegel = ref(0);

/** Läuft der Sprechpfad (Mikrofon und Wiedergabe offen)? */
const hoerbar = ref(false);

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
async function ladeZustand() {
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

/**
 * Meldet, wenn die Verschlüsselung des **eigenen** Raums umschaltet.
 *
 * Das Schloss in der Raumliste ist klein und leicht zu übersehen. Wer gerade
 * spricht, soll erfahren, dass ab jetzt Ende zu Ende verschlüsselt wird (oder
 * nicht mehr) — sonst ändert sich das Verhalten, ohne dass es jemand merkt.
 */
export function beobachteVerschluesselung() {
  watch(
    () => modell.raeume.find((r) => r.hier)?.verschluesselt ?? null,
    (neu, alt) => {
      // `null` heißt: noch kein Raum bekannt (erster Aufbau).
      if (neu === null || alt === null || neu === alt) return;
      modell.meldung = neu ? t("room.encrypted_now") : t("room.unencrypted_now");
    },
  );
}

/** Führt einen Befehl aus und macht einen Fehler sichtbar.
 *
 * Ohne das bliebe eine abgelehnte Anfrage (kein Recht, zu langer Text, Server
 * weg) unsichtbar: das Versprechen endete als unbehandelter Fehler, und die
 * Oberfläche zeigte weiter den alten Stand. Jeder Befehl geht deshalb hier
 * durch — ein Ort, eine Meldung.
 */
async function befehl<T>(ruf: () => Promise<T>): Promise<T | null> {
  try {
    const ergebnis = await ruf();
    if (modell.meldung) modell.meldung = null;
    return ergebnis;
  } catch (fehler) {
    modell.meldung = String(fehler);
    return null;
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

/**
 * Umlaufzeit zum Server messen.
 *
 * Die Zahl kommt aus dem Rust-Teil (dort sitzt die Uhr am selben Faden wie die
 * Verbindung) und landet als `latenz_ms` im Modell.
 */
export async function messeLatenz() {
  if (!verbunden.value) return;
  try {
    await api.ping();
  } catch {
    // Beiwerk — kein Grund, den Nutzer zu behelligen.
  }
}

/**
 * Kurztext für die Statuszeile: „8 ms" oder „1,2 s".
 *
 * Unter einer Sekunde in Millisekunden (das ist die Zahl, die man bei Sprache
 * vergleicht), darüber in Sekunden — „1240 ms" liest niemand gern.
 */
export const latenzText = computed(() => {
  const ms = modell.latenz_ms;
  if (ms === null || ms === undefined) return null;
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(1).replace(".", ",")} s`;
});

/**
 * Hat der Client den Schlüssel dieses Raums?
 *
 * `null` heißt: der Raum ist nicht verschlüsselt (oder ich bin nicht drin) —
 * dann ist die Frage sinnlos.
 */
export function schluesselVorhanden(raum: string): boolean | null {
  const r = modell.raeume.find((x) => x.id === raum);
  if (!r || !r.verschluesselt) return null;
  return modell.schluessel.some((s) => s.raum === raum);
}

/** Den Raumschlüssel erneut anfordern (nach einem verlorenen Paket). */
export async function schluesselAnfordern(raum: string) {
  try {
    await api.schluesselAnfordern(raum);
  } catch (fehler) {
    modell.meldung = `Schlüssel nicht angefordert: ${String(fehler)}`;
  }
}

/** Sperrliste holen (nur Administrator). */
export async function ladeSperrliste() {
  if (!darfSperren.value) return;
  try {
    await api.sperrliste();
  } catch (fehler) {
    modell.meldung = `Sperrliste nicht lesbar: ${String(fehler)}`;
  }
}

/** Darf ich Sperren verhängen und aufheben? (nur Administrator) */
export const darfSperren = computed(() => meineRolle.value === "administrator");

/** Betriebszahlen holen (Statuszeile, alle 10 s). */
export async function holeZaehler() {
  if (!verbunden.value) return;
  try {
    await api.zaehler();
  } catch {
    // Kein Grund, den Nutzer zu behelligen: die Zahlen sind Beiwerk.
  }
}

/**
 * Text für die Statuszeile: „keine Rahmen verloren" oder „3 Rahmen verloren".
 *
 * **0** ist die gute Nachricht und wird auch so gesagt. TeamSpeak zeigt an
 * dieser Stelle ein Symbol ohne Zahl — hier steht die Zahl.
 */
export const qualitaet = computed<{ text: string; tone: "ok" | "warn" } | null>(() => {
  const g = modell.guete;
  if (!g) return null;
  return g.verloren === 0
    ? { text: t("app.quality_ok"), tone: "ok" }
    : { text: t("app.quality_loss", g.verloren), tone: "warn" };
});

/** Darf ich das Prüfprotokoll lesen? (Administrator und Moderator) */
export const darfProtokollLesen = computed(() => {
  const r = meineRolle.value;
  return r === "administrator" || r === "moderator";
});

/**
 * Prüfprotokoll laden.
 *
 * Die Antwort kommt als Rahmen zurück und landet über `abbilden` im Modell —
 * hier wird nur angestoßen, nicht zwischengespeichert.
 */
export async function ladeProtokoll(ab = 0, anzahl = 50) {
  try {
    await api.protokoll(ab, anzahl);
  } catch (fehler) {
    modell.meldung = `Protokoll nicht lesbar: ${String(fehler)}`;
  }
}

/** Eine Sperre aufheben. */
export async function entsperren(kennung: string | null, adresse: string | null) {
  try {
    await api.entsperren(kennung, adresse);
  } catch (fehler) {
    modell.meldung = `Sperre nicht aufgehoben: ${String(fehler)}`;
  }
}

/** Bereich wechseln (Reiter in der Kopfzeile). */
export function geheZu(
  bereich: "server" | "raeume" | "protokoll" | "sperren" | "einstellungen",
) {
  ansicht.value = bereich;
}

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
  // Der Sprechpfad wirkt sofort und lokal; ein Fehler dort darf den Schalter
  // nicht blockieren (Meldung kommt vom Serverweg darunter).
  if (!istDummy) await api.mikrofonStumm(an).catch(() => undefined);
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  if (id) await befehl(() => api.stummschalten(id, false, an));
}

/** Eigenen Ton abschalten (taub): nichts wird abgespielt. */
export async function eigenesHoeren(an: boolean) {
  if (!istDummy) await api.hoerenAus(an).catch(() => undefined);
  const id = modell.verbindung?.art === "verbunden" ? modell.verbindung.ich : null;
  if (id) await befehl(() => api.stummschalten(id, true, an));
}

/** Push-to-Talk: Taste gedrückt oder losgelassen. */
export async function sprechtaste(an: boolean) {
  if (!verbunden.value || !einstellungen.push_to_talk || istDummy) return;
  await befehl(() => api.sprechtaste(an));
}

/** Stand des Sprechpfads holen (nach dem Verbinden, nach Gerätewechsel). */
async function holeAudioStand() {
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
async function wendeAudioEinstellungen() {
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
    // Der Rust-Teil hat beim Verbinden gerade das Zertifikat angeheftet. Die
    // Einstellungen deshalb **neu lesen**, bevor gespeichert wird: sonst
    // überschreibt der Stand von vor dem Verbinden den frisch angehefteten
    // Fingerabdruck — und die Anheftung wäre nach jeder Verbindung wieder weg.
    const frisch = await api.einstellungenLesen();
    einstellungen.vertrauen = frisch.vertrauen;
    await api.einstellungenSpeichern({ ...einstellungen });
    await holeAudioStand();
  } catch (fehler) {
    modell.meldung = String(fehler);
  } finally {
    laeuft.value = false;
  }
}

export async function trennen() {
  await befehl(() => api.trennen());
  if (!istDummy) await api.audioStoppen().catch(() => undefined);
  hoerbar.value = false;
  pegel.value = 0;
  // Nach dem Trennen gehört der Blick der Serverliste.
  ansicht.value = "server";
}

export async function betreten(raum: string) {
  await befehl(() => api.betreten(raum));
}

export async function sende(text: string) {
  const bereinigt = text.trim();
  if (!bereinigt) return;
  await befehl(() => api.textSenden(bereinigt));
}

export async function setzeRolle(mitglied: string, rolle: string) {
  await befehl(() => api.rolleSetzen(mitglied, rolle as never));
}

/** Darf ich in Räume und Personen eingreifen? (Sichtbarkeit folgt aus dem Modell) */
export const darfEingreifen = computed(() =>
  ["administrator", "moderator"].includes(meineRolle.value ?? ""),
);

/** Server-Stummschaltung (Mikrofon oder Ton). */
export async function stummschalten(mitglied: string, taub: boolean, an: boolean) {
  await befehl(() => api.stummschalten(mitglied, taub, an));
}

export async function verschieben(mitglied: string, raum: string) {
  await befehl(() => api.verschieben(mitglied, raum));
}

/** Ende-zu-Ende-Verschlüsselung eines Raums umschalten. */
export async function verschluesseln(raum: string, an: boolean) {
  await befehl(() => api.raumVerschluesseln(raum, an));
}

export async function ausRaumWerfen(mitglied: string) {
  await befehl(() => api.ausRaumWerfen(mitglied));
}

/** Vergisst den angehefteten Fingerabdruck eines Servers.
 *
 * Nötig, wenn der Server neu aufgesetzt wurde (neues Zertifikat): der nächste
 * Verbindungsversuch heftet dann den neuen Wert an. Bis dahin wird die
 * Verbindung mit dem alten Wert abgelehnt — das ist gewollt.
 */
export async function zertifikatVergessen(server: string) {
  await befehl(async () => {
    await api.zertifikatVergessen(server);
    einstellungen.vertrauen = einstellungen.vertrauen.filter((v) => v.server !== server);
    modell.meldung = t("server.certificate_forgotten");
  });
}

export async function trenneMitglied(mitglied: string, grund: string) {
  await befehl(() => api.trenneMitglied(mitglied, grund));
}

export async function sperreMitglied(mitglied: string, grund: string) {
  await befehl(() => api.sperreMitglied(mitglied, grund));
}

export async function raumUmbenennen(raum: string, name: string) {
  await befehl(() => api.raumUmbenennen(raum, name));
}

export async function loesche(nachricht: string) {
  await befehl(() => api.nachrichtLoeschen(nachricht));
}

export async function legeRaumAn(name: string) {
  const bereinigt = name.trim();
  if (!bereinigt) return;
  await befehl(() => api.raumAnlegen(bereinigt));
}

/** Kurzform für Texte in den Ansichten. */
export { rolle, sprache, t };

/** Zeit als „14:07" für die Nachrichtenliste (in der gewählten Sprache). */
export function uhrzeit(ms: number): string {
  const spracheDerUhr =
    sprache.value === "de" ? "de-DE" : sprache.value === "en" ? "en-GB" : sprache.value === "es" ? "es-ES" : "fr-FR";
  return new Date(ms).toLocaleTimeString(spracheDerUhr, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

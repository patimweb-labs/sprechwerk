// Der Vertrag zum Rust-Teil. Jeder Befehl hier hat ein Gegenstück in
// crates/desktop/src/commands.rs, jeder Typ eines in crates/ipc/src/lib.rs.
//
// Läuft die Oberfläche **nicht** in der Fensteranwendung (kein Tauri), wird
// automatisch der Klickdummy aus `mock.ts` benutzt. Damit lässt sich das
// Aussehen ohne Server, ohne Soundkarte und ohne Verbindung beurteilen.
import { invoke } from "@tauri-apps/api/core";
import { mockApi } from "./mock";

export type Rolle = "administrator" | "moderator" | "mitglied" | "gast";

export type Verbindung =
  | { art: "getrennt" }
  | { art: "verbindet"; server: string }
  | {
      art: "verbunden";
      server: string;
      server_name: string;
      ich: string;
      name: string;
      rolle: Rolle;
      /** Eigener öffentlicher Schlüssel (Hex) — dieselbe Person auf jedem Gerät. */
      kennung?: string;
    }
  | { art: "fehler"; text: string };

export interface Raum {
  id: string;
  name: string;
  anwesend: number;
  hier: boolean;
  /** Sprache Ende zu Ende verschlüsselt. */
  verschluesselt: boolean;
}

export interface Mitglied {
  id: string;
  name: string;
  rolle: Rolle;
  raum: string | null;
  spricht: boolean;
  stumm: boolean;
  taub: boolean;
  ich: boolean;
}

export interface Nachricht {
  id: string;
  verfasser: string;
  verfasser_name: string;
  text: string;
  zeit: number;
  geloescht: boolean;
  loeschbar: boolean;
}

/** Der Raumschlüssel eines verschlüsselten Raums. */
export interface Schluesselstand {
  raum: string;
  /** Epoche des Schlüssels (zählt bei jedem Verlassen hoch). */
  epoche: number;
}

/** Eine verhängte Sperre. */
export interface Sperre {
  kennung: string | null;
  adresse: string | null;
  grund: string;
  /** Zeitpunkt in Millisekunden seit der Epoche. */
  zeit: number;
  handelnder_name: string;
}

/** Betriebszahlen des Servers — kein Personenbezug. */
export interface Guete {
  sprachrahmen: number;
  verteilt: number;
  verloren: number;
  abgelehnt: number;
  verbundene: number;
}

/** Eine Zeile des Prüfprotokolls. */
export interface Eintrag {
  laufnummer: number;
  /** Zeitpunkt in Millisekunden seit der Epoche. */
  zeit: number;
  handelnder_name: string;
  /** Kurzbezeichnung, etwa `raum_angelegt`. */
  aktion: string;
  ziel: string;
  vorher: string | null;
  nachher: string | null;
  /** „erlaubt" oder „abgelehnt: …". */
  ergebnis: string;
  /** Begründung der Handlung (Sperre, Trennung), falls es eine gibt. */
  grund?: string | null;
}

/** Der vollständige Modellstand — die Oberfläche ist `render(modell)`. */
export interface Zustand {
  verbindung: Verbindung | null;
  aktueller_raum: string | null;
  raeume: Raum[];
  mitglieder: Mitglied[];
  nachrichten: Nachricht[];
  /** Verhängte Sperren (leer, bis sie einmal abgefragt wurden). */
  sperren: Sperre[];
  /** Räume, deren Schlüssel vorliegt (nur verschlüsselte Räume). */
  schluessel: Schluesselstand[];
  /** Umlaufzeit zum Server in Millisekunden (aus Ping/Pong). */
  latenz_ms: number | null;
  /** Betriebszahlen, bis sie einmal abgefragt wurden: null. */
  guete: Guete | null;
  /** Zuletzt gelesenes Prüfprotokoll (leer, solange niemand es geöffnet hat). */
  protokoll: Eintrag[];
  meldung: string | null;
}

/** Ein gemerkter Server (Favorit). */
export interface Favorit {
  name: string;
  server: string;
}

export interface Einstellungen {
  server: string;
  name: string;
  thema: string;
  sprache: string;
  passwort: string;
  mikrofon: string;
  lautsprecher: string;
  push_to_talk: boolean;
  ptt_taste: string;
  schwelle: number;
  vorlauf_ms: number;
  favoriten: Favorit[];
  /**
   * Chat eingeklappt (Schiene rechts). Ein Zustand, keine Vorliebe — deshalb
   * nicht in den Einstellungen sichtbar, aber gemerkt.
   */
  chat_eingeklappt: boolean;
  /** Rauschunterdrückung im Aufnahmepfad. */
  rauschunterdrueckung: boolean;
  /** Noisegate: unterhalb der Schwelle wird nichts gesendet. */
  noisegate: boolean;
  /** Echounterdrückung: eigene Wiedergabe aus der Aufnahme rechnen. */
  echounterdrueckung: boolean;
}

/** Läuft die Oberfläche in der Fensteranwendung (echter Rust-Teil)? */
export const inFenster =
  typeof window !== "undefined" &&
  ("__TAURI_INTERNALS__" in window || (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== undefined);

/** Klickdummy statt echtem Server.
 *
 * Drei Wege, absichtlich: im Browser gibt es kein Tauri (also Dummy), beim
 * Bauen mit `VITE_MOCK=1` wird der Dummy eingebettet, und `?mock=1` schaltet
 * ihn auch in der Fensteranwendung ein. */
export const istDummy =
  import.meta.env.VITE_MOCK === "1" ||
  !inFenster ||
  (typeof location !== "undefined" && location.search.includes("mock=1"));

const echtApi = {
  zustand: () => invoke<Zustand>("zustand"),
  verbinden: (server: string, name: string, passwort?: string) =>
    invoke<string>("verbinden", { server, name, passwort: passwort || null }),
  trennen: () => invoke<void>("trennen"),
  betreten: (raum: string) => invoke<void>("betreten", { raum }),
  textSenden: (text: string) => invoke<void>("text_senden", { text }),
  nachrichtLoeschen: (nachricht: string) => invoke<void>("nachricht_loeschen", { nachricht }),
  raumAnlegen: (name: string) => invoke<void>("raum_anlegen", { name }),
  raumUmbenennen: (raum: string, name: string) => invoke<void>("raum_umbenennen", { raum, name }),
  raumLoeschen: (raum: string) => invoke<void>("raum_loeschen", { raum }),
  rolleSetzen: (mitglied: string, rolle: Rolle) => invoke<void>("rolle_setzen", { mitglied, rolle }),
  stummschalten: (mitglied: string, taub: boolean, an: boolean) =>
    invoke<void>("stummschalten", { mitglied, taub, an }),
  verschieben: (mitglied: string, raum: string) => invoke<void>("verschieben", { mitglied, raum }),
  ausRaumWerfen: (mitglied: string) => invoke<void>("aus_raum_werfen", { mitglied }),
  trenneMitglied: (mitglied: string, grund: string) =>
    invoke<void>("trennen_mitglied", { mitglied, grund }),
  sperreMitglied: (mitglied: string, grund: string) => invoke<void>("sperren", { mitglied, grund }),
  entsperren: (kennung: string | null, adresse: string | null) =>
    invoke<void>("entsperren", { kennung, adresse }),
  protokoll: (ab = 0, anzahl = 50) => invoke<void>("protokoll", { ab, anzahl }),
  zaehler: () => invoke<void>("zaehler"),
  sperrliste: () => invoke<void>("sperrliste"),
  ping: () => invoke<void>("ping"),
  schluesselAnfordern: (raum: string) => invoke<void>("schluessel_anfordern", { raum }),
  raumVerschluesseln: (raum: string, an: boolean) =>
    invoke<void>("raum_verschluesseln", { raum, an }),
  audioGeraete: () => invoke<Geraeteliste>("audio_geraete"),
  audioPegel: (geraet: string, millisekunden: number) =>
    invoke<number>("audio_pegel", { geraet, millisekunden }),
  audioTestton: (geraet: string) => invoke<void>("audio_testton", { geraet }),
  audioStarten: () => invoke<void>("audio_starten"),
  audioStoppen: () => invoke<void>("audio_stoppen"),
  audioStand: () => invoke<AudioStand>("audio_stand"),
  audioEinstellen: (einstellungen: Einstellungen) =>
    invoke<void>("audio_einstellen", { einstellungen }),
  sprechtaste: (an: boolean) => invoke<void>("sprechtaste", { an }),
  mikrofonStumm: (an: boolean) => invoke<void>("mikrofon_stumm", { an }),
  hoerenAus: (an: boolean) => invoke<void>("hoeren_aus", { an }),
  einstellungenLesen: () => invoke<Einstellungen>("einstellungen_lesen"),
  einstellungenSpeichern: (einstellungen: Einstellungen) =>
    invoke<void>("einstellungen_speichern", { einstellungen }),
};

/** Stand des Sprechpfads (Mikrofon/Wiedergabe laufen). */
export interface AudioStand {
  laeuft: boolean;
  pegel: number;
  stumm: boolean;
  taub: boolean;
}

/** Ein wählbares Audiogerät. */
export interface Geraet {
  id: string;
  name: string;
  vorgabe: boolean;
}

export interface Geraeteliste {
  mikrofon: Geraet[];
  lautsprecher: Geraet[];
}


export const api = istDummy ? (mockApi as unknown as typeof echtApi) : echtApi;

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
  | { art: "verbunden"; server: string; server_name: string; ich: string; name: string; rolle: Rolle }
  | { art: "fehler"; text: string };

export interface Raum {
  id: string;
  name: string;
  anwesend: number;
  hier: boolean;
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

/** Der vollständige Modellstand — die Oberfläche ist `render(modell)`. */
export interface Zustand {
  verbindung: Verbindung | null;
  aktueller_raum: string | null;
  raeume: Raum[];
  mitglieder: Mitglied[];
  nachrichten: Nachricht[];
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
  einstellungenLesen: () => invoke<Einstellungen>("einstellungen_lesen"),
  einstellungenSpeichern: (einstellungen: Einstellungen) =>
    invoke<void>("einstellungen_speichern", { einstellungen }),
};

export const api = istDummy ? (mockApi as unknown as typeof echtApi) : echtApi;

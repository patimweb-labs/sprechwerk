// Der Vertrag zum Rust-Teil. Jeder Befehl hier hat ein Gegenstück in
// crates/desktop/src/commands.rs, jeder Typ eines in crates/ipc/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";

export type Rolle = "administrator" | "moderator" | "mitglied" | "gast";

export type Verbindung =
  | { art: "getrennt" }
  | { art: "verbindet"; server: string }
  | { art: "verbunden"; server: string; server_name: string; ich: string; name: string; rolle: Rolle }
  | { art: "fehler"; text: string };

export interface Raum {
  id: string;
  name: string;
  eltern: string | null;
  tiefe: number;
  anwesend: number;
  hier: boolean;
}

export interface Mitglied {
  id: string;
  name: string;
  rolle: Rolle;
  raum: string | null;
  spricht: boolean;
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

export interface Protokolleintrag {
  laufnummer: number;
  zeit: number;
  handelnder_name: string;
  aktion: string;
  ziel: string;
  ergebnis: string;
}

/** Der vollständige Modellstand — die Oberfläche ist `render(modell)`. */
export interface Zustand {
  verbindung: Verbindung | null;
  aktueller_raum: string | null;
  raeume: Raum[];
  mitglieder: Mitglied[];
  nachrichten: Nachricht[];
  protokoll: Protokolleintrag[];
  meldung: string | null;
}

export interface Einstellungen {
  server: string;
  name: string;
  thema: string;
  sprache: string;
  passwort: string;
}

export const api = {
  zustand: () => invoke<Zustand>("zustand"),
  verbinden: (server: string, name: string, passwort?: string) =>
    invoke<string>("verbinden", { server, name, passwort: passwort || null }),
  trennen: () => invoke<void>("trennen"),
  betreten: (raum: string) => invoke<void>("betreten", { raum }),
  verlassen: () => invoke<void>("verlassen"),
  textSenden: (text: string) => invoke<void>("text_senden", { text }),
  nachrichtLoeschen: (nachricht: string) => invoke<void>("nachricht_loeschen", { nachricht }),
  raumAnlegen: (name: string, eltern?: string | null) =>
    invoke<void>("raum_anlegen", { name, eltern: eltern ?? null }),
  raumUmbenennen: (raum: string, name: string) => invoke<void>("raum_umbenennen", { raum, name }),
  raumLoeschen: (raum: string) => invoke<void>("raum_loeschen", { raum }),
  rolleSetzen: (mitglied: string, rolle: Rolle) => invoke<void>("rolle_setzen", { mitglied, rolle }),
  protokoll: (ab: number, anzahl: number) => invoke<void>("protokoll", { ab, anzahl }),
  einstellungenLesen: () => invoke<Einstellungen>("einstellungen_lesen"),
  einstellungenSpeichern: (einstellungen: Einstellungen) =>
    invoke<void>("einstellungen_speichern", { einstellungen }),
};

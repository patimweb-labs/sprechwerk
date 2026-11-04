// Klickdummy: ein vollständiger Modellstand ohne Server.
//
// Zweck: das Aussehen und die Abläufe beurteilen, bevor Server und Audiopfad
// dahinter stehen. Er wird **automatisch** benutzt, wenn die Oberfläche nicht
// in der Fensteranwendung läuft (kein `__TAURI_INTERNALS__`) oder wenn die
// Adresse `?mock=1` trägt.
//
// Die Daten sind ein Verein: sieben Räume im Baum, vier Mitglieder mit
// unterschiedlichen Rollen, Nachrichten und ein Prüfprotokoll. Jede Aktion
// verändert den Stand und meldet ihn — genau wie der echte Rust-Teil.

import type { Mitglied, Nachricht, Raum, Zustand, Einstellungen, Rolle } from "./api";

const jetzt = Date.now();
const minut = 60_000;

let folge = 1;
const neueId = (vorsilbe: string) => `${vorsilbe}_${(folge++).toString(16).padStart(32, "0")}`;

function raum(id: string, name: string, tiefe: number, eltern: string | null, anwesend = 0, hier = false): Raum {
  return { id, name, eltern, tiefe, anwesend, hier };
}

function mitglied(
  id: string,
  name: string,
  rolle: Rolle,
  raum: string | null,
  spricht = false,
  ich = false,
  stumm = false,
  taub = false,
): Mitglied {
  return { id, name, rolle, raum, spricht, stumm, taub, ich };
}

function nachricht(id: string, verfasser: string, verfasserName: string, text: string, vorMinuten: number, geloescht = false): Nachricht {
  return {
    id,
    verfasser,
    verfasser_name: verfasserName,
    text,
    zeit: jetzt - vorMinuten * minut,
    geloescht,
    loeschbar: false,
  };
}

const ICH = "mitglied_aaaa0000000000000000000000000001";

const raeume: Raum[] = [
  raum("raum_0001", "Empfang", 0, null, 1),
  raum("raum_0002", "Vorstand", 0, null, 1),
  raum("raum_0003", "Sitzung", 1, "raum_0002"),
  raum("raum_0004", "Trainingsraum", 0, null, 2, true),
  raum("raum_0005", "Gruppe A", 1, "raum_0004"),
  raum("raum_0006", "Technik", 0, null),
  raum("raum_0007", "Off-Topic", 0, null, 1),
];

const mitglieder: Mitglied[] = [
  mitglied(ICH, "Ada", "administrator", "raum_0004", false, true),
  mitglied("mitglied_aaaa0000000000000000000000000002", "Bosse", "moderator", "raum_0002"),
  mitglied("mitglied_aaaa0000000000000000000000000003", "Mira", "mitglied", "raum_0004", true),
  mitglied("mitglied_aaaa0000000000000000000000000004", "Timo", "gast", "raum_0001", false, false, true),
  mitglied("mitglied_aaaa0000000000000000000000000006", "Jonas", "mitglied", "raum_0004", false, false, false, true),
  mitglied("mitglied_aaaa0000000000000000000000000005", "Katrin", "mitglied", "raum_0007"),
];

const nachrichten: Record<string, Nachricht[]> = {
  raum_0004: [
    nachricht("nachricht_0001", ICH, "Ada", "Willkommen im Trainingsraum. Heute: Aufstellung und Ansage.", 42),
    nachricht("nachricht_0002", "mitglied_aaaa0000000000000000000000000002", "Bosse", "Ich bringe die Folien mit, Folie 7 ist neu.", 38),
    nachricht("nachricht_0003", "mitglied_aaaa0000000000000000000000000003", "Mira", "Kann jemand den Beamer testen? Letztes Mal war der Ton weg.", 31),
    nachricht("nachricht_0004", ICH, "Ada", "Erledigt — und der Rückkanal ist jetzt auf Kanal 3 statt 5.", 22),
    nachricht("nachricht_0005", "mitglied_aaaa0000000000000000000000000003", "Mira", "Danke!", 19),
    nachricht("nachricht_0006", "mitglied_aaaa0000000000000000000000000004", "Timo", "(versehentlich im falschen Raum)", 12, true),
    nachricht("nachricht_0007", "mitglied_aaaa0000000000000000000000000002", "Bosse", "Ich fange in zehn Minuten an, vorher noch schnell die Liste prüfen.", 4),
  ],
  raum_0001: [nachricht("nachricht_0010", "mitglied_aaaa0000000000000000000000000004", "Timo", "Hallo? Ist jemand da?", 6)],
  raum_0002: [nachricht("nachricht_0011", "mitglied_aaaa0000000000000000000000000002", "Bosse", "Protokoll der letzten Sitzung liegt im Vereinslaufwerk.", 120)],
};

function zustandDerWahrheit(verbunden: boolean): Zustand {
  const aktuellerRaum = "raum_0004";
  const raumListe = raeume.map((r) => ({ ...r, hier: r.id === aktuellerRaum, anwesend: mitglieder.filter((m) => m.raum === r.id).length }));
  return {
    verbindung: verbunden
      ? {
          art: "verbunden",
          server: "sprechwerk.verein.example:7443",
          server_name: "Sprechwerk Musterverein",
          ich: ICH,
          name: "Ada",
          rolle: "administrator",
        }
      : { art: "getrennt" },
    aktueller_raum: verbunden ? aktuellerRaum : null,
    raeume: verbunden ? raumListe : [],
    mitglieder: verbunden ? mitglieder.filter((m) => m.raum) : [],
    nachrichten: verbunden ? (nachrichten[aktuellerRaum] ?? []).map((n) => ({ ...n, loeschbar: true })) : [],
    protokoll: [],
    meldung: null,
  };
}

let verbunden = true;
let stand = zustandDerWahrheit(verbunden);
const horcher = new Set<(z: Zustand) => void>();

function melden() {
  const kopie: Zustand = JSON.parse(JSON.stringify(stand));
  horcher.forEach((h) => h(kopie));
}

/** Abonnement für die Oberfläche (Ersatz für das Tauri-Ereignis). */
export function abonnieren(cb: (z: Zustand) => void): () => void {
  horcher.add(cb);
  cb(JSON.parse(JSON.stringify(stand)));
  return () => horcher.delete(cb);
}

const einstellungenSchluessel = "sprechwerk-dummy-einstellungen";

export const einstellungenVorgabe: Einstellungen = {
  server: "sprechwerk.verein.example:7443",
  name: "Ada",
  thema: "dark",
  sprache: "de",
  passwort: "",
  mikrofon: "",
  lautsprecher: "",
  push_to_talk: true,
  ptt_taste: "Leertaste",
  schwelle: 30,
  vorlauf_ms: 60,
};

export const mockApi = {
  async zustand(): Promise<Zustand> {
    return JSON.parse(JSON.stringify(stand));
  },

  async verbinden(server: string, name: string): Promise<string> {
    verbunden = true;
    stand = zustandDerWahrheit(true);
    if (stand.verbindung?.art === "verbunden") {
      stand.verbindung.server = server;
      stand.verbindung.name = name;
    }
    melden();
    return "Sprechwerk Musterverein";
  },

  async trennen(): Promise<void> {
    verbunden = false;
    stand = zustandDerWahrheit(false);
    melden();
  },

  async betreten(raumId: string): Promise<void> {
    const r = raeume.find((x) => x.id === raumId);
    if (!r) return;
    stand.aktueller_raum = raumId;
    stand.raeume = stand.raeume.map((x) => ({ ...x, hier: x.id === raumId }));
    stand.nachrichten = (nachrichten[raumId] ?? []).map((n) => ({ ...n, loeschbar: true }));
    melden();
    // Kurz darauf ein „spricht"-Wechsel, damit man die Anzeige sieht.
    if (raumId === "raum_0004") {
      setTimeout(() => {
        stand.mitglieder = stand.mitglieder.map((m) => ({ ...m, spricht: m.name === "Bosse" }));
        melden();
        setTimeout(() => {
          stand.mitglieder = stand.mitglieder.map((m) => ({ ...m, spricht: m.name === "Mira" }));
          melden();
        }, 2600);
      }, 900);
    }
  },

  async verlassen(): Promise<void> {
    stand.aktueller_raum = null;
    stand.raeume = stand.raeume.map((x) => ({ ...x, hier: false }));
    stand.nachrichten = [];
    melden();
  },

  async textSenden(text: string): Promise<void> {
    const raumId = stand.aktueller_raum ?? "raum_0004";
    const n = nachricht(neueId("nachricht"), ICH, "Ada", text, 0);
    n.loeschbar = true;
    (nachrichten[raumId] ??= []).push(n);
    stand.nachrichten = (nachrichten[raumId] ?? []).map((x) => ({ ...x, loeschbar: true }));
    melden();
  },

  async nachrichtLoeschen(id: string): Promise<void> {
    const raumId = stand.aktueller_raum ?? "raum_0004";
    const liste = nachrichten[raumId] ?? [];
    const treffer = liste.find((n) => n.id === id);
    if (treffer) {
      treffer.geloescht = true;
      treffer.text = "";
    }
    stand.nachrichten = liste.map((x) => ({ ...x, loeschbar: true }));
    melden();
  },

  async raumAnlegen(name: string, eltern: string | null): Promise<void> {
    const tiefe = eltern ? (raeume.find((r) => r.id === eltern)?.tiefe ?? 0) + 1 : 0;
    raeume.push(raum(neueId("raum"), name, tiefe, eltern));
    stand.raeume = raeume.map((r) => ({ ...r, hier: r.id === stand.aktueller_raum, anwesend: mitglieder.filter((m) => m.raum === r.id).length }));
    melden();
  },

  async raumUmbenennen(id: string, name: string): Promise<void> {
    const r = raeume.find((x) => x.id === id);
    if (r) r.name = name;
    stand.raeume = stand.raeume.map((x) => (x.id === id ? { ...x, name } : x));
    melden();
  },

  async raumLoeschen(id: string): Promise<void> {
    const index = raeume.findIndex((r) => r.id === id);
    if (index >= 0) raeume.splice(index, 1);
    delete nachrichten[id];
    stand.raeume = stand.raeume.filter((r) => r.id !== id);
    melden();
  },

  async rolleSetzen(mitgliedId: string, rolle: Rolle): Promise<void> {
    const m = mitglieder.find((x) => x.id === mitgliedId);
    if (m) m.rolle = rolle;
    stand.mitglieder = stand.mitglieder.map((x) => (x.id === mitgliedId ? { ...x, rolle } : x));
    melden();
  },

  async stummschalten(mitgliedId: string, taub: boolean, an: boolean): Promise<void> {
    const m = mitglieder.find((x) => x.id === mitgliedId);
    if (m) {
      if (taub) {
        m.taub = an;
        if (an) m.stumm = true;
      } else {
        m.stumm = an;
        if (!an) m.taub = false;
      }
    }
    stand.mitglieder = stand.mitglieder.map((x) =>
      x.id === mitgliedId ? { ...x, stumm: m?.stumm ?? false, taub: m?.taub ?? false } : x,
    );
    melden();
  },

  async verschieben(mitgliedId: string, raumId: string): Promise<void> {
    const m = mitglieder.find((x) => x.id === mitgliedId);
    if (m) m.raum = raumId;
    stand.mitglieder = mitglieder.filter((x) => x.raum).map((x) => ({ ...x }));
    stand.raeume = raeume.map((r) => ({
      ...r,
      hier: r.id === stand.aktueller_raum,
      anwesend: mitglieder.filter((x) => x.raum === r.id).length,
    }));
    melden();
  },

  async ausRaumWerfen(mitgliedId: string): Promise<void> {
    const m = mitglieder.find((x) => x.id === mitgliedId);
    if (m) m.raum = "raum_0001";
    stand.mitglieder = mitglieder.filter((x) => x.raum).map((x) => ({ ...x }));
    stand.raeume = raeume.map((r) => ({
      ...r,
      hier: r.id === stand.aktueller_raum,
      anwesend: mitglieder.filter((x) => x.raum === r.id).length,
    }));
    melden();
  },

  async trenneMitglied(mitgliedId: string, grund: string): Promise<void> {
    const index = mitglieder.findIndex((x) => x.id === mitgliedId);
    if (index >= 0) mitglieder.splice(index, 1);
    stand.mitglieder = stand.mitglieder.filter((x) => x.id !== mitgliedId);
    stand.meldung = grund ? `getrennt: ${grund}` : "getrennt";
    melden();
  },

  async sperreMitglied(mitgliedId: string, grund: string): Promise<void> {
    const index = mitglieder.findIndex((x) => x.id === mitgliedId);
    if (index >= 0) mitglieder.splice(index, 1);
    stand.mitglieder = stand.mitglieder.filter((x) => x.id !== mitgliedId);
    stand.meldung = grund ? `gesperrt: ${grund}` : "gesperrt";
    melden();
  },

  async protokoll(): Promise<void> {
    stand.protokoll = [
      { laufnummer: 9, zeit: jetzt - 2 * minut, handelnder_name: "Ada", aktion: "raum_angelegt", ziel: "raum_0005", ergebnis: "erlaubt" },
      { laufnummer: 8, zeit: jetzt - 6 * minut, handelnder_name: "Timo", aktion: "raum_angelegt", ziel: "Gruppe B", ergebnis: "abgelehnt: Recht fehlt (RaumVerwalten)" },
      { laufnummer: 7, zeit: jetzt - 14 * minut, handelnder_name: "Ada", aktion: "rolle_gesetzt", ziel: "mitglied_…0003", ergebnis: "erlaubt" },
      { laufnummer: 6, zeit: jetzt - 18 * minut, handelnder_name: "Bosse", aktion: "text_geloescht", ziel: "nachricht_…0006", ergebnis: "erlaubt: hart" },
      { laufnummer: 5, zeit: jetzt - 26 * minut, handelnder_name: "Mira", aktion: "raum_betreten", ziel: "raum_0004", ergebnis: "erlaubt" },
      { laufnummer: 4, zeit: jetzt - 33 * minut, handelnder_name: "Timo", aktion: "text_gesendet", ziel: "raum_0001", ergebnis: "erlaubt" },
      { laufnummer: 3, zeit: jetzt - 41 * minut, handelnder_name: "Bosse", aktion: "raum_umbenannt", ziel: "raum_0006", ergebnis: "erlaubt" },
      { laufnummer: 2, zeit: jetzt - 55 * minut, handelnder_name: "Ada", aktion: "sperre_verhaengt", ziel: "Adresse 192.0.2.7", ergebnis: "erlaubt" },
      { laufnummer: 1, zeit: jetzt - 70 * minut, handelnder_name: "Ada", aktion: "erster_beitritt", ziel: "mitglied_…0001", ergebnis: "erlaubt" },
    ];
    melden();
  },

  async einstellungenLesen(): Promise<Einstellungen> {
    try {
      const roh = localStorage.getItem(einstellungenSchluessel);
      if (roh) return { ...einstellungenVorgabe, ...JSON.parse(roh) };
    } catch {
      /* egal */
    }
    return { ...einstellungenVorgabe };
  },

  async einstellungenSpeichern(e: Einstellungen): Promise<void> {
    try {
      localStorage.setItem(einstellungenSchluessel, JSON.stringify(e));
    } catch {
      /* egal */
    }
  },
};

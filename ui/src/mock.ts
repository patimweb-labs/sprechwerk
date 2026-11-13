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

function raum(
  id: string,
  name: string,
  anwesend = 0,
  hier = false,
  verschluesselt = false,
): Raum {
  return { id, name, anwesend, hier, verschluesselt };
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
  raum("raum_0001", "Empfang", 1),
  raum("raum_0002", "Vorstand", 1),
  raum("raum_0004", "Trainingsraum", 2, true),
  raum("raum_0006", "Technik"),
  raum("raum_0007", "Off-Topic", 1),
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
    // Protokoll und Betriebszahlen holt man sich ausdrücklich; im Dummy
    // beginnen sie leer, damit man den Weg „noch nicht gelesen" sieht.
    latenz_ms: null,
    schluessel: [],
    guete: null,
    sperren: [],
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
  chat_eingeklappt: false,
  rauschunterdrueckung: true,
  noisegate: true,
  echounterdrueckung: true,
  favoriten: [
    { name: "Trainingsabend", server: "sprechwerk.verein.example:7443" },
    { name: "Vorstand (Test)", server: "192.0.2.11:7443" },
    { name: "Werkstatt", server: "192.0.2.12:7443" },
  ],
};

export const mockApi = {
  async schluesselAnfordern(): Promise<void> {
    melden();
  },

  async ping(): Promise<void> {
    // Im Dummy eine plausible Loopback-Zahl, damit man die Anzeige beurteilen kann.
    stand.latenz_ms = 8 + Math.floor(Math.random() * 6);
    melden();
  },

  async sperrliste(): Promise<void> {
    stand.sperren = [
      {
        kennung: "ab".repeat(32),
        adresse: null,
        grund: "Spam im Chat",
        zeit: Date.now() - 3_600_000,
        handelnder_name: "Ada",
      },
      {
        kennung: null,
        adresse: "203.0.113.9",
        grund: "Störung",
        zeit: Date.now() - 86_400_000,
        handelnder_name: "Ada",
      },
    ];
    melden();
  },

  async zaehler(): Promise<void> {
    stand.guete = { sprachrahmen: 12480, verteilt: 87360, verloren: 0, abgelehnt: 3, verbundene: 4 };
    melden();
  },

  async protokoll(): Promise<void> {
    stand.protokoll = [
      {
        laufnummer: 12,
        zeit: Date.now() - 60_000,
        handelnder_name: "Ada",
        aktion: "rolle_gesetzt",
        ziel: "Bosse",
        vorher: "mitglied",
        nachher: "moderator",
        ergebnis: "erlaubt",
      },
      {
        laufnummer: 11,
        zeit: Date.now() - 120_000,
        handelnder_name: "Bosse",
        aktion: "raum_angelegt",
        ziel: "Vorstand",
        vorher: null,
        nachher: null,
        ergebnis: "abgelehnt: dafür fehlt das Recht",
      },
      {
        laufnummer: 10,
        zeit: Date.now() - 300_000,
        handelnder_name: "",
        aktion: "anmeldung",
        ziel: "ab12…",
        vorher: null,
        nachher: null,
        ergebnis: "abgelehnt: gesperrt (Spam)",
      },
    ];
    melden();
  },

  async entsperren(): Promise<void> {
    melden();
  },

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

  async raumAnlegen(name: string): Promise<void> {
    raeume.push(raum(neueId("raum"), name));
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

  async audioGeraete() {
    return {
      mikrofon: [
        { id: "alsa_input.usb-Sennheiser_Profile_00.mono-fallback", name: "Sennheiser Profile", vorgabe: true },
        { id: "alsa_input.pci-0000_0f_00.6.analog-stereo", name: "Mainboard analog", vorgabe: false },
      ],
      lautsprecher: [
        { id: "alsa_output.usb-Sennheiser_Profile_00.analog-stereo", name: "Sennheiser Profile", vorgabe: true },
        { id: "alsa_output.hdmi-stereo", name: "HDMI", vorgabe: false },
      ],
    };
  },

  // --- Ton und Verschlüsselung: im Dummy ohne Wirkung, aber vorhanden, damit
  // die Oberfläche nicht in Fehler läuft.
  async audioStarten(): Promise<void> {},
  async audioStoppen(): Promise<void> {},
  async audioStand() {
    return { laeuft: verbunden, pegel: 0, stumm: false, taub: false };
  },
  async audioEinstellen(): Promise<void> {},
  async sprechtaste(): Promise<void> {},
  async mikrofonStumm(): Promise<void> {},
  async hoerenAus(): Promise<void> {},

  async raumVerschluesseln(raum: string, an: boolean): Promise<void> {
    const r = stand.raeume.find((x) => x.id === raum);
    if (r) r.verschluesselt = an;
    melden();
  },

  async audioPegel(): Promise<number> {
    // Im Dummy: schwankender Pegel, damit man die Anzeige sieht.
    return 20 + Math.floor(Math.random() * 60);
  },

  async audioTestton(): Promise<void> {
    await new Promise((f) => setTimeout(f, 400));
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

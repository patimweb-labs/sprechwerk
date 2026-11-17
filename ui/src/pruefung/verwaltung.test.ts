// Verwaltung im Browser: reine Funktionen und die Ansicht gegen einen
// nachgebauten Server (`fetch` ersetzt). Der echte Server ist in
// `crates/server/src/verwaltung.rs` geprüft — hier geht es darum, dass die
// Seite auf seine Antworten richtig reagiert.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { setSprache } from "../i18n";
import {
  anhaengen,
  dauer,
  rolleWaehlbar,
  stufe,
  type Mitgliedszeile,
  type Stand,
} from "../verwaltung/logik";
import VerwaltungApp from "../verwaltung/VerwaltungApp.vue";
import { ansicht, meldung, protokoll, stoppen } from "../verwaltung/store";

function mitglied(teil: Partial<Mitgliedszeile>): Mitgliedszeile {
  return {
    id: "mitglied_" + "0".repeat(32),
    name: "Ada",
    kennung: "ab".repeat(32),
    rolle: "mitglied",
    verbunden: false,
    raum: null,
    zuletzt_gesehen: 0,
    ...teil,
  };
}

function stand(mitglieder: Mitgliedszeile[], teil: Partial<Stand> = {}): Stand {
  return {
    name: "Vereinsheim",
    version: "0.1.0",
    adresse: "0.0.0.0:7443",
    port: 7443,
    fingerabdruck: "12".repeat(32),
    gestartet: 0,
    jetzt: 3_600_000,
    raeume: 2,
    verbundene: mitglieder.filter((m) => m.verbunden).length,
    erster_wird_administrator: mitglieder.length === 0,
    sicherung: { aktiv: true, verzeichnis: "/data/sicherungen", letzte: null },
    mitglieder,
    ...teil,
  };
}

describe("Verwaltung: Logik", () => {
  it("erkennt die Stufe einer Protokollzeile", () => {
    expect(stufe("2026-11-17T07:19:37Z ERROR Sicherung fehlgeschlagen")).toBe("fehler");
    expect(stufe("2026-11-17T07:19:37Z  WARN Lebenszeichen")).toBe("warnung");
    expect(stufe("2026-11-17T07:19:37Z  INFO lauscht")).toBe("info");
    expect(stufe("2026-11-17T07:19:37Z DEBUG Verbindung beendet")).toBe("leise");
    // Ohne Stufe (z. B. die Fingerabdruck-Kopfzeilen): normal zeigen.
    expect(stufe("sprechwerk lauscht auf 0.0.0.0:7443")).toBe("info");
    // „INFORMATION" im Text ist keine Stufe.
    expect(stufe("INFORMATIONEN")).toBe("info");
  });

  it("hängt nur neue Zeilen an und begrenzt die Länge", () => {
    const a = [
      { nr: 0, text: "a" },
      { nr: 1, text: "b" },
    ];
    // Überschneidung (Zeile 1 kommt doppelt) wird nicht doppelt gezeigt.
    const b = anhaengen(a, [
      { nr: 1, text: "b" },
      { nr: 2, text: "c" },
    ]);
    expect(b.map((z) => z.nr)).toEqual([0, 1, 2]);
    expect(anhaengen(b, [])).toBe(b);
    expect(anhaengen(b, [{ nr: 3, text: "d" }], 2).map((z) => z.nr)).toEqual([2, 3]);
  });

  it("schreibt Laufzeiten lesbar", () => {
    expect(dauer(0)).toBe("0 min");
    expect(dauer(5 * 60_000)).toBe("5 min");
    expect(dauer(192 * 60_000)).toBe("3 h 12 min");
    expect(dauer((2 * 1440 + 4 * 60) * 60_000)).toBe("2 d 4 h");
  });

  it("bietet den letzten Administrator nicht zum Herabstufen an", () => {
    const ada = mitglied({ rolle: "administrator" });
    const bosse = mitglied({ id: "mitglied_" + "1".repeat(32), name: "Bosse" });
    const s = stand([ada, bosse]);
    expect(rolleWaehlbar(s, ada, "gast")).toBe(false);
    expect(rolleWaehlbar(s, ada, "administrator")).toBe(true);
    expect(rolleWaehlbar(s, bosse, "administrator")).toBe(true);
    const zwei = stand([ada, { ...bosse, rolle: "administrator" }]);
    expect(rolleWaehlbar(zwei, ada, "gast")).toBe(true);
  });
});

/** Ein nachgebauter Server: antwortet je Pfad, merkt sich die Anfragen. */
function server(antworten: Record<string, () => { status: number; daten?: unknown }>) {
  const anfragen: { pfad: string; methode: string; daten?: unknown }[] = [];
  const fetchErsatz = vi.fn(async (pfad: string, init?: RequestInit) => {
    const ohneAbfrage = pfad.split("?")[0];
    anfragen.push({
      pfad: ohneAbfrage,
      methode: init?.method ?? "GET",
      daten: init?.body ? JSON.parse(String(init.body)) : undefined,
    });
    const antwort = antworten[`${init?.method ?? "GET"} ${ohneAbfrage}`]?.() ?? { status: 404 };
    return new Response(antwort.daten === undefined ? null : JSON.stringify(antwort.daten), {
      status: antwort.status,
    });
  });
  vi.stubGlobal("fetch", fetchErsatz);
  return anfragen;
}

describe("Verwaltung: Ansicht", () => {
  beforeEach(() => {
    setSprache("de");
    ansicht.value = { art: "laedt" };
    protokoll.zeilen = [];
    protokoll.weiterAb = 0;
    meldung.value = null;
  });
  afterEach(() => {
    stoppen();
    vi.unstubAllGlobals();
  });

  it("ohne Anmeldung fragt sie nach dem Code und meldet einen falschen", async () => {
    const anfragen = server({
      "GET /api/stand": () => ({ status: 401 }),
      "POST /api/anmelden": () => ({ status: 401, daten: { fehler: "Code falsch" } }),
    });
    const w = mount(VerwaltungApp);
    await flushPromises();
    expect(w.text()).toContain("Zugang zur Verwaltung");

    await w.find("input").setValue("falsch");
    await w.find("form").trigger("submit");
    await flushPromises();
    expect(anfragen.find((a) => a.pfad === "/api/anmelden")?.daten).toEqual({ code: "falsch" });
    expect(w.text()).toContain("Der Code stimmt nicht.");
    w.unmount();
  });

  it("zeigt Server, Mitglieder und Protokoll", async () => {
    const ada = mitglied({ rolle: "administrator", verbunden: true, raum: "Empfang" });
    server({
      "GET /api/stand": () => ({ status: 200, daten: stand([ada]) }),
      "GET /api/protokoll": () => ({
        status: 200,
        daten: { zeilen: [{ nr: 0, text: "2026-11-17T07:19:37Z  WARN etwas" }], weiter_ab: 1 },
      }),
    });
    const w = mount(VerwaltungApp);
    await flushPromises();
    const text = w.text();
    expect(text).toContain("Vereinsheim");
    expect(text).toContain("Port 7443 (UDP)");
    // Fingerabdruck vollständig in Vierergruppen, wie der Server ihn druckt.
    expect(text).toContain("1212:1212:1212");
    expect(text).toContain("Ada");
    expect(text).toContain("in Empfang");
    expect(text).toContain("1 h 0 min");
    expect(w.find(".zeile--warnung").text()).toContain("etwas");
    w.unmount();
  });

  it("warnt, solange der Erste noch Administrator wird", async () => {
    server({
      "GET /api/stand": () => ({ status: 200, daten: stand([]) }),
      "GET /api/protokoll": () => ({ status: 200, daten: { zeilen: [], weiter_ab: 0 } }),
    });
    const w = mount(VerwaltungApp);
    await flushPromises();
    expect(w.text()).toContain("Noch niemand da");
    w.unmount();
  });

  it("vergibt eine Rolle und meldet den letzten Administrator", async () => {
    const ada = mitglied({ rolle: "administrator" });
    const bosse = mitglied({ id: "mitglied_" + "1".repeat(32), name: "Bosse" });
    const anfragen = server({
      "GET /api/stand": () => ({ status: 200, daten: stand([ada, bosse]) }),
      "GET /api/protokoll": () => ({ status: 200, daten: { zeilen: [], weiter_ab: 0 } }),
      "POST /api/rolle": () => ({ status: 409, daten: { fehler: "letzter_administrator" } }),
    });
    const w = mount(VerwaltungApp, { attachTo: document.body });
    await flushPromises();

    // Bosse über die Auswahl zum Administrator machen.
    const auswahl = w.findAll(".wp-select__trigger");
    expect(auswahl).toHaveLength(2);
    await auswahl[1].trigger("click");
    const option = w.findAll('[role="option"]').find((o) => o.text() === "Administrator");
    await option!.trigger("mousedown");
    await flushPromises();
    expect(anfragen.find((a) => a.pfad === "/api/rolle")?.daten).toEqual({
      mitglied: bosse.id,
      rolle: "administrator",
    });
    // Der Server lehnt ab (hier künstlich): die Seite sagt, warum.
    expect(w.text()).toContain("letzte Administrator");
    w.unmount();
  });

  it("verliert der Server die Anmeldung, steht wieder die Anmeldung da", async () => {
    let angemeldet = true;
    server({
      "GET /api/stand": () =>
        angemeldet ? { status: 200, daten: stand([mitglied({})]) } : { status: 401 },
      "GET /api/protokoll": () => ({ status: 200, daten: { zeilen: [], weiter_ab: 0 } }),
    });
    const w = mount(VerwaltungApp);
    await flushPromises();
    expect(w.text()).toContain("Mitglieder und Rollen");
    // Neustart des Servers: neuer Code, der Keks passt nicht mehr.
    angemeldet = false;
    const { standLaden } = await import("../verwaltung/store");
    await standLaden();
    await flushPromises();
    expect(w.text()).toContain("Zugang zur Verwaltung");
    w.unmount();
  });
});

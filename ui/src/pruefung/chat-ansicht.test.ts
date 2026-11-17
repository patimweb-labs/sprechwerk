// Reaktionen, Links, Vorschau und Effekte in der Ansicht (ADR 0012, Stufe 2).
// Geprüft wird, was aus dem Modell folgt — gerechnet hat der Rust-Teil.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import RaeumeView from "../views/RaeumeView.vue";
import EffektBuehne from "../views/EffektBuehne.vue";
import { api, type Nachricht } from "../api";
import { einstellungen, modell } from "../store";

function n(id: string, inhalt: Nachricht["inhalt"], reaktionen: Nachricht["reaktionen"] = []): Nachricht {
  return { id, verfasser: "mitglied_2", verfasser_name: "Bosse", inhalt, zeit: 0, geloescht: false, loeschbar: false, reaktionen };
}

beforeEach(() => {
  modell.verbindung = { art: "verbunden", server: "s", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "mitglied" };
  modell.raeume = [{ id: "raum_1", name: "Empfang", anwesend: 1, hier: true, verschluesselt: true }];
  modell.mitglieder = [];
  modell.textschluessel = "bereit";
  modell.effekt = null;
  modell.entwurf = { art: "keiner" };
  einstellungen.link_vorschau = true;
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  modell.nachrichten = [];
});

describe("Reaktionen", () => {
  it("stehen am Ziel, nicht als eigener Beitrag — ein Klick schaltet um", async () => {
    const reagieren = vi.spyOn(api, "reagieren").mockResolvedValue(undefined);
    modell.nachrichten = [
      n("nachricht_a", { zustand: "text", text: "Wer kommt Samstag?" }, [
        { zeichen: "👍", anzahl: 2, ich: true, namen: ["Ada", "Mira"] },
        { zeichen: "🎉", anzahl: 1, ich: false, namen: ["Jonas"] },
      ]),
      n("nachricht_b", { zustand: "reaktion", ziel: "nachricht_a", zeichen: "👍", weg: false }),
    ];
    const w = mount(RaeumeView);
    expect(w.findAll(".beitrag")).toHaveLength(1);
    const knoepfe = w.findAll(".reaktion");
    expect(knoepfe.map((k) => k.text())).toEqual(["👍2", "🎉1"]);
    expect(knoepfe[0].attributes("aria-pressed")).toBe("true");
    expect(knoepfe[1].attributes("aria-pressed")).toBe("false");
    expect(knoepfe[0].attributes("title")).toBe("Ada, Mira");
    await knoepfe[1].trigger("click");
    expect(reagieren).toHaveBeenCalledWith("nachricht_a", "🎉");
  });
});

describe("Links und Vorschau", () => {
  it("Links sind anklickbar und öffnen im Browser des Systems", async () => {
    const oeffnen = vi.spyOn(api, "linkOeffnen").mockResolvedValue(undefined);
    modell.nachrichten = [
      n("nachricht_a", {
        zustand: "text",
        text: "Anmeldung: https://verein.example/fest. Und javascript:alert(1) nicht",
        vorschau: { url: "https://verein.example/fest", titel: "Sommerfest", beschreibung: "Am Vereinsheim", bild: "QUJD" },
      }),
    ];
    const w = mount(RaeumeView);
    const links = w.findAll("a.beitrag__link");
    expect(links).toHaveLength(1);
    expect(links[0].attributes("href")).toBe("https://verein.example/fest");
    await links[0].trigger("click");
    expect(oeffnen).toHaveBeenCalledWith("https://verein.example/fest");

    const karte = w.find("a.vorschau");
    expect(karte.text()).toContain("Sommerfest");
    expect(karte.text()).toContain("Am Vereinsheim");
    expect(karte.find("img").attributes("src")).toBe("data:image/jpeg;base64,QUJD");
  });

  it("der erste Link im Entwurf wird zur Vorschau angefragt — abgeschaltet nicht", async () => {
    vi.useFakeTimers();
    const vorschau = vi.spyOn(api, "vorschau").mockResolvedValue(undefined);
    modell.nachrichten = [];
    const w = mount(RaeumeView);
    await w.find(".schreiben input").setValue("schau mal https://verein.example/fest");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(vorschau).toHaveBeenLastCalledWith("https://verein.example/fest");

    einstellungen.link_vorschau = false;
    await w.find(".schreiben input").setValue("schau mal https://verein.example/anders");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(vorschau).toHaveBeenLastCalledWith(null);
  });

  it("eine fertige Vorschau steht über dem Feld und lässt sich wegklicken", async () => {
    const verwerfen = vi.spyOn(api, "vorschauVerwerfen").mockResolvedValue(undefined);
    modell.nachrichten = [];
    modell.entwurf = { art: "fertig", vorschau: { url: "https://verein.example/fest", titel: "Sommerfest" } };
    const w = mount(RaeumeView);
    expect(w.find(".entwurf__vorschau").text()).toContain("Sommerfest");
    await w.find(".entwurf__vorschau button").trigger("click");
    expect(verwerfen).toHaveBeenCalled();
  });
});

describe("Effekte", () => {
  it("Konfetti spielt auf der Bühne, Beben am Beitrag", async () => {
    modell.nachrichten = [n("nachricht_a", { zustand: "text", text: "Glückwunsch!", effekt: "beben" })];
    const buehne = mount(EffektBuehne);
    expect(buehne.find(".buehne").exists()).toBe(false);
    modell.effekt = { nachricht: "nachricht_a", effekt: "konfetti", nr: 1 };
    await flushPromises();
    expect(buehne.findAll(".teil").length).toBeGreaterThan(10);

    modell.effekt = { nachricht: "nachricht_a", effekt: "beben", nr: 2 };
    const w = mount(RaeumeView);
    await flushPromises();
    expect(buehne.find(".buehne").exists()).toBe(false);
    expect(w.find(".bebt").exists()).toBe(true);
    expect(w.find(".beitrag__effekt").text()).toBe("Beben");
  });
});

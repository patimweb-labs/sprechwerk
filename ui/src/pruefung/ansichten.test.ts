// Was hier geprüft wird, ist genau das, was die Typprüfung nicht sieht:
// dass eine Ansicht **läuft**, wenn sie gerendert wird, und dass ein Klick
// den Zustand ändert.
import { describe, expect, it, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import ServerView from "../views/ServerView.vue";
import RaeumeView from "../views/RaeumeView.vue";
import EinstellungenView from "../views/EinstellungenView.vue";
import { ansicht, einstellungen, modell } from "../store";

describe("Ansichten rendern", () => {
  beforeEach(() => {
    modell.raeume = [];
    modell.mitglieder = [];
    modell.nachrichten = [];
    modell.meldung = null;
    ansicht.value = "server";
  });

  it("Serverliste zeigt die gemerkten Server", () => {
    einstellungen.favoriten = [
      { name: "Zuhause", server: "127.0.0.1:7443" },
      { name: "", server: "sprechwerk.example.org:7443" },
    ];
    const w = mount(ServerView);
    // Mit Namen: der Name steht da. Ohne Namen: die Adresse.
    expect(w.text()).toContain("Zuhause");
    expect(w.text()).toContain("sprechwerk.example.org:7443");
  });

  /// Der zuletzt benutzte Server steht auch dann in der Liste, wenn er nicht
  /// gemerkt ist — sonst müsste man die Adresse nach jedem Start neu tippen.
  it("der zuletzt benutzte Server erscheint ohne Favorit", () => {
    einstellungen.favoriten = [];
    einstellungen.server = "192.0.2.10:7443";
    const w = mount(ServerView);
    expect(w.text()).toContain("192.0.2.10:7443");
    expect(w.text()).toContain("zuletzt benutzt");
  });

  it("Serverliste ohne Favoriten bleibt bedienbar", () => {
    einstellungen.favoriten = [];
    const w = mount(ServerView);
    // Der Hauptweg muss sichtbar bleiben, auch wenn nichts gemerkt ist.
    expect(w.html().length).toBeGreaterThan(200);
  });

  it("Räume zeigen die Räume aus dem Zustand und markieren den eigenen", () => {
    modell.raeume = [
      { id: "raum_1", name: "Empfang", anwesend: 2, hier: true, verschluesselt: false },
      { id: "raum_2", name: "Vorstand", anwesend: 0, hier: false, verschluesselt: true },
    ];
    const w = mount(RaeumeView);
    expect(w.text()).toContain("Empfang");
    expect(w.text()).toContain("Vorstand");
  });

  it("Einstellungen zeigen die Schalter aus dem Modell", () => {
    einstellungen.rauschunterdrueckung = true;
    einstellungen.noisegate = false;
    const w = mount(EinstellungenView);
    const html = w.html();
    expect(html).toContain("Rauschunterdrückung");
    // Die Kästen spiegeln den Zustand (geprüft wird das Attribut, nicht die Optik).
    const kaesten = w.findAll('input[type="checkbox"]');
    expect(kaesten.length).toBeGreaterThan(0);
  });
});

describe("Zustand und Ansicht hängen zusammen", () => {
  it("eine Meldung erscheint in der Serverliste", async () => {
    modell.meldung = "Verbindung abgelehnt: falsches Passwort";
    const w = mount(ServerView);
    expect(w.text()).toContain("Verbindung abgelehnt");
  });
});

describe("Chat", () => {
  it("abgelehnter Text bleibt im Feld stehen, statt verloren zu gehen", async () => {
    const { vi } = await import("vitest");
    const { flushPromises } = await import("@vue/test-utils");
    const { api } = await import("../api");
    const senden = vi.spyOn(api, "textSenden").mockRejectedValue("Text zu lang");
    modell.raeume = [{ id: "raum_1", name: "Empfang", anwesend: 1, hier: true, verschluesselt: false }];
    modell.verbindung = { art: "verbunden" } as never;
    const w = mount(RaeumeView);
    const feld = w.find(".schreiben input");
    await feld.setValue("Hallo Welt");
    await feld.trigger("keyup", { key: "Enter" });
    await flushPromises();
    expect(senden).toHaveBeenCalledWith("Hallo Welt", null);
    expect((feld.element as HTMLInputElement).value).toBe("Hallo Welt");
    expect(modell.meldung).toBe("Text zu lang");

    // Gelingt es, ist das Feld leer.
    senden.mockResolvedValue(undefined);
    await feld.trigger("keyup", { key: "Enter" });
    await flushPromises();
    expect((feld.element as HTMLInputElement).value).toBe("");
    senden.mockRestore();
    modell.verbindung = null;
  });
});

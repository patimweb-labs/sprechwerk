// Eine Sperre, die man nicht aufheben kann, ist im Betrieb eine Falle. Diese
// Ansicht zeigt beides: wer gesperrt ist und den Knopf zurück.
import { describe, expect, it, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import SperrenView from "../views/SperrenView.vue";
import { api } from "../api";
import { modell } from "../store";

function sperre(teil: Partial<(typeof modell.sperren)[number]> = {}) {
  return {
    kennung: "ab".repeat(32),
    adresse: null,
    grund: "Spam im Chat",
    zeit: Date.parse("2026-11-13T10:00:00"),
    handelnder_name: "Ada",
    ...teil,
  };
}

describe("Sperrliste", () => {
  beforeEach(() => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "administrator" };
    modell.sperren = [];
  });

  it("zeigt Kennung, Grund, Verhänger und Zeit", () => {
    modell.sperren = [sperre()];
    const w = mount(SperrenView);
    const text = w.text();
    expect(text).toContain("abababab…");
    expect(text).toContain("Spam im Chat");
    expect(text).toContain("Ada");
    expect(text).toContain("13.11.");
    expect(text).toContain("Kennung");
  });

  it("unterscheidet Kennung und Adresse", () => {
    modell.sperren = [sperre({ kennung: null, adresse: "203.0.113.9" })];
    const w = mount(SperrenView);
    expect(w.text()).toContain("203.0.113.9");
    expect(w.text()).toContain("Adresse");
  });

  it("bietet für jede Zeile ein Aufheben an", () => {
    modell.sperren = [sperre(), sperre({ kennung: null, adresse: "198.51.100.7" })];
    const w = mount(SperrenView);
    const knoepfe = w.findAll("button").filter((b) => b.text().includes("Aufheben"));
    expect(knoepfe.length).toBe(2);
  });

  it("leere Liste erklärt sich", () => {
    const w = mount(SperrenView);
    expect(w.text()).toContain("Niemand gesperrt");
  });

  it("ein Moderator sieht den Hinweis, keine Daten", () => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_2", name: "Bosse", rolle: "moderator" };
    modell.sperren = [sperre()];
    const w = mount(SperrenView);
    expect(w.text()).toContain("Nur Administratoren");
    expect(w.text()).not.toContain("Spam im Chat");
  });
});

describe("Aufheben", () => {
  beforeEach(() => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "administrator" };
    modell.sperren = [sperre()];
  });

  it("schickt Kennung und Adresse der Zeile an den Server", async () => {
    const entsperren = vi.spyOn(api, "entsperren").mockResolvedValue(undefined);
    const sperrliste = vi.spyOn(api, "sperrliste").mockResolvedValue(undefined);
    const w = mount(SperrenView);
    const knopf = w.findAll("button").find((b) => b.text().includes("Aufheben"));
    expect(knopf).toBeTruthy();
    await knopf!.trigger("click");
    await new Promise((r) => setTimeout(r, 0));
    expect(entsperren).toHaveBeenCalledWith("ab".repeat(32), null);
    // Danach wird die Liste neu geholt — die Zeile soll ja verschwinden.
    expect(sperrliste).toHaveBeenCalled();
    entsperren.mockRestore();
    sperrliste.mockRestore();
  });
});

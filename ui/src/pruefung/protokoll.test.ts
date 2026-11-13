// Das Prüfprotokoll ist die sichtbare Zusage dieses Projekts: man sieht, wer
// was getan hat — und was **abgelehnt** wurde. Genau das wird hier geprüft.
import { describe, expect, it, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import ProtokollView from "../views/ProtokollView.vue";
import { modell } from "../store";

function eintrag(teil: Partial<(typeof modell.protokoll)[number]> = {}) {
  return {
    laufnummer: 1,
    zeit: Date.parse("2026-11-13T14:05:09"),
    handelnder_name: "Ada",
    aktion: "raum_angelegt",
    ziel: "Vorstand",
    vorher: null,
    nachher: null,
    ergebnis: "erlaubt",
    ...teil,
  };
}

describe("Prüfprotokoll", () => {
  beforeEach(() => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "administrator" };
    modell.protokoll = [];
  });

  it("zeigt die Einträge mit Zeit, Handelndem, Aktion und Ziel", () => {
    modell.protokoll = [eintrag()];
    const w = mount(ProtokollView);
    const text = w.text();
    expect(text).toContain("14:05:09");
    expect(text).toContain("Ada");
    // Die technische Aktion wird übersetzt, nicht roh angezeigt.
    expect(text).toContain("Raum angelegt");
    expect(text).not.toContain("raum_angelegt");
    expect(text).toContain("Vorstand");
  });

  it("kennzeichnet abgelehnte Versuche", () => {
    modell.protokoll = [
      eintrag({ laufnummer: 2, aktion: "raum_angelegt", ergebnis: "abgelehnt: dafür fehlt das Recht" }),
    ];
    const w = mount(ProtokollView);
    expect(w.findAll("tr.abgelehnt").length).toBe(1);
    expect(w.text()).toContain("abgelehnt");
  });

  it("zeigt bei Anmeldungen den Server als Handelnden", () => {
    modell.protokoll = [eintrag({ handelnder_name: "", aktion: "anmeldung", ziel: "ab12" })];
    const w = mount(ProtokollView);
    expect(w.text()).toContain("Server");
  });

  it("zeigt Änderungen als vorher → nachher", () => {
    modell.protokoll = [eintrag({ aktion: "rolle_gesetzt", vorher: "mitglied", nachher: "moderator" })];
    const w = mount(ProtokollView);
    expect(w.text()).toContain("mitglied → moderator");
  });

  it("leeres Protokoll erklärt sich, statt leer zu bleiben", () => {
    const w = mount(ProtokollView);
    expect(w.text()).toContain("Noch nichts geschehen");
  });

  it("ein Mitglied sieht den Hinweis statt einer leeren Tabelle", () => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_2", name: "Bosse", rolle: "mitglied" };
    modell.protokoll = [eintrag()];
    const w = mount(ProtokollView);
    expect(w.text()).toContain("Nur Administrator und Moderator");
    // Und die Einträge bleiben verborgen — Rechte sind keine Höflichkeit.
    expect(w.text()).not.toContain("Vorstand");
  });
});

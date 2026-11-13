// Der häufigste Grund für „ich höre niemanden" ist ein fehlender Raumschlüssel.
// Diese Zeile macht ihn sichtbar — und bietet an, ihn erneut zu holen.
import { describe, expect, it, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import RaeumeView from "../views/RaeumeView.vue";
import { modell, schluesselVorhanden } from "../store";

function raum(verschluesselt: boolean) {
  return { id: "raum_1", name: "Geheim", anwesend: 1, hier: true, verschluesselt };
}

describe("Schlüsselstand", () => {
  beforeEach(() => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "administrator" };
    modell.mitglieder = [];
    modell.nachrichten = [];
    modell.schluessel = [];
  });

  it("ein offener Raum stellt die Frage nicht", () => {
    modell.raeume = [raum(false)];
    expect(schluesselVorhanden("raum_1")).toBeNull();
  });

  it("verschlüsselt ohne Schlüssel: false", () => {
    modell.raeume = [raum(true)];
    expect(schluesselVorhanden("raum_1")).toBe(false);
  });

  it("verschlüsselt mit Schlüssel: true", () => {
    modell.raeume = [raum(true)];
    modell.schluessel = [{ raum: "raum_1", epoche: 1794575146 }];
    expect(schluesselVorhanden("raum_1")).toBe(true);
  });

  it("die Ansicht sagt es und bietet das Nachfordern an", () => {
    modell.raeume = [raum(true)];
    const w = mount(RaeumeView);
    expect(w.text()).toContain("Schlüssel fehlt");
    const knoepfe = w.findAll("button").filter((b) => b.text().includes("Schlüssel anfordern"));
    expect(knoepfe.length).toBe(1);
  });

  it("mit Schlüssel steht die gute Nachricht da", () => {
    modell.raeume = [raum(true)];
    modell.schluessel = [{ raum: "raum_1", epoche: 1794575146 }];
    const w = mount(RaeumeView);
    expect(w.text()).toContain("Schlüssel liegt vor");
    expect(w.text()).not.toContain("Schlüssel fehlt");
  });
});

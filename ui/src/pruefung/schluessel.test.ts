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

  it("mit Schlüssel bleibt es ruhig — jeder Raum ist verschlüsselt", () => {
    modell.raeume = [raum(true)];
    modell.schluessel = [{ raum: "raum_1", epoche: 1794575146 }];
    const w = mount(RaeumeView);
    expect(w.text()).not.toContain("Schlüssel fehlt");
    // Einmal im Kopf, nicht an jedem Raum (ADR 0012).
    expect(w.text()).toContain("Ende zu Ende verschlüsselt");
  });

  it("fremde Räume warnen nicht — dort ist man ja nicht", () => {
    modell.raeume = [raum(true), { ...raum(true), id: "raum_2", name: "Vorstand", hier: false }];
    modell.schluessel = [{ raum: "raum_1", epoche: 1 }];
    const w = mount(RaeumeView);
    expect(w.text()).not.toContain("Schlüssel fehlt");
  });
});

describe("Chat Ende zu Ende (ADR 0012)", () => {
  beforeEach(() => {
    modell.verbindung = { art: "verbunden", server: "s:7443", server_name: "Werk", ich: "mitglied_1", name: "Ada", rolle: "mitglied" };
    modell.mitglieder = [];
    modell.schluessel = [];
    modell.raeume = [raum(true)];
    modell.textschluessel = "bereit";
  });

  function n(id: string, inhalt: (typeof modell.nachrichten)[number]["inhalt"]) {
    return { id, verfasser: "mitglied_2", verfasser_name: "Bosse", inhalt, zeit: 0, geloescht: false, loeschbar: false, reaktionen: [] };
  }

  it("zeigt jeden Zustand einer Nachricht — nie den Geheimtext", () => {
    modell.nachrichten = [
      n("a", { zustand: "text", text: "Guten Abend" }),
      n("b", { zustand: "wartet" }),
      n("c", { zustand: "unlesbar" }),
      n("d", { zustand: "neuer", art: "anhang" }),
      n("e", { zustand: "geloescht" }),
    ];
    const w = mount(RaeumeView);
    const text = w.text();
    expect(text).toContain("Guten Abend");
    expect(text).toContain("der Schlüssel fehlt noch");
    expect(text).toContain("Nicht lesbar");
    expect(text).toContain("„anhang“");
    expect(text).toContain("Nachricht gelöscht");
    expect(text).not.toContain("e1.");
  });

  it("ohne Schlüssel kein Eingabefeld, sondern die Erklärung", () => {
    modell.nachrichten = [];
    modell.textschluessel = "wartet";
    const w = mount(RaeumeView);
    expect(w.find(".schreiben input").exists()).toBe(false);
    expect(w.text()).toContain("sobald eines online ist");
    modell.textschluessel = "bereit";
  });
});

// Zwei Sprachen, ein Schlüssel: fehlt eine Übersetzung, muss das auffallen —
// nicht als leerer Text im Fenster.
import { describe, expect, it } from "vitest";
import { setSprache, t } from "../i18n";

describe("Übersetzungen", () => {
  it("deutsch und englisch unterscheiden sich, sind aber beide gefüllt", () => {
    setSprache("de");
    const deutsch = t("app.settings");
    setSprache("en");
    const englisch = t("app.settings");
    expect(deutsch).toBe("Einstellungen");
    expect(englisch).toBe("Settings");
  });

  /// Jeder Schlüssel muss **alle vier** Sprachen haben — sonst steht im
  /// Fenster plötzlich Deutsch, wo Spanisch eingestellt ist.
  it("alle Texte sind in allen vier Sprachen vorhanden", async () => {
    const texte = (await import("../i18n/texte.json")).default as Record<
      string,
      Record<string, string>
    >;
    const fehlend: string[] = [];
    for (const [schluessel, sprachen] of Object.entries(texte)) {
      for (const s of ["de", "en", "es", "fr"]) {
        if (!sprachen[s] || !sprachen[s].trim()) fehlend.push(`${schluessel}/${s}`);
      }
    }
    expect(fehlend).toEqual([]);
  });

  /// Platzhalter müssen beim Füllen verschwinden — ein stehengebliebenes `{}`
  /// ist im Fenster sichtbar.
  it("Platzhalter werden gefüllt", () => {
    setSprache("de");
    const text = t("rolle.administrator");
    expect(text).not.toContain("{}");
    expect(text.length).toBeGreaterThan(0);
  });

  it("ein unbekannter Schlüssel liefert den Schlüssel selbst", () => {
    setSprache("de");
    // Lieber ein sichtbarer Schlüssel als ein leerer Platz im Fenster.
    expect(t("gibt-es-nicht")).toBe("gibt-es-nicht");
  });
});

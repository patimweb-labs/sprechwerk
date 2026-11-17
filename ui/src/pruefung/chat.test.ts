import { describe, expect, it } from "vitest";
import { ersterLink, sichtbar, teile, teilchen } from "../chat";
import type { Nachricht } from "../api";

describe("Links im Text", () => {
  it("findet Links und lässt den Rest als Text", () => {
    expect(teile("Siehe https://verein.example/fest und dann")).toEqual([
      { art: "text", text: "Siehe " },
      { art: "link", text: "https://verein.example/fest", url: "https://verein.example/fest" },
      { art: "text", text: " und dann" },
    ]);
  });

  it("Satzzeichen am Ende gehören zum Satz", () => {
    for (const [text, url] of [
      ["Hier: https://a.example/x.", "https://a.example/x"],
      ["(https://a.example/x)", "https://a.example/x"],
      ["Echt? https://a.example/?q=1!", "https://a.example/?q=1"],
      ["https://de.wikipedia.org/wiki/Verein_(Recht).", "https://de.wikipedia.org/wiki/Verein_(Recht)"],
    ]) {
      expect(ersterLink(text), text).toBe(url);
    }
  });

  it("nur http und https, nie javascript:", () => {
    expect(ersterLink("javascript:alert(1) und ftp://x.example")).toBeNull();
    expect(ersterLink("http:// leer")).toBeNull();
    expect(teile("kein Link hier")).toEqual([{ art: "text", text: "kein Link hier" }]);
    expect(teile("")).toEqual([]);
  });

  it("mehrere Links, der erste zählt für die Vorschau", () => {
    const s = teile("https://a.example https://b.example");
    expect(s.filter((x) => x.art === "link")).toHaveLength(2);
    expect(ersterLink("https://a.example https://b.example")).toBe("https://a.example");
  });
});

describe("Verlauf", () => {
  const n = (id: string, inhalt: Nachricht["inhalt"]): Nachricht => ({
    id,
    verfasser: "m",
    verfasser_name: "Ada",
    inhalt,
    zeit: 0,
    geloescht: false,
    loeschbar: false,
    reaktionen: [],
  });

  it("Reaktionen sind keine Beiträge", () => {
    const liste = [
      n("a", { zustand: "text", text: "Hallo" }),
      n("b", { zustand: "reaktion", ziel: "a", zeichen: "👍", weg: false }),
      n("c", { zustand: "wartet" }),
    ];
    expect(sichtbar(liste).map((x) => x.id)).toEqual(["a", "c"]);
  });
});

describe("Teilchen", () => {
  it("sind aus der Nummer reproduzierbar und bleiben im Bild", () => {
    expect(teilchen(3, 20)).toEqual(teilchen(3, 20));
    expect(teilchen(3, 20)).not.toEqual(teilchen(4, 20));
    for (const t of teilchen(7, 200)) {
      expect(t.x).toBeGreaterThanOrEqual(0);
      expect(t.x).toBeLessThanOrEqual(100);
      expect(t.dauer).toBeGreaterThan(1);
      expect(t.farbe).toBeGreaterThanOrEqual(0);
      expect(t.farbe).toBeLessThan(5);
    }
  });
});

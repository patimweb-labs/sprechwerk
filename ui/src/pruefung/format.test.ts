// Die Kennung ist das, was eine Person auf jedem Gerät ist. Sie muss lesbar
// sein — und genau so aussehen wie der Fingerabdruck, den der Server druckt.
import { describe, expect, it } from "vitest";
import { kennungGruppiert } from "../format";

describe("Kennung lesbar machen", () => {
  it("gruppiert zu vier Zeichen", () => {
    expect(kennungGruppiert("3ff05a78eb5d4caa", 16)).toBe("3ff0:5a78:eb5d:4caa");
  });

  it("kürzt lange Kennungen und zeigt das an", () => {
    const lang = "ab".repeat(32); // 64 Zeichen, wie ein echter Schlüssel
    const text = kennungGruppiert(lang, 16);
    expect(text.endsWith("…")).toBe(true);
    expect(text.length).toBeLessThan(25);
  });

  it("nimmt auch große Buchstaben und trennt selbst", () => {
    expect(kennungGruppiert("AB-CD-EF-01", 8)).toBe("ABCD:EF01");
  });

  it("ohne Kennung steht ein Strich, nicht nichts", () => {
    // Ein leerer Platz im Fenster sieht aus wie ein Fehler.
    expect(kennungGruppiert("")).toBe("—");
  });
});

import { zuletztText } from "../format";

describe("zuletzt verbunden", () => {
  const jetzt = Date.parse("2026-11-13T12:00:00");
  const vor = (ms: number) => jetzt - ms;

  it("ohne Zeit kein Text", () => {
    expect(zuletztText(0, jetzt)).toBeNull();
    expect(zuletztText(null, jetzt)).toBeNull();
  });

  it("sagt die Größenordnung, nicht die Sekunde", () => {
    expect(zuletztText(vor(10_000), jetzt)).toBe("gerade eben");
    expect(zuletztText(vor(5 * 60_000), jetzt)).toBe("vor 5 Minuten");
    expect(zuletztText(vor(3 * 3_600_000), jetzt)).toBe("vor 3 Stunden");
    expect(zuletztText(vor(4 * 24 * 3_600_000), jetzt)).toBe("vor 4 Tagen");
    expect(zuletztText(vor(90 * 24 * 3_600_000), jetzt)).toBe("vor 3 Monaten");
    expect(zuletztText(vor(800 * 24 * 3_600_000), jetzt)).toBe("vor 2 Jahren");
  });

  it("eine Zeit in der Zukunft ist gerade eben, nicht negativ", () => {
    // Uhren gehen falsch; „vor -3 Minuten" wäre ein Fehler im Fenster.
    expect(zuletztText(jetzt + 180_000, jetzt)).toBe("gerade eben");
  });
});

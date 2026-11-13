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

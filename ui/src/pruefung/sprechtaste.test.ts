// Push-to-Talk darf das Tippen nicht stören, und jede Taste, die man in den
// Einstellungen aufnehmen kann, muss auch funken.
import { describe, expect, it } from "vitest";
import {
  codeFuer,
  istSprechtaste,
  istSprechtasteLos,
  schreibfeld,
  sprechMaustaste,
  tasteName,
} from "../sprechtaste";

const feld = { tagName: "INPUT", type: "text" };
const seite = { tagName: "DIV" };
const leer = { aktiv: true, taste: "Leertaste" };

describe("Sprechtaste", () => {
  it("Push-to-Talk aus: keine Taste wird abgefangen", () => {
    // Vorher schluckte die Leertaste auch dann jeden Druck.
    const aus = { aktiv: false, taste: "Leertaste" };
    expect(istSprechtaste(aus, { code: "Space", ziel: seite })).toBe(false);
    expect(istSprechtasteLos(aus, "Space")).toBe(false);
    expect(sprechMaustaste({ aktiv: false, taste: "Maus 4" })).toBeNull();
  });

  it("Leertaste funkt außerhalb von Textfeldern", () => {
    expect(istSprechtaste(leer, { code: "Space", ziel: seite })).toBe(true);
    expect(istSprechtaste(leer, { code: "Space", ziel: null })).toBe(true);
  });

  it("Leertaste und Buchstaben gehören im Chatfeld dem Text", () => {
    expect(istSprechtaste(leer, { code: "Space", ziel: feld })).toBe(false);
    expect(istSprechtaste(leer, { code: "Space", ziel: { tagName: "TEXTAREA" } })).toBe(false);
    expect(
      istSprechtaste(leer, { code: "Space", ziel: { tagName: "DIV", isContentEditable: true } }),
    ).toBe(false);
    expect(istSprechtaste({ aktiv: true, taste: "V" }, { code: "KeyV", ziel: feld })).toBe(false);
  });

  it("Strg, Alt und F-Tasten funken auch aus dem Textfeld", () => {
    expect(istSprechtaste({ aktiv: true, taste: "Strg links" }, { code: "ControlLeft", ziel: feld })).toBe(true);
    expect(istSprechtaste({ aktiv: true, taste: "Alt links" }, { code: "AltLeft", ziel: feld })).toBe(true);
    expect(istSprechtaste({ aktiv: true, taste: "F13" }, { code: "F13", ziel: feld })).toBe(true);
  });

  it("jede aufnehmbare Taste wird auch erkannt", () => {
    // Vorher kannte die Erkennung nur Leertaste, Strg links und Alt links.
    for (const code of ["ControlRight", "AltRight", "ShiftLeft", "CapsLock", "KeyV", "Digit4", "F5", "Numpad0", "Backquote"]) {
      const name = tasteName(code);
      expect(codeFuer(name)).toBe(code);
      expect(istSprechtaste({ aktiv: true, taste: name }, { code, ziel: seite })).toBe(true);
    }
    expect(tasteName("Space")).toBe("Leertaste");
    expect(tasteName("KeyV")).toBe("V");
  });

  it("Loslassen beendet auch im Textfeld", () => {
    // Gedrückt außerhalb, losgelassen im Feld: sonst sendete man weiter.
    expect(istSprechtasteLos(leer, "Space")).toBe(true);
    expect(istSprechtasteLos(leer, "KeyA")).toBe(false);
  });

  it("Maustasten zählen nur als Maustasten", () => {
    const maus = { aktiv: true, taste: "Maus 4" };
    expect(sprechMaustaste(maus)).toBe(3);
    expect(sprechMaustaste(leer)).toBeNull();
    expect(istSprechtaste(maus, { code: "Space", ziel: seite })).toBe(false);
  });

  it("Kontrollkästchen und Knöpfe sind kein Schreibfeld", () => {
    expect(schreibfeld({ tagName: "INPUT", type: "checkbox" })).toBe(false);
    expect(schreibfeld({ tagName: "BUTTON" })).toBe(false);
    expect(schreibfeld({ tagName: "INPUT" })).toBe(true);
    expect(schreibfeld({ tagName: "input", type: "search" })).toBe(true);
  });
});

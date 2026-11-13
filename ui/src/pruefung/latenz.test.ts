// Die Umlaufzeit steht in der Statuszeile, weil sie die zweite Hälfte der
// Qualitätsfrage beantwortet: TeamSpeak zeigt dort ein Symbol, hier die Zahl.
import { describe, expect, it, beforeEach } from "vitest";
import { latenzText, modell } from "../store";

describe("Umlaufzeit", () => {
  beforeEach(() => {
    modell.latenz_ms = null;
  });

  it("schweigt, solange nichts gemessen wurde", () => {
    expect(latenzText.value).toBeNull();
  });

  it("zeigt kurze Zeiten in Millisekunden", () => {
    modell.latenz_ms = 8;
    expect(latenzText.value).toBe("8 ms");
    modell.latenz_ms = 999;
    expect(latenzText.value).toBe("999 ms");
  });

  it("wechselt ab einer Sekunde auf Sekunden mit Komma", () => {
    // „1240 ms" liest niemand gern.
    modell.latenz_ms = 1240;
    expect(latenzText.value).toBe("1,2 s");
    modell.latenz_ms = 10_000;
    expect(latenzText.value).toBe("10,0 s");
  });

  it("0 ms wird angezeigt, nicht verschluckt", () => {
    // Loopback kann 0 ms ergeben — das ist ein Wert, kein fehlender Wert.
    modell.latenz_ms = 0;
    expect(latenzText.value).toBe("0 ms");
  });
});

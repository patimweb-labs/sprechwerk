// „0 verworfen" ist die ehrlichste Auskunft, die ein Sprachserver geben kann —
// und TeamSpeak zeigt an dieser Stelle nur ein Symbol. Hier steht die Zahl.
import { describe, expect, it, beforeEach } from "vitest";
import { modell, qualitaet } from "../store";

describe("Verbindungsqualität", () => {
  beforeEach(() => {
    modell.guete = null;
  });

  it("schweigt, solange nichts abgefragt wurde", () => {
    expect(qualitaet.value).toBeNull();
  });

  it("sagt die gute Nachricht deutlich", () => {
    modell.guete = { sprachrahmen: 800, verteilt: 5596, verloren: 0, abgelehnt: 0, verbundene: 4 };
    expect(qualitaet.value?.tone).toBe("ok");
    expect(qualitaet.value?.text).toBe("keine Rahmen verloren");
  });

  it("nennt die Zahl, wenn etwas verloren ging", () => {
    modell.guete = { sprachrahmen: 900, verteilt: 5000, verloren: 3, abgelehnt: 1, verbundene: 4 };
    expect(qualitaet.value?.tone).toBe("warn");
    expect(qualitaet.value?.text).toBe("3 Rahmen verloren");
  });

  it("Verworfene und Abgelehnte werden nicht verwechselt", () => {
    // Abgelehnt heißt: der Server hat nicht weitergeleitet, weil etwas nicht
    // stimmte (Recht, Raum, kaputter Rahmen). Das ist kein Netzverlust.
    modell.guete = { sprachrahmen: 10, verteilt: 20, verloren: 0, abgelehnt: 7, verbundene: 2 };
    expect(qualitaet.value?.tone).toBe("ok");
  });
});

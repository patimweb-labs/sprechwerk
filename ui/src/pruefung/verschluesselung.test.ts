// Das Schloss in der Raumliste ist klein. Wer gerade spricht, soll erfahren,
// dass ab jetzt Ende zu Ende verschlüsselt wird — sonst ändert sich das
// Verhalten, ohne dass es jemand merkt.
import { describe, expect, it, beforeEach } from "vitest";
import { nextTick } from "vue";
import { beobachteVerschluesselung, modell } from "../store";

function raum(verschluesselt: boolean) {
  return { id: "raum_1", name: "Geheim", anwesend: 1, hier: true, verschluesselt };
}

describe("Verschlüsselung umschalten", () => {
  beforeEach(() => {
    modell.meldung = null;
    modell.raeume = [];
  });

  it("meldet, wenn der eigene Raum verschlüsselt wird", async () => {
    beobachteVerschluesselung();
    modell.raeume = [raum(false)];
    await nextTick();
    modell.raeume = [raum(true)];
    await nextTick();
    expect(modell.meldung).toContain("Ende zu Ende verschlüsselt");
  });

  it("meldet auch das Ausschalten", async () => {
    beobachteVerschluesselung();
    modell.raeume = [raum(true)];
    await nextTick();
    modell.raeume = [raum(false)];
    await nextTick();
    expect(modell.meldung).toContain("Verschlüsselung aus");
  });

  it("schweigt beim ersten Aufbau (kein Wechsel)", async () => {
    beobachteVerschluesselung();
    modell.raeume = [raum(true)];
    await nextTick();
    expect(modell.meldung).toBeNull();
  });
});

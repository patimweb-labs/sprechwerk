// Vertrag mit `@weppelmann/tokens`: jede `var(--wp-…)` in der Oberfläche muss
// es dort auch geben.
//
// Warum ein Test? Ein unbekanntes Token ist kein Fehler, den irgendwer meldet:
// der Browser verwirft die Deklaration still, `vue-tsc` und `vite build` sind
// grün. So standen hier jahrelang `--wp-color-text`, `--wp-color-border`,
// `--wp-font-heading` & Co. — Namen, die die Bibliothek nie hatte. Die
// Überschriften der Sperr- und Protokollansicht fielen auf die Browserschrift
// zurück, Trennlinien und Warnfarbe fehlten, in allen drei Themes.
import { describe, expect, it } from "vitest";

// Dieselbe Datei, die `vite.config.ts` als `@weppelmann/tokens-css` einbindet.
const tokenDateien = import.meta.glob<string>(
  "../../../../weppelmann-ui/packages/tokens/dist/css/vars.css",
  { query: "?raw", import: "default", eager: true },
);
const quellen = import.meta.glob<string>(["../**/*.vue", "../**/*.css"], {
  query: "?raw",
  import: "default",
  eager: true,
});

function definiert(text: string): Set<string> {
  return new Set([...text.matchAll(/(--wp-[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
}

describe("Tokens", () => {
  it("die Token-Datei ist da (sonst prüft dieser Test nichts)", () => {
    expect(Object.keys(tokenDateien)).toHaveLength(1);
    expect(definiert(Object.values(tokenDateien)[0]).size).toBeGreaterThan(50);
  });

  it("jede verwendete --wp-Variable ist definiert", () => {
    const bekannt = definiert(Object.values(tokenDateien).join("\n"));
    // Was die Oberfläche selbst setzt, darf sie auch lesen.
    for (const text of Object.values(quellen)) {
      for (const name of definiert(text)) bekannt.add(name);
    }
    const unbekannt: string[] = [];
    for (const [datei, text] of Object.entries(quellen)) {
      for (const m of text.matchAll(/var\(\s*(--wp-[a-z0-9-]+)/g)) {
        if (!bekannt.has(m[1])) unbekannt.push(`${datei}: ${m[1]}`);
      }
    }
    expect(unbekannt).toEqual([]);
  });

  it("keine px-Werte in den Stilen (AGENTS.md §2.6)", () => {
    // Ausnahme: `@media`. Dort wertet CSS keine Variablen aus — eine
    // Umbruchbreite kann kein Token sein.
    const funde: string[] = [];
    for (const [datei, text] of Object.entries(quellen)) {
      const stil = text.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? (datei.endsWith(".css") ? text : "");
      stil.split("\n").forEach((zeile, i) => {
        if (/\b\d+px\b/.test(zeile) && !zeile.includes("@media")) {
          funde.push(`${datei}:${i + 1}: ${zeile.trim()}`);
        }
      });
    }
    expect(funde).toEqual([]);
  });
});

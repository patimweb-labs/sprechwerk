// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Patrick Weppelmann
//
// Screenshots **erzeugen**, nicht ablegen (AGENTS.md §7).
//
// Startet die Oberfläche mit dem Klickdummy (`src/mock.ts`, kein Server, keine
// Soundkarte), klickt sich wie ein Mensch durch die Ansichten und nimmt jede in
// allen drei Themes auf. Die README zeigt dark und light; mono liegt daneben,
// damit die Prüfung „alle drei Themes" (AGENTS.md §5) ein Bild hat.
//
//   npm --prefix ui run bilder              → docs/bilder/*.png
//   SPRECHWERK_CHROMIUM=/pfad/zu/chrome …   anderer Browser
//
// Browser: `playwright-core` lädt keinen Browser herunter. Genommen wird, in
// dieser Reihenfolge, `SPRECHWERK_CHROMIUM`, der Browser aus dem
// Playwright-Zwischenspeicher, ein Chromium/Chrome aus dem `PATH`.

import { createServer } from "vite";
import { chromium } from "playwright-core";
import { existsSync, mkdirSync } from "node:fs";
import { delimiter, join } from "node:path";
import { fileURLToPath } from "node:url";

const UI = fileURLToPath(new URL("..", import.meta.url));
const ZIEL = fileURLToPath(new URL("../../docs/bilder/", import.meta.url));
const THEMES = ["dark", "light", "mono"];
const GROESSE = { width: 1280, height: 800 };

function browserPfad() {
  if (process.env.SPRECHWERK_CHROMIUM) return process.env.SPRECHWERK_CHROMIUM;
  try {
    const eigen = chromium.executablePath();
    if (existsSync(eigen)) return eigen;
  } catch {
    // kein Browser im Zwischenspeicher — weiter im PATH suchen
  }
  const namen = ["chromium", "chromium-browser", "google-chrome", "chrome"];
  for (const ordner of (process.env.PATH ?? "").split(delimiter)) {
    for (const name of namen) {
      for (const datei of [name, `${name}.exe`]) {
        const pfad = join(ordner, datei);
        if (existsSync(pfad)) return pfad;
      }
    }
  }
  throw new Error("Kein Chromium gefunden — SPRECHWERK_CHROMIUM setzen.");
}

/** Die Ansichten in der Reihenfolge, in der ein Mensch sie erreicht. */
const ANSICHTEN = [
  // Der Dummy startet verbunden, im Trainingsraum — wie nach dem Verbinden.
  {
    name: "raeume",
    hin: async (seite) => {
      await seite.getByText("Trainingsraum").first().waitFor();
      await namenSichtbar(seite);
    },
  },
  { name: "protokoll", hin: (seite) => ueberMenue(seite, "Protokoll") },
  { name: "sperren", hin: (seite) => ueberMenue(seite, "Sperren") },
  { name: "einstellungen", hin: (seite) => ueberMenue(seite, "Einstellungen") },
  { name: "server", hin: (seite) => ueberMenue(seite, "Serverliste") },
];

/**
 * Jeder Name in der Raumliste muss zu sehen sein — ohne Überfahren.
 *
 * Das prüft nur ein echter Browser (jsdom wendet die Stile einer `.vue`-Datei
 * nicht an). Gefunden 2026-11-15: eine Regel `.mensch > :last-child
 * { opacity: 0 }` für das ⋯-Menü versteckte den Namen, wo es kein Menü gab.
 */
async function namenSichtbar(seite) {
  const unsichtbar = await seite.evaluate(() =>
    [...document.querySelectorAll(".mensch__text")]
      .filter((el) => {
        let e = el;
        for (; e; e = e.parentElement) {
          if (Number(getComputedStyle(e).opacity) === 0) return true;
        }
        return false;
      })
      .map((el) => el.textContent?.trim()),
  );
  if (unsichtbar.length > 0) {
    throw new Error(`Namen in der Raumliste unsichtbar: ${unsichtbar.join(", ")}`);
  }
}

async function ueberMenue(seite, eintrag) {
  // Das Kontomenü oben rechts — über seinen Namen, wie ein Bildschirmleser.
  await seite.locator("[aria-haspopup='menu'][aria-label='Kontomenü']").click();
  const menue = seite.getByRole("menu");
  await menue.waitFor();
  // `WpMenu` nimmt in den ersten 250 ms nach dem Öffnen keine Wahl an (damit
  // die Geste, die ein Kontextmenü öffnet, nicht gleich einen Eintrag trifft).
  await seite.waitForTimeout(300);
  await seite.getByRole("menuitem", { name: eintrag }).click();
  await menue.waitFor({ state: "detached" });
}

const server = await createServer({
  root: UI,
  logLevel: "warn",
  server: { port: 0, strictPort: false, host: "127.0.0.1" },
});
await server.listen();
const adresse = server.resolvedUrls?.local[0];
if (!adresse) throw new Error("Vite nennt keine Adresse");

const browser = await chromium.launch({ executablePath: browserPfad() });
mkdirSync(ZIEL, { recursive: true });
let fehler = 0;
try {
  for (const thema of THEMES) {
    // Eine frische Seite je Theme: der Dummy fängt wieder „nicht verbunden" an.
    // Weniger Bewegung: der Sprechpuls am Avatar steht dann still (die
    // Bibliothek zeigt stattdessen einen festen Ring) — sonst fiele jedes Bild
    // je nach Augenblick anders aus.
    const seite = await browser.newPage({
      viewport: GROESSE,
      deviceScaleFactor: 1,
      reducedMotion: "reduce",
    });
    seite.on("pageerror", (e) => {
      fehler += 1;
      console.error(`[${thema}] Seitenfehler: ${e.message}`);
    });
    // Feste Uhrzeit: der Dummy rechnet „vor 42 Minuten" ab jetzt. Ohne das
    // wäre jedes Bild bei jedem Lauf ein anderes — und jeder Commit trüge
    // fünfzehn geänderte PNGs. Die Uhr läuft danach normal weiter.
    await seite.clock.install({ time: new Date("2026-11-14T19:30:00+01:00") });
    await seite.goto(`${adresse}?mock=1`);
    // Das Theme so setzen, wie die App es tut (`wendeThema`): als Daten am Root.
    await seite.evaluate((t) => (document.documentElement.dataset.theme = t), thema);
    for (const ansicht of ANSICHTEN) {
      await ansicht.hin(seite);
      await seite.evaluate(() => document.fonts.ready);
      // Kurz warten, bis Übergänge stehen (die Oberfläche blendet weich).
      await seite.waitForTimeout(250);
      const datei = join(ZIEL, `${ansicht.name}-${thema}.png`);
      await seite.screenshot({ path: datei });
      console.log(`  ${ansicht.name}-${thema}.png`);
    }
    await seite.close();
  }
} finally {
  await browser.close();
  await server.close();
}
if (fehler > 0) {
  console.error(`${fehler} Seitenfehler — die Bilder zeigen womöglich nicht, was sie sollen.`);
  process.exit(1);
}

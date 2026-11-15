// Prüfstand für die Oberfläche.
//
// Ohne diese Datei gäbe es für `ui/` nur eine Typprüfung (`vue-tsc`) — die
// fängt Tippfehler in Typen, aber keinen Template-Fehler zur Laufzeit und
// keine falsche Reaktion auf einen Klick. Die Ansichten laufen hier mit dem
// eingebauten Klickdummy (`src/mock.ts`), also ohne Fenster, ohne Server,
// ohne Soundkarte.
import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  // Wie in `vite.config.ts`: die Token-Datei liegt in `weppelmann-ui` und wird
  // von `pruefung/tokens.test.ts` gelesen.
  server: { fs: { allow: ["..", "../../weppelmann-ui"] } },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // Die Token-Datei als Text lesen dürfen (sonst liefert Vitest CSS leer aus).
    css: { include: [/vars\.css/] },
    // Kein Netz, keine Fenster-API: `api.ts` erkennt das und nimmt den Dummy.
    env: { VITE_MOCK: "1" },
  },
});

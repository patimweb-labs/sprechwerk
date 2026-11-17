// Bau der Verwaltungsoberfläche (`verwaltung.html`) für den Server.
//
// Eigene Ausgabe `dist-verwaltung/`: `dist/` gehört dem Fenster (Tauri liest
// es). `crates/server/build.rs` bettet `dist-verwaltung/` in das Programm ein.
//
//   npm run verwaltung        bauen (danach `cargo build -p sprechwerk-server`)
//   npm run verwaltung:dev    entwickeln gegen einen laufenden Server (7444)
import { defineConfig, mergeConfig } from "vite";
import basis from "./vite.config";

export default mergeConfig(
  basis,
  defineConfig({
    build: {
      outDir: "dist-verwaltung",
      emptyOutDir: true,
      rollupOptions: { input: "verwaltung.html" },
    },
    server: {
      // 1423 ist das Fenster; die Verwaltung daneben.
      port: 1424,
      open: "/verwaltung.html",
      proxy: {
        "/api": "http://127.0.0.1:7444",
        "/anmelden": "http://127.0.0.1:7444",
      },
    },
  }),
);

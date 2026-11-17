// Einstieg der Verwaltung im Browser (`ui/verwaltung.html`).
//
// Dieselben Stile wie das Fenster (`src/main.ts`): Komponenten, Tokens,
// Schriften, Grundfläche — die Verwaltung soll aussehen wie ein Teil der App.
import { createApp } from "vue";
import "@weppelmann/components/style.css";
import "@weppelmann/tokens-css";
import "@weppelmann/tokens-fonts";
import "../base.css";
import VerwaltungApp from "./VerwaltungApp.vue";
import { vorlieben } from "./store";

vorlieben();
createApp(VerwaltungApp).mount("#app");

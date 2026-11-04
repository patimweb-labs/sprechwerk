import { createApp } from "vue";
import "@weppelmann/components/style.css";
import "@weppelmann/tokens-css";
import "@weppelmann/tokens-fonts";
import "./base.css";
import App from "./App.vue";
import { ladeEinstellungen, verbindeEreignisse } from "./store";

// Erst die Einstellungen (Thema, Name, Server), dann das Fenster — sonst
// blitzt die Vorgabe auf.
ladeEinstellungen();
verbindeEreignisse();

// Fehler sichtbar machen: im Fenster gibt es keine Konsole, die man öffnen
// könnte. Ein Renderfehler soll nicht als „Klick tut nichts" enden.
window.addEventListener("error", (ereignis) => {
  document.title = `FEHLER: ${ereignis.message}`;
});
window.addEventListener("unhandledrejection", (ereignis) => {
  document.title = `FEHLER: ${String(ereignis.reason)}`;
});

const app = createApp(App);
app.config.errorHandler = (fehler) => {
  document.title = `FEHLER: ${String(fehler)}`;
};
app.mount("#app");

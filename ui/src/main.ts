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

createApp(App).mount("#app");

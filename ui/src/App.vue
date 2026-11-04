<script setup lang="ts">
// Fensteraufbau: links die Räume und Mitglieder, rechts der Verlauf des
// aktuellen Raums mit Eingabe. Kein Zustand in der Darstellung, den das Modell
// nicht kennt (AGENTS.md §2.1).
//
// Sichtbarkeit folgt aus dem Modell: „Raum anlegen" erscheint nur, wenn die
// Rolle es erlaubt; „Protokoll" nur mit Leserecht.
import { computed, nextTick, ref, watch } from "vue";
import {
  darfProtokollLesen,
  darfRaeumeVerwalten,
  einstellungen,
  holeProtokoll,
  laeuft,
  legeRaumAn,
  loesche,
  meinRaum,
  modell,
  sende,
  trennen,
  uhrzeit,
  verbinden,
  verbunden,
  betreten,
  verlassen,
  wendeThema,
} from "./store";

const eingabe = ref("");
const neuerRaum = ref("");
const verlauf = ref<HTMLDivElement | null>(null);
const zeigeProtokoll = ref(false);

/** Wer sitzt im selben Raum wie ich? */
const anwesend = computed(() => modell.mitglieder.filter((m) => m.raum && m.raum === meinRaum.value?.id));

/** Verbindungszustand als Text. */
const statusText = computed(() => {
  const v = modell.verbindung;
  if (!v) return "…";
  switch (v.art) {
    case "getrennt":
      return "getrennt";
    case "verbindet":
      return `verbinde zu ${v.server} …`;
    case "verbunden":
      return `${v.server_name} · ${v.name} · ${v.rolle}`;
    case "fehler":
      return `Fehler: ${v.text}`;
  }
});

// Nach jeder neuen Nachricht nach unten scrollen.
watch(
  () => modell.nachrichten.length,
  async () => {
    await nextTick();
    if (verlauf.value) verlauf.value.scrollTop = verlauf.value.scrollHeight;
  },
);

async function absenden() {
  const text = eingabe.value;
  eingabe.value = "";
  await sende(text);
}

async function raumAnlegen() {
  const name = neuerRaum.value;
  neuerRaum.value = "";
  await legeRaumAn(name, meinRaum.value?.id ?? null);
}

async function umschaltenProtokoll() {
  zeigeProtokoll.value = !zeigeProtokoll.value;
  if (zeigeProtokoll.value) await holeProtokoll();
}
</script>

<template>
  <div class="fenster">
    <!-- Kopfzeile -->
    <header class="kopf">
      <strong class="marke">Sprechwerk</strong>
      <span class="status">{{ statusText }}</span>
      <span class="rechts">
        <button v-if="darfProtokollLesen" class="knopf" @click="umschaltenProtokoll">
          {{ zeigeProtokoll ? "Verlauf" : "Protokoll" }}
        </button>
        <button
          class="knopf"
          :title="'Farbschema: ' + einstellungen.thema"
          @click="wendeThema(einstellungen.thema === 'dark' ? 'light' : einstellungen.thema === 'light' ? 'mono' : 'dark')"
        >
          {{ einstellungen.thema }}
        </button>
        <button v-if="verbunden" class="knopf" @click="trennen">Trennen</button>
      </span>
    </header>

    <!-- Getrennt: Anmeldeformular -->
    <main v-if="!verbunden" class="anmeldung">
      <label>
        <span>Server</span>
        <input v-model="einstellungen.server" placeholder="127.0.0.1:7443" />
      </label>
      <label>
        <span>Name</span>
        <input v-model="einstellungen.name" placeholder="Wie heißt du?" />
      </label>
      <label>
        <span>Serverpasswort (falls nötig)</span>
        <input v-model="einstellungen.passwort" type="password" />
      </label>
      <button class="knopf haupt" :disabled="laeuft || !einstellungen.name" @click="verbinden">
        {{ laeuft ? "verbinde …" : "Verbinden" }}
      </button>
      <p v-if="modell.meldung" class="meldung">{{ modell.meldung }}</p>
    </main>

    <!-- Verbunden: Räume links, Verlauf rechts -->
    <main v-else class="inhalt">
      <aside class="seite">
        <div class="abschnitt">
          <h2>Räume</h2>
          <ul class="liste">
            <li
              v-for="raum in modell.raeume"
              :key="raum.id"
              class="eintrag"
              :class="{ hier: raum.hier, tief: raum.tiefe > 0 }"
              :style="{ paddingLeft: `calc(var(--wp-space-2) + ${raum.tiefe} * var(--wp-space-3))` }"
              @click="betreten(raum.id)"
            >
              <span class="name">{{ raum.name }}</span>
              <span class="zahl">{{ raum.anwesend }}</span>
            </li>
          </ul>
          <button v-if="meinRaum" class="knopf klein" @click="verlassen">Raum verlassen</button>
        </div>

        <div class="abschnitt">
          <h2>Hier</h2>
          <ul class="liste">
            <li v-for="m in anwesend" :key="m.id" class="eintrag" :class="{ ich: m.ich }">
              <span class="name">{{ m.name }}<em v-if="m.spricht" class="spricht"> ●</em></span>
              <span class="rolle">{{ m.rolle }}</span>
            </li>
            <li v-if="anwesend.length === 0" class="leer">niemand hier</li>
          </ul>
        </div>

        <div v-if="darfRaeumeVerwalten" class="abschnitt">
          <h2>Raum anlegen</h2>
          <div class="zeile">
            <input v-model="neuerRaum" placeholder="Name" @keyup.enter="raumAnlegen" />
            <button class="knopf klein" @click="raumAnlegen">+</button>
          </div>
        </div>
      </aside>

      <section class="haupt">
        <!-- Prüfprotokoll -->
        <div v-if="zeigeProtokoll" class="protokoll">
          <table>
            <thead>
              <tr><th>Zeit</th><th>Wer</th><th>Aktion</th><th>Ziel</th><th>Ergebnis</th></tr>
            </thead>
            <tbody>
              <tr v-for="e in modell.protokoll" :key="e.laufnummer">
                <td>{{ uhrzeit(e.zeit) }}</td>
                <td>{{ e.handelnder_name }}</td>
                <td>{{ e.aktion }}</td>
                <td class="kurz">{{ e.ziel }}</td>
                <td>{{ e.ergebnis }}</td>
              </tr>
            </tbody>
          </table>
          <p v-if="modell.protokoll.length === 0" class="leer">keine Einträge</p>
        </div>

        <!-- Verlauf -->
        <div v-else ref="verlauf" class="verlauf">
          <p v-if="!meinRaum" class="leer">Wähle links einen Raum.</p>
          <template v-else>
            <p v-if="modell.nachrichten.length === 0" class="leer">Noch nichts geschrieben.</p>
            <article v-for="n in modell.nachrichten" :key="n.id" class="nachricht">
              <header>
                <span class="wer">{{ n.verfasser_name }}</span>
                <span class="wann">{{ uhrzeit(n.zeit) }}</span>
                <button v-if="n.loeschbar" class="knopf winzig" @click="loesche(n.id)">löschen</button>
              </header>
              <p v-if="n.geloescht" class="geloescht">(Nachricht gelöscht)</p>
              <p v-else class="text">{{ n.text }}</p>
            </article>
          </template>
        </div>

        <!-- Eingabe -->
        <form v-if="meinRaum" class="eingabe" @submit.prevent="absenden">
          <input v-model="eingabe" placeholder="Schreiben …" />
          <button class="knopf" type="submit">Senden</button>
        </form>
        <p v-if="modell.meldung" class="meldung">{{ modell.meldung }}</p>
      </section>
    </main>
  </div>
</template>

<style scoped>
.fenster {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.kopf {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  padding: var(--wp-space-2) var(--wp-space-4);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
}

.marke {
  font-family: var(--wp-font-family-mono);
  letter-spacing: 0.04em;
}

.status {
  color: var(--wp-color-text-muted);
  font-size: var(--wp-font-size-sm);
}

.rechts {
  margin-left: auto;
  display: flex;
  gap: var(--wp-space-2);
}

.anmeldung {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
  max-width: 24rem;
  margin: var(--wp-space-8) auto;
  padding: var(--wp-space-4);
}

label {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
}

input {
  font: inherit;
  color: var(--wp-color-text-default);
  background: var(--wp-color-bg-panel);
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  border-radius: 0;
  padding: var(--wp-space-2);
}

.knopf {
  font: inherit;
  color: var(--wp-color-text-default);
  background: var(--wp-color-bg-raised);
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  border-radius: 0;
  padding: var(--wp-space-1) var(--wp-space-3);
  cursor: pointer;
}

.knopf:disabled {
  opacity: 0.5;
  cursor: default;
}

.knopf.haupt {
  background: var(--wp-color-accent-default);
  color: var(--wp-color-text-on-accent);
  border-color: transparent;
  padding: var(--wp-space-2);
}

.knopf.klein {
  padding: 0 var(--wp-space-2);
  font-size: var(--wp-font-size-sm);
}

.knopf.winzig {
  padding: 0 var(--wp-space-1);
  font-size: var(--wp-font-size-xs);
}

.inhalt {
  display: grid;
  grid-template-columns: 16rem 1fr;
  min-height: 0;
  flex: 1;
}

.seite {
  border-right: var(--wp-size-border) solid var(--wp-color-border-subtle);
  overflow-y: auto;
  padding: var(--wp-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-4);
}

.abschnitt h2 {
  font-size: var(--wp-font-size-xs);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--wp-color-text-muted);
  margin: 0 0 var(--wp-space-2);
}

.liste {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.eintrag {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-1) var(--wp-space-2);
  border-radius: 0;
  cursor: pointer;
}

.eintrag:hover {
  background: var(--wp-color-bg-raised);
}

.eintrag.hier {
  background: var(--wp-color-bg-raised);
  outline: 1px solid var(--wp-color-accent-default);
}

.eintrag .zahl,
.eintrag .rolle {
  margin-left: auto;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
}

.eintrag .spricht {
  color: var(--wp-color-accent-default);
}

.zeile {
  display: flex;
  gap: var(--wp-space-1);
}

.zeile input {
  flex: 1;
}

.haupt {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.verlauf,
.protokoll {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-3) var(--wp-space-4);
}

.nachricht {
  margin-bottom: var(--wp-space-3);
}

.nachricht header {
  display: flex;
  align-items: baseline;
  gap: var(--wp-space-2);
}

.nachricht .wer {
  font-weight: 600;
}

.nachricht .wann {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
}

.nachricht .text {
  margin: var(--wp-space-1) 0 0;
  white-space: pre-wrap;
}

.nachricht .geloescht,
.leer {
  color: var(--wp-color-text-muted);
  font-style: italic;
}

.eingabe {
  display: flex;
  gap: var(--wp-space-2);
  padding: var(--wp-space-3) var(--wp-space-4);
  border-top: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.eingabe input {
  flex: 1;
}

.meldung {
  margin: 0;
  padding: var(--wp-space-2) var(--wp-space-4);
  color: var(--wp-color-status-error);
  font-size: var(--wp-font-size-sm);
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--wp-font-size-sm);
}

th, td {
  text-align: left;
  padding: var(--wp-space-1) var(--wp-space-2);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

th {
  color: var(--wp-color-text-muted);
  font-weight: 500;
}

td.kurz {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-xs);
}
</style>

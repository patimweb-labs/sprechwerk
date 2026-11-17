<script setup lang="ts">
// Räume: „Du bist hier" oben, darunter die Wechselliste. Zwei Spalten, nicht
// drei — der Raum, in dem man sitzt, ist eine Karte mit seinen Leuten; die
// anderen Räume sind eine dichte Wechselliste. Der Verlauf bekommt die Breite.
//
// Regeln, an die sich das hält:
// • Kanten sind scharf, getrennt wird mit 1-px-Linien, nicht mit Schatten.
// • Akzentfarbe nur für Zustand (hier, spricht), nicht als Dekoration.
// • Eingriffe liegen im Kontextmenü (Rechtsklick oder „…"), nie im Hauptweg.
import { computed, nextTick, ref, watch } from "vue";
import * as UI from "@weppelmann/components";
import { IconClose, IconLock, IconMore, IconPanelRight, IconPlus, IconSprechblase } from "@weppelmann/icons";
import Beitrag from "./Beitrag.vue";
import EffektBuehne from "./EffektBuehne.vue";
import { EFFEKTE, ersterLink, sichtbar } from "../chat";
import type { Effekt } from "../api";
import {
  ausRaumWerfen,
  betreten,
  darfEingreifen,
  darfRaeumeVerwalten,
  speichere,
  setzeRolle,
  darfRollenVergeben,
  einstellungen,
  entwurfsLink,
  legeRaumAn,
  meinRaum,
  modell,
  raumLoeschen,
  raumUmbenennen,
  sende,
  schluesselAnfordern,
  schluesselVorhanden,
  sperreMitglied,
  stummschalten,
  trenneMitglied,
  t,
  rolle as rollenName,
  verbunden,
  verschieben,
  vorschauVerwerfen,
} from "../store";

const eingabe = ref("");
/**
 * Effekt für die nächste Nachricht. Gehört zum Entwurf wie der Text im Feld —
 * er geht mit dem Absenden hinaus und ist danach wieder leer.
 */
const effektWahl = ref<Effekt | null>(null);
/** Was im Verlauf als Beitrag steht (Reaktionen hängen an ihrem Ziel). */
const beitraege = computed(() => sichtbar(modell.nachrichten));

const effektMenue = computed<UI.MenuItem[]>(() => [
  { id: "kein", label: t("effect.none") },
  ...EFFEKTE.map((e) => ({ id: e, label: t(`effect.${e}`) })),
]);
function effektGewaehlt(wahl: string) {
  effektWahl.value = wahl === "kein" ? null : (wahl as Effekt);
}

// Link-Vorschau: der erste Link im Entwurf geht an den Rust-Teil, der sie
// abruft. Kurz gewartet, damit nicht jeder Tastendruck mitten in der Adresse
// eine Seite anfragt.
let vorschauZeit: ReturnType<typeof setTimeout> | undefined;
watch(
  () => ersterLink(eingabe.value),
  (url) => {
    clearTimeout(vorschauZeit);
    vorschauZeit = setTimeout(() => void entwurfsLink(url), url ? 500 : 0);
  },
);
const verlauf = ref<HTMLElement | null>(null);

// ---- Menüs --------------------------------------------------------------
const offenesMitglied = ref<string | null>(null);
/**
 * Ist der Chat eingeklappt? Das ist **keine Einstellung**, sondern eine
 * Handbewegung: man klappt zu, um die Raumliste breit zu sehen, und wieder auf,
 * wenn man schreiben will. Der Zustand wird gemerkt, damit er beim nächsten
 * Start nicht überrascht — er steht aber nicht in den Einstellungen.
 */
const chatEingeklappt = ref(einstellungen.chat_eingeklappt);

/** Wie viele Nachrichten seit dem Einklappen kamen. */
const ungelesen = ref(0);
const letzteGesehen = ref(0);

// Die Einstellungen kommen erst nach dem ersten Zeichnen an (sie werden
// geladen, nicht abgewartet). Also nachziehen, wenn sie da sind.
watch(
  () => einstellungen.chat_eingeklappt,
  (zu) => {
    if (zu !== chatEingeklappt.value) chatEingeklappt.value = zu;
  },
);

function chatUmschalten() {
  chatEingeklappt.value = !chatEingeklappt.value;
  einstellungen.chat_eingeklappt = chatEingeklappt.value;
  if (!chatEingeklappt.value) {
    // Aufgeklappt: alles ist gelesen.
    ungelesen.value = 0;
    letzteGesehen.value = beitraege.value.length;
  }
  void speichere();
}

watch(
  () => beitraege.value.length,
  (anzahl) => {
    if (!chatEingeklappt.value) {
      letzteGesehen.value = anzahl;
      return;
    }
    // Nur zählen, was neu ist — nicht alles seit dem Start.
    ungelesen.value = Math.max(0, anzahl - letzteGesehen.value);
  },
);

const offenerRaum = ref<string | null>(null);

function oeffneMitgliedMenue(id: string, ereignis?: MouseEvent) {
  ereignis?.preventDefault();
  offenerRaum.value = null;
  offenesMitglied.value = id;
}

function oeffneRaumMenue(id: string, ereignis?: MouseEvent) {
  ereignis?.preventDefault();
  offenesMitglied.value = null;
  // Der Zielraum muss **vor** dem Aufbau des Menüs stehen: die Einträge hängen
  // davon ab (Schloss auf/zu).
  zielRaum.value = id;
  offenerRaum.value = id;
}

// ---- Dialoge ------------------------------------------------------------
const dialog = ref<
  null | "raum" | "umbenennen" | "verschieben" | "kicken" | "sperren" | "loeschen" | "rolle"
>(null);
/** Gewählte Rolle im Rollen-Dialog. */
const rollenWahl = ref<string>("mitglied");
// Wer Administrator werden will, muss Rolle vergeben dürfen: die Auswahl
// folgt aus dem Modell, nicht aus einer Prüfung hinterher (AGENTS.md §2.3).
const rollen = computed(() =>
  [
    { value: "administrator", label: t("rolle.administrator") },
    { value: "moderator", label: t("rolle.moderator") },
    { value: "mitglied", label: t("rolle.mitglied") },
    { value: "gast", label: t("rolle.gast") },
  ].filter((r) => r.value !== "administrator" || darfRollenVergeben.value),
);
const zielMitglied = ref<string | null>(null);
const zielRaum = ref<string | null>(null);
const eingabeName = ref("");
const eingabeGrund = ref("");
const zielRaumWahl = ref<string | null>(null);

const zielName = computed(() => modell.mitglieder.find((m) => m.id === zielMitglied.value)?.name ?? "");
const raumName = computed(() => modell.raeume.find((r) => r.id === zielRaum.value)?.name ?? "");
const raumWahl = computed(() =>
  modell.raeume.map((r) => ({ value: r.id, label: r.name })),
);

// ---- Daten --------------------------------------------------------------
const eigeneId = computed(() => {
  const v = modell.verbindung;
  return v && v.art === "verbunden" ? v.ich : null;
});


function anwesendeVon(raumId: string) {
  return modell.mitglieder.filter((m) => m.raum === raumId);
}

// ---- Menüpunkte ---------------------------------------------------------
function mitgliedMenue(id: string): UI.MenuItem[] {
  const m = modell.mitglieder.find((x) => x.id === id);
  if (!m) return [];
  const punkte: UI.MenuItem[] = [
    {
      id: "mute",
      label: m.stumm ? t("member.unmute") : t("member.mute"),
      hint: t("member.server_hint"),
    },
    {
      id: "deaf",
      label: m.taub ? t("member.undeaf") : t("member.deaf"),
      hint: t("member.server_hint"),
    },
    { id: "move", label: t("member.move"), separatorBefore: true },
  ];
  const erster = modell.raeume[0]?.id;
  if (m.raum !== erster) punkte.push({ id: "kick-room", label: t("member.kick_room") });
  // Rollen nur, wer sie vergeben darf — und dann im Dialog, nicht über einen
  // Umweg durch die Einstellungen.
  if (darfRollenVergeben.value) {
    punkte.push({ id: "role", label: t("member.role"), separatorBefore: true });
  }
  punkte.push({ id: "kick", label: t("member.kick"), separatorBefore: true, danger: true });
  punkte.push({ id: "ban", label: t("member.ban"), danger: true });
  return punkte;
}

function mitgliedMenueGewaehlt(id: string, wahl: string) {
  zielMitglied.value = id;
  offenesMitglied.value = null;
  const m = modell.mitglieder.find((x) => x.id === id);
  switch (wahl) {
    case "mute":
      void stummschalten(id, false, !(m?.stumm ?? false));
      break;
    case "deaf":
      void stummschalten(id, true, !(m?.taub ?? false));
      break;
    case "move":
      zielRaumWahl.value = m?.raum ?? null;
      dialog.value = "verschieben";
      break;
    case "kick-room":
      void ausRaumWerfen(id);
      break;
    case "role":
      rollenWahl.value = m?.rolle ?? "mitglied";
      dialog.value = "rolle";
      break;
    case "kick":
      eingabeGrund.value = "";
      dialog.value = "kicken";
      break;
    case "ban":
      eingabeGrund.value = "";
      dialog.value = "sperren";
      break;
  }
}

function raumMenuePunkte(): UI.MenuItem[] {
  const punkte: UI.MenuItem[] = [{ id: "enter", label: t("room.enter") }];
  // Verwalten nur mit Recht — wer es nicht darf, sieht es auch nicht.
  if (darfRaeumeVerwalten.value) {
    punkte.push({ id: "new", label: t("room.add"), separatorBefore: true });
    punkte.push({ id: "rename", label: t("room.rename") });
    // Kein Schalter „verschlüsseln": jeder Raum ist es (ADR 0012).
    punkte.push({ id: "delete", label: t("room.delete"), separatorBefore: true, danger: true });
  }
  return punkte;
}

function raumMenueGewaehlt(id: string, wahl: string) {
  offenerRaum.value = null;
  zielRaum.value = id;
  switch (wahl) {
    case "enter":
      void betreten(id);
      break;
    case "new":
      eingabeName.value = "";
      dialog.value = "raum";
      break;
    case "rename":
      eingabeName.value = modell.raeume.find((r) => r.id === id)?.name ?? "";
      dialog.value = "umbenennen";
      break;
    case "delete":
      dialog.value = "loeschen";
      break;
  }
}

async function bestaetigen() {
  const art = dialog.value;
  dialog.value = null;
  if (art === "raum") {
    await legeRaumAn(eingabeName.value);
  } else if (art === "umbenennen" && zielRaum.value) {
    await raumUmbenennen(zielRaum.value, eingabeName.value);
  } else if (art === "loeschen" && zielRaum.value) {
    await raumLoeschen(zielRaum.value);
  } else if (art === "verschieben" && zielMitglied.value && zielRaumWahl.value) {
    await verschieben(zielMitglied.value, zielRaumWahl.value);
  } else if (art === "kicken" && zielMitglied.value) {
    await trenneMitglied(zielMitglied.value, eingabeGrund.value);
  } else if (art === "sperren" && zielMitglied.value) {
    await sperreMitglied(zielMitglied.value, eingabeGrund.value);
  } else if (art === "rolle" && zielMitglied.value) {
    await setzeRolle(zielMitglied.value, rollenWahl.value);
  }
}

watch(
  () => beitraege.value.length,
  async () => {
    await nextTick();
    if (verlauf.value) verlauf.value.scrollTop = verlauf.value.scrollHeight;
  },
);

async function absenden() {
  const text = eingabe.value;
  eingabe.value = "";
  // Abgelehnt (Server weg, zu lang): der Text kommt zurück ins Feld, statt
  // verloren zu sein — es sei denn, inzwischen wurde Neues getippt.
  const effekt = effektWahl.value;
  effektWahl.value = null;
  if (!(await sende(text, effekt)) && eingabe.value === "") {
    eingabe.value = text;
    effektWahl.value = effekt;
  }
}
</script>

<template>
  <div class="ansicht" :class="{ 'chat-zu': chatEingeklappt }">
    <!-- Spalte 1: Räume mit ihren Leuten — eine Liste, kein Springen -->
    <aside class="seite">
      <header class="seite__kopf">
        <span class="marke">{{ t("app.rooms") }}</span>
        <UI.WpIconButton
          v-if="darfRaeumeVerwalten"
          :icon="IconPlus"
          :label="t('room.add')"
          size="sm"
          variant="ghost"
          @press="(eingabeName = ''), (zielRaum = null), (dialog = 'raum')"
        />
      </header>

      <div class="raeume">
        <section v-for="raum in modell.raeume" :key="raum.id" class="block">
          <div class="zeile">
            <button
              type="button"
              class="raum"
              :class="{ 'ist-hier': raum.hier }"
              @click="(e: MouseEvent) => e.button === 0 && betreten(raum.id)"
              @contextmenu="oeffneRaumMenue(raum.id, $event)"
            >
              <span class="raum__marke" aria-hidden="true" />
              <span class="raum__name">{{ raum.name }}</span>
              <span class="raum__zahl">{{ anwesendeVon(raum.id).length || "" }}</span>
            </button>
            <UI.WpMenu
              class="zeilenmenue"
              :items="raumMenuePunkte()"
              :label="t('room.menu')"
              align="right"
              :open="offenerRaum === raum.id"
              @update:open="(offen: boolean) => (offenerRaum = offen ? raum.id : null)"
              @select="(wahl: string) => raumMenueGewaehlt(raum.id, wahl)"
            >
              <UI.WpIconButton :icon="IconMore" :label="t('room.menu')" size="sm" variant="ghost" />
            </UI.WpMenu>
          </div>

          <!-- Jeder Raum ist verschlüsselt. Wer den Sprachschlüssel des
               eigenen Raums nicht hat, hört nichts — und weiß ohne diese
               Zeile nicht, warum. Für andere Räume ist die Frage sinnlos. -->
          <p v-if="raum.hier && schluesselVorhanden(raum.id) === false" class="schluessel fehlt">
            {{ t("room.key_missing") }}
            <UI.WpButton
              :label="t('room.key_request')"
              size="sm"
              variant="ghost"
              @press="schluesselAnfordern(raum.id)"
            />
          </p>

          <ul v-if="anwesendeVon(raum.id).length" class="leute">
            <li
              v-for="m in anwesendeVon(raum.id)"
              :key="m.id"
              class="mensch"
              @contextmenu="m.ich ? undefined : oeffneMitgliedMenue(m.id, $event)"
            >
              <UI.WpAvatar
                :name="m.name"
                :speaking="m.spricht"
                :muted="m.stumm"
                :away="m.taub"
                :tone="m.ich ? 'accent' : 'neutral'"
                size="sm"
              />
              <span class="mensch__text">
                <span class="mensch__name">
                  {{ m.name }}<span v-if="m.ich" class="mensch__ich"> {{ t("chat.you") }}</span>
                </span>
                <span class="mensch__rolle">
                  {{ rollenName(m.rolle) }}<template v-if="m.stumm"> · {{ t("state.muted") }}</template
                  ><template v-if="m.taub"> · {{ t("state.deaf") }}</template>
                </span>
              </span>
              <UI.WpMenu
                v-if="!m.ich && darfEingreifen"
                class="zeilenmenue"
                :items="mitgliedMenue(m.id)"
                :label="t('server.menu_for', m.name)"
                align="right"
                :open="offenesMitglied === m.id"
                @update:open="(offen: boolean) => (offenesMitglied = offen ? m.id : null)"
                @select="(wahl: string) => mitgliedMenueGewaehlt(m.id, wahl)"
              >
                <UI.WpIconButton :icon="IconMore" :label="t('server.menu_for', m.name)" size="sm" variant="ghost" />
              </UI.WpMenu>
            </li>
          </ul>
        </section>
      </div>
    </aside>

    <!-- Spalte 2: Verlauf -->
    <section class="haupt">
      <header class="kopf">
        <div class="kopf__text">
          <h1>{{ meinRaum ? meinRaum.name : t("room.none") }}</h1>
          <p v-if="meinRaum" class="kopf__e2e">
            <IconLock class="kopf__schloss" aria-hidden="true" />{{ t("chat.e2e") }}
          </p>
        </div>
        <!-- Chat nach rechts einklappen: der Verlauf verschwindet, die
             Raumliste bekommt die Breite. Kein Schalter in den Einstellungen —
             das ist eine Handbewegung, keine Vorliebe. -->
        <div class="kopf__rechts">
          <UI.WpIconButton
            :icon="IconPanelRight"
            :label="t('chat.collapse')"
            size="sm"
            variant="ghost"
            @press="chatUmschalten"
          />
        </div>
      </header>

      <EffektBuehne />

      <div ref="verlauf" class="verlauf">
        <UI.WpEmptyState
          v-if="!meinRaum"
          :title="t('room.none')"
          :description="t('room.choose')"
        />
        <UI.WpEmptyState
          v-else-if="beitraege.length === 0"
          :title="t('room.empty.title')"
          :description="t('room.empty.desc')"
        />

        <Beitrag v-for="n in beitraege" :key="n.id" :n="n" :eigene="n.verfasser === eigeneId" />
      </div>

      <!-- Ohne Serverschlüssel kein Schreiben: was hinausginge, wäre
           unverschlüsselt — der Server nähme es auch nicht an (ADR 0012). -->
      <p v-if="meinRaum && verbunden && modell.textschluessel === 'wartet'" class="schreiben schreiben--wartet" role="status">
        {{ t("chat.key_wait") }}
      </p>
      <div v-else-if="meinRaum && verbunden" class="entwurf">
        <!-- Vorschau des ersten Links: so geht sie mit hinaus. Wegklicken
             heißt, der Text geht ohne. -->
        <div v-if="modell.entwurf.art === 'fertig'" class="entwurf__vorschau">
          <img
            v-if="modell.entwurf.vorschau.bild"
            class="entwurf__bild"
            :src="`data:image/jpeg;base64,${modell.entwurf.vorschau.bild}`"
            alt=""
          />
          <span class="entwurf__titel">{{ modell.entwurf.vorschau.titel }}</span>
          <UI.WpIconButton :icon="IconClose" :label="t('preview.remove')" size="sm" variant="ghost" @press="vorschauVerwerfen" />
        </div>
        <p v-else-if="modell.entwurf.art === 'laedt'" class="entwurf__laedt" role="status">{{ t("preview.loading") }}</p>
        <div class="schreiben">
          <UI.WpMenu :items="effektMenue" :label="t('effect.choose')" align="left" @select="effektGewaehlt">
            <UI.WpButton
              :label="effektWahl ? t(`effect.${effektWahl}`) : t('effect.short')"
              size="sm"
              :variant="effektWahl ? 'secondary' : 'ghost'"
            />
          </UI.WpMenu>
          <UI.WpInput v-model="eingabe" :placeholder="t('chat.placeholder')" @keyup.enter="absenden" />
          <UI.WpButton
            :label="t('chat.send')"
            variant="primary"
            :state="eingabe.trim() ? 'default' : 'disabled'"
            @press="absenden"
          />
        </div>
      </div>
    </section>

    <!-- Eingeklappt: eine schmale Schiene rechts. Ein Klick holt den Chat
         zurück; die Zahl zeigt ungelesene Nachrichten seit dem Einklappen. -->
    <button
      v-if="chatEingeklappt"
      class="schiene"
      type="button"
      :title="t('chat.expand')"
      @click="chatUmschalten"
    >
      <UI.WpIcon :icon="IconSprechblase" size="md" class="schiene__zeichen" />
      <span v-if="ungelesen > 0" class="schiene__zahl">{{ ungelesen }}</span>
      <span class="schiene__text">{{ meinRaum ? meinRaum.name : t("chat.title") }}</span>
    </button>

    <!-- Rollen vergeben: an Ort und Stelle, mit dem Namen im Titel. -->
    <UI.WpDialog
      :open="dialog === 'rolle'"
      :title="t('member.role') + ': ' + zielName"
      :description="t('member.role.desc')"
      :confirm-label="t('member.role.apply')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    >
      <UI.WpSelect v-model="rollenWahl" :options="rollen" :label="t('member.role')" />
    </UI.WpDialog>

    <!-- Dialoge -->
    <UI.WpDialog
      :open="dialog === 'raum'"
      :title="t('room.add.title')"
      :description="t('room.add.desc')"
      :confirm-label="t('room.add.confirm')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    >
      <UI.WpInput v-model="eingabeName" :placeholder="t('room.add.placeholder')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'umbenennen'"
      :title="t('room.rename.title')"
      :description="raumName"
      :confirm-label="t('room.rename')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    >
      <UI.WpInput v-model="eingabeName" :placeholder="t('room.rename.placeholder')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'loeschen'"
      :title="t('room.delete.title')"
      :description="t('room.delete.desc', raumName)"
      tone="warn"
      :confirm-label="t('chat.delete')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    />

    <UI.WpDialog
      :open="dialog === 'verschieben'"
      :title="t('member.move.title', zielName)"
      :description="t('member.move.desc')"
      :confirm-label="t('member.move.confirm')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    >
      <UI.WpSelect v-model="zielRaumWahl" :options="raumWahl" :label="t('room.enter')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'kicken'"
      :title="t('member.kick.title', zielName)"
      :description="t('member.kick.desc')"
      tone="warn"
      :confirm-label="t('member.kick.confirm')"
      @confirm="bestaetigen"
      @update:open="dialog = null"
    >
      <UI.WpInput v-model="eingabeGrund" :placeholder="t('member.kick.reason')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'sperren'"
      :title="t('member.ban.title', zielName)"
      :description="t('member.ban.desc')"
      tone="error"
      :confirm-label="t('member.ban.confirm')"
      @update:open="dialog = null"
      @confirm="bestaetigen"
    >
      <UI.WpInput v-model="eingabeGrund" :placeholder="t('member.ban.reason')" />
    </UI.WpDialog>
  </div>
</template>

<style scoped>
.ansicht {
  display: flex;
  flex: 1;
  min-height: 0;
}

/* ---- Spalte 1: Räume mit Leuten ---- */
.seite {
  width: 19rem;
  flex: none;
  display: flex;
  flex-direction: column;
  border-right: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
  overflow-y: auto;
}

.seite__kopf {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-2) var(--wp-space-3);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
  min-height: var(--wp-size-control-md);
}

/* Kleine Versalien: Beschriftung, keine Überschrift. */
.marke {
  font-size: var(--wp-font-size-xs);
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--wp-color-text-faint);
}

.seite__kopf .marke {
  flex: 1;
}

.raeume {
  display: flex;
  flex-direction: column;
  padding: var(--wp-space-1) 0 var(--wp-space-3);
}

.block {
  display: flex;
  flex-direction: column;
}

.zeile {
  display: flex;
  align-items: center;
  gap: var(--wp-space-1);
  padding-right: var(--wp-space-1);
}

.raum {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: var(--wp-space-2);
  padding: var(--wp-space-1) var(--wp-space-2) var(--wp-space-1) var(--wp-space-3);
  background: transparent;
  border: 0;
  color: var(--wp-color-text-muted);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.raum:hover {
  background: var(--wp-color-bg-hover);
  color: var(--wp-color-text-default);
}

/* Aktiver Raum: Fläche + 2-px-Akzentkante links. Kein Rahmen, keine Rundung. */
.raum.ist-hier {
  background: var(--wp-color-bg-raised);
  color: var(--wp-color-text-default);
  font-weight: var(--wp-font-weight-semibold);
}

.raum.ist-hier .raum__marke {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: calc(var(--wp-size-border) * 2);
  background: var(--wp-color-accent-default);
}

/* Ende zu Ende: einmal im Kopf des Chats, nicht an jedem Raum — es gilt für
   alle. Klein und ruhig: ein Zustand, kein Schmuck. */
.kopf__e2e {
  display: flex;
  align-items: center;
  gap: var(--wp-space-1);
}

.kopf__schloss {
  width: var(--wp-font-size-xs);
  height: var(--wp-font-size-xs);
  flex: none;
}

.raum__name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--wp-font-size-sm);
}

.raum__zahl {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  font-variant-numeric: tabular-nums;
}

/* Die Leute eines Raums hängen direkt darunter — eine Liste, ein Blick. */
.leute {
  list-style: none;
  margin: 0;
  padding: 0 0 var(--wp-space-1) 0;
  display: flex;
  flex-direction: column;
  gap: var(--wp-size-border);
}

.mensch {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: calc(var(--wp-space-1) / 2) var(--wp-space-2) calc(var(--wp-space-1) / 2) calc(var(--wp-space-4) + var(--wp-space-1));
}

.mensch:hover {
  background: var(--wp-color-bg-hover);
}

.mensch__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.2;
}

.mensch__name {
  font-size: var(--wp-font-size-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mensch__ich {
  /* Der Leerraum vor „du" fällt beim Übersetzen der Vorlage weg. */
  margin-left: var(--wp-space-1);
  color: var(--wp-color-text-faint);
  font-size: var(--wp-font-size-xs);
}

.mensch__rolle {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

/* Das ⋯-Menü einer Zeile erscheint erst beim Überfahren oder mit Fokus.
   Über eine eigene Klasse, **nicht** über `:last-child`: eine Zeile ohne Menü
   (die eigene, oder jede, wenn man nicht eingreifen darf) hätte sonst ihren
   Namen versteckt — ein Mitglied sah in der Raumliste nur Kürzel. */
.zeilenmenue {
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.zeile:hover > .zeilenmenue,
.zeile:focus-within > .zeilenmenue,
.mensch:hover > .zeilenmenue,
.mensch:focus-within > .zeilenmenue {
  opacity: 1;
}

/* ---- Spalte 2 ---- */
.haupt {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.kopf {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  padding: var(--wp-space-3) var(--wp-space-4);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.kopf h1 {
  margin: 0;
  font-size: var(--wp-font-size-lg);
  font-weight: var(--wp-font-weight-semibold);
}

.kopf p {
  margin: calc(var(--wp-space-1) / 2) 0 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
}

.kopf__rechts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
}

.verlauf {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-4) var(--wp-space-4) var(--wp-space-3);
}

/* ---- Eingeklappter Chat ---- */
/*
 * Zugeklappt verschwindet der Verlauf, und die Raumliste bekommt die Breite.
 * Rechts bleibt eine schmale Schiene stehen — sie ist der einzige Weg zurück,
 * damit niemand den Chat „verliert".
 */
.ansicht.chat-zu .haupt {
  display: none;
}

.ansicht.chat-zu .seite {
  width: auto;
  flex: 1;
}

.schiene {
  flex: none;
  width: var(--wp-size-control-lg);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-3) 0;
  background: var(--wp-color-bg-panel);
  border: 0;
  border-left: var(--wp-size-border) solid var(--wp-color-border-subtle);
  color: var(--wp-color-text-muted);
  cursor: pointer;
}

.schiene:hover {
  color: var(--wp-color-text-default);
  background: var(--wp-color-bg-raised);
}

.schiene__zeichen {
  color: inherit;
}

.schiene__zahl {
  min-width: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
  padding: 0 var(--wp-space-1);
  font-size: var(--wp-font-size-xs);
  line-height: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
  text-align: center;
  color: var(--wp-color-bg-panel);
  background: var(--wp-color-accent-default);
  border-radius: var(--wp-radius-sm);
}

/* Der Raumname läuft senkrecht — die Schiene bleibt schmal. */
.schiene__text {
  writing-mode: vertical-rl;
  font-size: var(--wp-font-size-xs);
  letter-spacing: 0.04em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-height: 60%;
}

.schreiben--wartet {
  margin: 0;
  border-top: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
}

.entwurf {
  border-top: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
}

.entwurf__vorschau {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  margin: var(--wp-space-2) var(--wp-space-4) 0;
  padding: var(--wp-space-1) var(--wp-space-2);
  border-left: calc(var(--wp-size-border) * 2) solid var(--wp-color-border-strong);
  background: var(--wp-color-bg-raised);
}

.entwurf__bild {
  width: var(--wp-size-control-lg);
  height: var(--wp-size-control-lg);
  object-fit: cover;
}

.entwurf__titel {
  flex: 1;
  min-width: 0;
  font-size: var(--wp-font-size-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entwurf__laedt {
  margin: var(--wp-space-2) var(--wp-space-4) 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.schreiben {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-3) var(--wp-space-4);
}

.schreiben > :nth-child(2) {
  flex: 1;
}

/* ---------------------------------------------------------------------------
 * Schmale Fenster: eine Spalte statt zwei
 *
 * Unter 620 px passen Raumliste und Verlauf nicht mehr nebeneinander. Dann gilt
 * dieselbe Regel wie in einem Telefon: **eine** Ansicht zur Zeit. Der
 * Zuklapp-Knopf wird damit zum Zurück-Knopf — die Schiene führt zur Liste,
 * ein Klick auf die Liste zum Chat.
 * ------------------------------------------------------------------------- */
@media (max-width: 620px) {
  .ansicht:not(.chat-zu) .seite {
    display: none;
  }

  .ansicht.chat-zu .seite {
    width: 100%;
  }

  .verlauf {
    padding: var(--wp-space-3);
  }

  .schreiben {
    padding: var(--wp-space-2) var(--wp-space-3);
  }

  .kopf {
    padding: var(--wp-space-2) var(--wp-space-3);
  }

  .schiene {
    width: var(--wp-size-control-md);
  }
}

/* Der Schlüsselstand ist eine Zeile unter dem Raumnamen: ruhig, wenn alles da
   ist; deutlich, wenn nicht — dann hört man nämlich nichts. */
.schluessel {
  margin: 0 0 var(--wp-space-2) var(--wp-space-3);
  font-size: var(--wp-font-size-xs, 0.75rem);
  color: var(--wp-color-text-muted);
}

.schluessel.fehlt {
  color: var(--wp-color-status-warn);
  font-weight: var(--wp-font-weight-semibold);
}
</style>
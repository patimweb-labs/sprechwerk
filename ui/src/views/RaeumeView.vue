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
import { IconChat, IconLock, IconMore, IconPanelRight, IconPlus } from "@weppelmann/icons";
import {
  ausRaumWerfen,
  betreten,
  darfEingreifen,
  darfRaeumeVerwalten,
  verschluesseln,
  speichere,
  setzeRolle,
  darfRollenVergeben,
  einstellungen,
  legeRaumAn,
  loesche,
  meinRaum,
  modell,
  raumUmbenennen,
  sende,
  sperreMitglied,
  stummschalten,
  trenneMitglied,
  t,
  rolle as rollenName,
  uhrzeit,
  verbunden,
  verschieben,
} from "../store";
import { api } from "../api";

const eingabe = ref("");
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
    letzteGesehen.value = modell.nachrichten.length;
  }
  void speichere();
}

watch(
  () => modell.nachrichten.length,
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
const rollen = computed(() => [
  { value: "administrator", label: t("rolle.administrator") },
  { value: "moderator", label: t("rolle.moderator") },
  { value: "mitglied", label: t("rolle.mitglied") },
  { value: "gast", label: t("rolle.gast") },
]);
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
    // Ende-zu-Ende-Verschlüsselung: je Raum, nur mit Recht.
    const verschluesselt =
      modell.raeume.find((r) => r.id === zielRaum.value)?.verschluesselt ?? false;
    punkte.push({
      id: verschluesselt ? "klar" : "verschluesseln",
      label: verschluesselt ? t("room.unencrypt") : t("room.encrypt"),
      hint: t("room.encrypt.hint"),
      separatorBefore: true,
    });
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
    case "verschluesseln":
      void verschluesseln(id, true);
      break;
    case "klar":
      void verschluesseln(id, false);
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
    await api.raumLoeschen(zielRaum.value);
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
              <span class="raum__name">
                {{ raum.name }}
                <IconLock
                  v-if="raum.verschluesselt"
                  class="raum__schloss"
                  :title="t('room.encrypted')"
                />
              </span>
              <span class="raum__zahl">{{ anwesendeVon(raum.id).length || "" }}</span>
            </button>
            <UI.WpMenu
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
                :items="mitgliedMenue(m.id)"
                :label="`Menü für ${m.name}`"
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

      <div ref="verlauf" class="verlauf">
        <UI.WpEmptyState
          v-if="!meinRaum"
          :title="t('room.none')"
          :description="t('room.choose')"
        />
        <UI.WpEmptyState
          v-else-if="modell.nachrichten.length === 0"
          :title="t('room.empty.title')"
          :description="t('room.empty.desc')"
        />

        <article
          v-for="n in modell.nachrichten"
          :key="n.id"
          class="beitrag"
          :class="{ 'ist-eigene': n.verfasser === eigeneId }"
        >
          <div class="beitrag__text">
            <header class="beitrag__kopf">
              <span class="beitrag__wer" :class="{ 'ist-eigene': n.verfasser === eigeneId }">{{ n.verfasser_name }}</span>
              <span class="beitrag__wann">{{ uhrzeit(n.zeit) }}</span>
              <button v-if="n.loeschbar" class="beitrag__aktion" type="button" @click="loesche(n.id)">
                {{ t("chat.delete") }}
              </button>
            </header>
            <p v-if="n.geloescht" class="beitrag__weg">{{ t("chat.deleted") }}</p>
            <p v-else class="beitrag__inhalt">{{ n.text }}</p>
          </div>
        </article>
      </div>

      <div v-if="meinRaum && verbunden" class="schreiben">
        <UI.WpInput v-model="eingabe" :placeholder="t('chat.placeholder')" @keyup.enter="absenden" />
        <UI.WpButton
          :label="t('chat.send')"
          variant="primary"
          :state="eingabe.trim() ? 'default' : 'disabled'"
          @press="absenden"
        />
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
      <IconChat class="schiene__zeichen" />
      <span v-if="ungelesen > 0" class="schiene__zahl">{{ ungelesen }}</span>
      <span class="schiene__text">{{ meinRaum ? meinRaum.name : t("chat.title") }}</span>
    </button>

    <!-- Rollen vergeben: an Ort und Stelle, mit dem Namen im Titel. -->
    <UI.WpDialog
      :open="dialog === 'rolle'"
      :title="t('member.role') + ': ' + zielName"
      :description="t('member.role.desc')"
      :confirm-label="t('member.role.apply')"
      :state="rollenWahl === 'administrator' && !darfRollenVergeben ? 'disabled' : 'default'"
      @confirm="bestaetigen"
      @cancel="dialog = null"
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
      @cancel="dialog = null"
    >
      <UI.WpInput v-model="eingabeName" :placeholder="t('room.add.placeholder')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'umbenennen'"
      :title="t('room.rename.title')"
      :description="raumName"
      :confirm-label="t('room.rename')"
      @confirm="bestaetigen"
      @cancel="dialog = null"
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
      @cancel="dialog = null"
    />

    <UI.WpDialog
      :open="dialog === 'verschieben'"
      :title="t('member.move.title', zielName)"
      :description="t('member.move.desc')"
      :confirm-label="t('member.move.confirm')"
      @confirm="bestaetigen"
      @cancel="dialog = null"
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
      @cancel="dialog = null"
    >
      <UI.WpInput v-model="eingabeGrund" :placeholder="t('member.kick.reason')" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'sperren'"
      :title="t('member.ban.title', zielName)"
      :description="t('member.ban.desc')"
      tone="error"
      :confirm-label="t('member.ban.confirm')"
      @cancel="dialog = null"
      @confirm="bestaetigen"
    >
      <UI.WpInput v-model="eingabeGrund" :placeholder="t('member.kick.reason')" />
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
  width: 2px;
  background: var(--wp-color-accent-default);
}

/* Schloss an verschlüsselten Räumen: klein, in der Textfarbe — ein Zustand,
   kein Schmuck. */
.raum__schloss {
  width: 12px;
  height: 12px;
  margin-left: var(--wp-space-1);
  vertical-align: -1px;
  color: var(--wp-color-text-muted);
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
  gap: 1px;
}

.mensch {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: 2px var(--wp-space-2) 2px calc(var(--wp-space-4) + var(--wp-space-1));
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
  color: var(--wp-color-text-faint);
  font-size: var(--wp-font-size-xs);
}

.mensch__rolle {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.zeile > :last-child,
.mensch > :last-child {
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.zeile:hover > :last-child,
.zeile:focus-within > :last-child,
.mensch:hover > :last-child,
.mensch:focus-within > :last-child {
  opacity: 1;
}

/* ---- Spalte 2 ---- */
.haupt {
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
  margin: 2px 0 0;
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
  color: var(--wp-color-text);
  background: var(--wp-color-bg-raised);
}

.schiene__zeichen {
  width: 16px;
  height: 16px;
}

.schiene__zahl {
  min-width: 18px;
  padding: 0 4px;
  font-size: var(--wp-font-size-xs);
  line-height: 18px;
  text-align: center;
  color: var(--wp-color-bg-panel);
  background: var(--wp-color-accent);
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

.beitrag {
  padding: var(--wp-space-1) 0 var(--wp-space-3);
  border-left: 2px solid transparent;
  padding-left: var(--wp-space-2);
}

/* Eigene Beiträge: Akzentkante links statt Farbe im Text. */
.beitrag.ist-eigene {
  border-left-color: var(--wp-color-accent-default);
}

.beitrag__wer.ist-eigene {
  color: var(--wp-color-accent-default);
}

.beitrag__text {
  min-width: 0;
  flex: 1;
}

.beitrag__kopf {
  display: flex;
  align-items: baseline;
  gap: var(--wp-space-2);
}

.beitrag__wer {
  font-size: var(--wp-font-size-sm);
  font-weight: var(--wp-font-weight-semibold);
}

.beitrag__wann {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  font-variant-numeric: tabular-nums;
}

.beitrag__aktion {
  margin-left: auto;
  border: 0;
  background: transparent;
  color: var(--wp-color-text-faint);
  font: inherit;
  font-size: var(--wp-font-size-xs);
  padding: 0;
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.beitrag:hover .beitrag__aktion,
.beitrag:focus-within .beitrag__aktion {
  opacity: 1;
}

.beitrag__aktion:hover {
  color: var(--wp-color-status-error);
}

.beitrag__inhalt {
  margin: var(--wp-space-1) 0 0;
  white-space: pre-wrap;
  line-height: var(--wp-font-line-normal);
}

.beitrag__weg {
  margin: var(--wp-space-1) 0 0;
  color: var(--wp-color-text-faint);
  font-style: italic;
}

.schreiben {
  display: flex;
  gap: var(--wp-space-2);
  padding: var(--wp-space-3) var(--wp-space-4);
  border-top: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
}

.schreiben > :first-child {
  flex: 1;
}
</style>

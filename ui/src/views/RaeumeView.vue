<script setup lang="ts">
// Räume links, Verlauf rechts. Zwei Seitenleisten wie im Pult-Modell:
// oben die Räume (Baum), darunter die Anwesenden.
import { computed, nextTick, ref, watch } from "vue";
import * as UI from "@weppelmann/components";
import { IconPlus, IconTrash } from "@weppelmann/icons";
import {
  anwesenheitText,
  betreten,
  darfRaeumeVerwalten,
  darfRollenVergeben,
  legeRaumAn,
  loesche,
  meinRaum,
  modell,
  sende,
  setzeRolle,
  uhrzeit,
  verlassen,
} from "../store";

const eingabe = ref("");
const neuerRaum = ref("");
const verlauf = ref<HTMLElement | null>(null);
const zeigeRaumDialog = ref(false);
const zeigeLoeschDialog = ref(false);
const raumLoeschen = ref<string | null>(null);

const hierAnwesend = computed(() =>
  modell.mitglieder.filter((m) => m.raum && m.raum === meinRaum.value?.id),
);

/** Räume als Auswahl für die Leiste — mit Einrückung als Zeichen. */
const raumEintraege = computed(() =>
  modell.raeume.map((r) => ({
    id: r.id,
    label: `${"·  ".repeat(r.tiefe)}${r.name}`,
    count: r.anwesend || "",
    aktiv: r.hier,
  })),
);

const rollen = [
  { value: "administrator", label: "Administrator" },
  { value: "moderator", label: "Moderator" },
  { value: "mitglied", label: "Mitglied" },
  { value: "gast", label: "Gast" },
];

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

async function anlegen() {
  const name = neuerRaum.value;
  neuerRaum.value = "";
  zeigeRaumDialog.value = false;
  await legeRaumAn(name, meinRaum.value?.id ?? null);
}

async function loeschen() {
  const raum = raumLoeschen.value;
  zeigeLoeschDialog.value = false;
  if (!raum) return;
  const { api } = await import("../api");
  await api.raumLoeschen(raum);
}

function frageLoeschen(raum: string) {
  raumLoeschen.value = raum;
  zeigeLoeschDialog.value = true;
}
</script>

<template>
  <div class="ansicht">
    <UI.WpSidePanel title="Räume" :count="modell.raeume.length" width="17rem">
      <template #actions>
        <UI.WpIconButton
          v-if="darfRaeumeVerwalten"
          :icon="IconPlus"
          label="Raum anlegen"
          size="sm"
          @press="zeigeRaumDialog = true"
        />
      </template>
      <ul class="liste">
        <li v-for="r in raumEintraege" :key="r.id">
          <button
            type="button"
            class="eintrag"
            :class="{ 'ist-hier': r.aktiv }"
            @click="betreten(r.id)"
            @dblclick="darfRaeumeVerwalten && frageLoeschen(r.id)"
          >
            <span class="name">{{ r.label }}</span>
            <span v-if="r.count !== ''" class="zahl">{{ r.count }}</span>
          </button>
        </li>
      </ul>
      <template #footer>
        <UI.WpButton v-if="meinRaum" label="Raum verlassen" size="sm" @press="verlassen" />
      </template>
    </UI.WpSidePanel>

    <UI.WpSidePanel title="Hier" :count="hierAnwesend.length" width="13rem">
      <ul class="liste">
        <li v-for="m in hierAnwesend" :key="m.id" class="mitglied">
          <span class="name">
            {{ m.name }}<span v-if="m.ich" class="ich"> (du)</span>
          </span>
          <UI.WpBadge v-if="m.spricht" label="spricht" tone="ok" />
          <span v-else class="rolle">{{ m.rolle }}</span>
        </li>
        <li v-if="hierAnwesend.length === 0">
          <UI.WpText variant="caption" tone="muted">niemand hier</UI.WpText>
        </li>
      </ul>
      <template v-if="darfRollenVergeben && hierAnwesend.length" #footer>
        <UI.WpText variant="caption" tone="muted">Rollen</UI.WpText>
        <div class="rollen">
          <div v-for="m in hierAnwesend.filter((x) => !x.ich)" :key="m.id" class="rollenzeile">
            <span class="name">{{ m.name }}</span>
            <UI.WpSelect
              :model-value="m.rolle"
              :options="rollen"
              @update:model-value="(wert: string | null) => wert && setzeRolle(m.id, wert)"
            />
          </div>
        </div>
      </template>
    </UI.WpSidePanel>

    <section class="haupt">
      <UI.WpToolbar
        :title="meinRaum ? meinRaum.name : 'Kein Raum'"
        :subtitle="meinRaum ? anwesenheitText() : 'Wähle links einen Raum.'"
      >
        <template #actions>
          <UI.WpButton
            v-if="meinRaum && darfRaeumeVerwalten"
            label="Löschen"
            size="sm"
            :icon="IconTrash"
            @press="frageLoeschen(meinRaum.id)"
          />
        </template>
      </UI.WpToolbar>

      <div ref="verlauf" class="verlauf">
        <UI.WpEmptyState
          v-if="!meinRaum"
          title="Kein Raum offen"
          description="Wähle links einen Raum, um mitzulesen und zu sprechen."
        />
        <UI.WpEmptyState
          v-else-if="modell.nachrichten.length === 0"
          title="Noch nichts geschrieben"
          description="Der erste Satz ist der schwerste."
        />
        <article v-for="n in modell.nachrichten" :key="n.id" class="nachricht">
          <header>
            <UI.WpText variant="body">{{ n.verfasser_name }}</UI.WpText>
            <UI.WpText variant="caption" tone="muted">{{ uhrzeit(n.zeit) }}</UI.WpText>
            <UI.WpButton
              v-if="n.loeschbar"
              label="löschen"
              size="sm"
              variant="ghost"
              @press="loesche(n.id)"
            />
          </header>
          <UI.WpText v-if="n.geloescht" variant="body" tone="muted">(Nachricht gelöscht)</UI.WpText>
          <p v-else class="text">{{ n.text }}</p>
        </article>
      </div>

      <form v-if="meinRaum" class="eingabe" @submit.prevent="absenden">
        <UI.WpInput v-model="eingabe" placeholder="Schreiben …" @keyup.enter="absenden" />
        <UI.WpButton label="Senden" variant="primary" @press="absenden" />
      </form>
    </section>

    <UI.WpDialog
      :open="zeigeRaumDialog"
      title="Raum anlegen"
      description="Der Raum entsteht unter dem aktuellen Raum."
      confirm-label="Anlegen"
      @confirm="anlegen"
      @cancel="zeigeRaumDialog = false"
    >
      <UI.WpInput v-model="neuerRaum" placeholder="Name des Raums" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="zeigeLoeschDialog"
      title="Raum löschen"
      description="Nur möglich, wenn keine Unterräume mehr darin sind. Die Nachrichten des Raums verschwinden mit."
      tone="warn"
      confirm-label="Löschen"
      @confirm="loeschen"
      @cancel="zeigeLoeschDialog = false"
    />
  </div>
</template>

<style scoped>
.ansicht {
  display: flex;
  flex: 1;
  min-height: 0;
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
  width: 100%;
  padding: var(--wp-space-1) var(--wp-space-2);
  background: transparent;
  border: 0;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.eintrag:hover {
  background: var(--wp-color-bg-hover);
}

.eintrag.ist-hier {
  background: var(--wp-color-bg-selected);
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.zahl,
.rolle {
  margin-left: auto;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.mitglied {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-1) var(--wp-space-2);
}

.ich {
  color: var(--wp-color-text-muted);
}

.rollen {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-2);
}

.rollenzeile {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
}

.haupt {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.verlauf {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-3) var(--wp-space-4);
}

.nachricht {
  margin-bottom: var(--wp-space-3);
}

.nachricht header {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
}

.text {
  margin: var(--wp-space-1) 0 0;
  white-space: pre-wrap;
}

.eingabe {
  display: flex;
  gap: var(--wp-space-2);
  padding: var(--wp-space-3) var(--wp-space-4);
  border-top: var(--wp-size-border) solid var(--wp-color-border-subtle);
}
</style>

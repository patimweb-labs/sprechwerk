<script setup lang="ts">
// Räume, Menschen, Verlauf.
//
// Gestaltung nach den Regeln für Kontextmenüs (NN/G, Apple HIG, Paste):
// • Harmlose Eingriffe oben, **trennende Linie**, destruktive unten und in
//   Warnfarbe — nie neben einer harmlosen Aktion.
// • Kein verschachteltes Bedienelement im Eintrag; wer eine Auswahl braucht,
//   bekommt einen Dialog (Verschieben, Kicken, Sperren).
// • Sprech-, Stumm- und Taubzustand sind **getrennt** gekennzeichnet: der Ring
//   am Namenszeichen zeigt Sprechen, ein Strich Stumm, ein Punkt Taub.
// • Das Menü öffnet per Rechtsklick **und** über den Knopf (Tastatur, Touch).
import { computed, nextTick, ref, watch } from "vue";
import * as UI from "@weppelmann/components";
import { IconInfo, IconMore, IconPlus, IconTrash } from "@weppelmann/icons";
import {
  anwesenheitText,
  betreten,
  darfEingreifen,
  darfRaeumeVerwalten,
  legeRaumAn,
  loesche,
  meinRaum,
  modell,
  sende,
  stummschalten,
  uhrzeit,
  verschieben,
  ausRaumWerfen,
  trenneMitglied,
  sperreMitglied,
  raumUmbenennen,
  verlassen,
} from "../store";
import { api } from "../api";

const eingabe = ref("");
const verlauf = ref<HTMLElement | null>(null);

// ---- Kontextmenüs -------------------------------------------------------
/** Welches Mitgliedsmenü ist offen? (Kennung oder null) */
const offenesMitglied = ref<string | null>(null);
/** Welches Raummenü ist offen? */
const offenerRaum = ref<string | null>(null);

function oeffneMitgliedMenue(id: string, ereignis?: MouseEvent) {
  ereignis?.preventDefault();
  offenerRaum.value = null;
  offenesMitglied.value = id;
}

function oeffneRaumMenue(id: string, ereignis?: MouseEvent) {
  ereignis?.preventDefault();
  offenesMitglied.value = null;
  offenerRaum.value = id;
}

// ---- Dialoge ------------------------------------------------------------
const dialog = ref<null | "raum" | "umbenennen" | "verschieben" | "kicken" | "sperren" | "loeschen">(null);
const zielMitglied = ref<string | null>(null);
const zielRaum = ref<string | null>(null);
const eingabeName = ref("");
const eingabeGrund = ref("");
const zielRaumWahl = ref<string | null>(null);

const zielName = computed(
  () => modell.mitglieder.find((m) => m.id === zielMitglied.value)?.name ?? "",
);
const raumName = computed(
  () => modell.raeume.find((r) => r.id === zielRaum.value)?.name ?? "",
);
const raumWahl = computed(() =>
  modell.raeume.map((r) => ({ value: r.id, label: `${"— ".repeat(r.tiefe)}${r.name}` })),
);

// ---- Daten --------------------------------------------------------------
const eigeneId = computed(() => {
  const v = modell.verbindung;
  return v && v.art === "verbunden" ? v.ich : null;
});

const hierAnwesend = computed(() =>
  modell.mitglieder.filter((m) => m.raum && m.raum === meinRaum.value?.id),
);

const sprechender = computed(() => hierAnwesend.value.find((m) => m.spricht)?.name ?? null);

const raumBaum = computed(() => {
  const oben = modell.raeume.filter((r) => r.eltern === null);
  return oben.map((r) => ({ ...r, kinder: modell.raeume.filter((k) => k.eltern === r.id) }));
});

function kachelMeta(raumId: string, anwesend: number): string {
  const teile = [anwesend === 0 ? "leer" : anwesend === 1 ? "1 anwesend" : `${anwesend} anwesend`];
  const spricht = modell.mitglieder.find((m) => m.raum === raumId && m.spricht);
  if (spricht) teile.push(`${spricht.name} spricht`);
  return teile.join(" · ");
}

/** Menüpunkte für ein Mitglied. */
function mitgliedMenue(id: string): UI.MenuItem[] {
  const m = modell.mitglieder.find((x) => x.id === id);
  if (!m) return [];
  const punkte: UI.MenuItem[] = [
    {
      id: "mute",
      label: m.stumm ? "Sprechen freigeben" : "Mikrofon stummschalten",
      hint: "Server",
    },
    {
      id: "deaf",
      label: m.taub ? "Ton freigeben" : "Ton stummschalten",
      hint: "Server",
    },
    { id: "move", label: "In Raum verschieben …", separatorBefore: true },
  ];
  if (m.raum !== modell.raeume.find((r) => r.eltern === null)?.id) {
    punkte.push({ id: "kick-room", label: "Aus Raum werfen" });
  }
  punkte.push({ id: "role", label: "Rolle setzen …", separatorBefore: true });
  punkte.push({ id: "kick", label: "Vom Server trennen …", separatorBefore: true, danger: true });
  punkte.push({ id: "ban", label: "Sperren …", danger: true });
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
      zielRaumWahl.value = m?.rolle ?? null;
      dialog.value = "umbenennen"; // wird unten in „ro lle" umgedeutet
      dialog.value = "verschieben";
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
  const punkte: UI.MenuItem[] = [
    { id: "enter", label: "Betreten" },
    { id: "new", label: "Unterraum anlegen …", separatorBefore: true },
    { id: "rename", label: "Umbenennen …" },
  ];
  if (darfRaeumeVerwalten.value) {
    punkte.push({ id: "delete", label: "Raum löschen …", separatorBefore: true, danger: true });
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
    await legeRaumAn(eingabeName.value, meinRaum.value?.id ?? null);
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
  <div class="ansicht">
    <!-- Räume -->
    <UI.WpSidePanel title="Räume" :count="modell.raeume.length" width="18rem">
      <template #actions>
        <UI.WpIconButton
          v-if="darfRaeumeVerwalten"
          :icon="IconPlus"
          label="Raum anlegen"
          size="sm"
          variant="ghost"
          @press="(eingabeName = ''), (zielRaum = null), (dialog = 'raum')"
        />
      </template>

      <div class="raeume">
        <section v-for="raum in raumBaum" :key="raum.id" class="gruppe">
          <div class="kachelzeile" @contextmenu="oeffneRaumMenue(raum.id, $event)">
            <UI.WpChoiceCard
              :title="raum.name"
              :meta="kachelMeta(raum.id, raum.anwesend)"
              :active="raum.hier"
              @press="betreten(raum.id)"
            >
              <template #trailing>
                <UI.WpBadge v-if="raum.anwesend > 0" :label="String(raum.anwesend)" tone="neutral" />
              </template>
            </UI.WpChoiceCard>
            <UI.WpMenu
              :items="raumMenuePunkte()"
              label="Raum-Menü"
              align="left"
              :open="offenerRaum === raum.id"
              @update:open="(offen: boolean) => (offenerRaum = offen ? raum.id : null)"
              @select="(wahl: string) => raumMenueGewaehlt(raum.id, wahl)"
            >
              <UI.WpIconButton :icon="IconMore" label="Raum-Menü" size="sm" variant="ghost" />
            </UI.WpMenu>
          </div>

          <div v-if="raum.kinder.length" class="kinder">
            <div
              v-for="kind in raum.kinder"
              :key="kind.id"
              class="kachelzeile"
              @contextmenu="oeffneRaumMenue(kind.id, $event)"
            >
              <UI.WpChoiceCard
                :title="kind.name"
                :meta="kachelMeta(kind.id, kind.anwesend)"
                :active="kind.hier"
                @press="betreten(kind.id)"
              >
                <template #trailing>
                  <UI.WpBadge v-if="kind.anwesend > 0" :label="String(kind.anwesend)" tone="neutral" />
                </template>
              </UI.WpChoiceCard>
              <UI.WpMenu
                :items="raumMenuePunkte()"
                label="Raum-Menü"
                align="left"
                :open="offenerRaum === kind.id"
                @update:open="(offen: boolean) => (offenerRaum = offen ? kind.id : null)"
                @select="(wahl: string) => raumMenueGewaehlt(kind.id, wahl)"
              >
                <UI.WpIconButton :icon="IconMore" label="Raum-Menü" size="sm" variant="ghost" />
              </UI.WpMenu>
            </div>
          </div>
        </section>
      </div>

      <template #footer>
        <UI.WpButton v-if="meinRaum" label="Raum verlassen" size="sm" variant="ghost" @press="verlassen" />
      </template>
    </UI.WpSidePanel>

    <!-- Anwesende -->
    <UI.WpSidePanel title="Hier" :count="hierAnwesend.length" width="16rem">
      <ul class="menschen">
        <li
          v-for="m in hierAnwesend"
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
            size="md"
          />
          <span class="mensch__text">
            <span class="mensch__name">
              {{ m.name }}<span v-if="m.ich" class="mensch__ich"> · du</span>
            </span>
            <span class="mensch__rolle">
              {{ m.rolle }}
              <template v-if="m.stumm"> · stumm</template>
              <template v-if="m.taub"> · taub</template>
            </span>
          </span>

          <UI.WpMenu
            v-if="!m.ich && darfEingreifen"
            :items="mitgliedMenue(m.id)"
            :label="`Menü für ${m.name}`"
            :open="offenesMitglied === m.id"
            @update:open="(offen: boolean) => (offenesMitglied = offen ? m.id : null)"
            @select="(wahl: string) => mitgliedMenueGewaehlt(m.id, wahl)"
          >
            <UI.WpIconButton :icon="IconMore" :label="`Menü für ${m.name}`" size="sm" variant="ghost" />
          </UI.WpMenu>
        </li>
        <li v-if="hierAnwesend.length === 0" class="leer">
          <UI.WpText variant="caption" tone="muted">niemand hier</UI.WpText>
        </li>
      </ul>

      <template #footer>
        <UI.WpText variant="caption" tone="muted">
          <IconInfo /> Rechtsklick auf eine Person öffnet die Eingriffe.
        </UI.WpText>
      </template>
    </UI.WpSidePanel>

    <!-- Verlauf -->
    <section class="haupt">
      <header class="kopf">
        <div class="kopf__text">
          <h1>{{ meinRaum ? meinRaum.name : "Kein Raum" }}</h1>
          <p>{{ meinRaum ? anwesenheitText() : "Wähle links einen Raum." }}</p>
        </div>
        <div class="kopf__rechts">
          <UI.WpBadge v-if="sprechender" :label="`${sprechender} spricht`" tone="ok" />
          <UI.WpBadge
            v-if="meinRaum"
            :label="hierAnwesend.filter((m) => m.stumm).length ? `${hierAnwesend.filter((m) => m.stumm).length} stumm` : 'Mikrofone offen'"
            :tone="hierAnwesend.some((m) => m.stumm) ? 'warn' : 'neutral'"
          />
          <UI.WpButton
            v-if="meinRaum && darfRaeumeVerwalten"
            label="Löschen"
            size="sm"
            variant="ghost"
            :icon="IconTrash"
            @press="(zielRaum = meinRaum.id), (dialog = 'loeschen')"
          />
        </div>
      </header>

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

        <article
          v-for="n in modell.nachrichten"
          :key="n.id"
          class="beitrag"
          :class="{ 'ist-eigene': n.verfasser === eigeneId }"
        >
          <UI.WpAvatar :name="n.verfasser_name" size="md" :tone="n.verfasser === eigeneId ? 'accent' : 'neutral'" />
          <div class="beitrag__text">
            <header class="beitrag__kopf">
              <span class="beitrag__wer">{{ n.verfasser_name }}</span>
              <span class="beitrag__wann">{{ uhrzeit(n.zeit) }}</span>
              <button v-if="n.loeschbar" class="beitrag__aktion" type="button" @click="loesche(n.id)">
                löschen
              </button>
            </header>
            <p v-if="n.geloescht" class="beitrag__weg">Nachricht gelöscht</p>
            <p v-else class="beitrag__inhalt">{{ n.text }}</p>
          </div>
        </article>
      </div>

      <div v-if="meinRaum" class="schreiben">
        <UI.WpInput v-model="eingabe" placeholder="Schreiben … (Enter sendet)" @keyup.enter="absenden" />
        <UI.WpButton
          label="Senden"
          variant="primary"
          :state="eingabe.trim() ? 'default' : 'disabled'"
          @press="absenden"
        />
      </div>
    </section>

    <!-- Dialoge: nur wo eine Eingabe nötig ist (Menüs verschachteln nichts). -->
    <UI.WpDialog
      :open="dialog === 'raum'"
      title="Unterraum anlegen"
      description="Der Raum entsteht unter dem aktuellen Raum."
      confirm-label="Anlegen"
      @confirm="bestaetigen"
      @cancel="dialog = null"
    >
      <UI.WpInput v-model="eingabeName" placeholder="Name des Raums" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'umbenennen'"
      title="Raum umbenennen"
      :description="raumName"
      confirm-label="Umbenennen"
      @confirm="bestaetigen"
      @cancel="dialog = null"
    >
      <UI.WpInput v-model="eingabeName" placeholder="Neuer Name" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'loeschen'"
      title="Raum löschen"
      :description="`„${raumName}“ wird gelöscht. Nur möglich, wenn keine Unterräume mehr darin sind; die Nachrichten des Raums verschwinden mit.`"
      tone="warn"
      confirm-label="Löschen"
      @confirm="bestaetigen"
      @cancel="dialog = null"
    />

    <UI.WpDialog
      :open="dialog === 'verschieben'"
      :title="`${zielName} verschieben`"
      description="Wähle den Zielraum. Die Person wird sofort dorthin bewegt."
      confirm-label="Verschieben"
      @confirm="bestaetigen"
      @cancel="dialog = null"
    >
      <UI.WpSelect v-model="zielRaumWahl" :options="raumWahl" label="Zielraum" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'kicken'"
      :title="`${zielName} vom Server trennen`"
      description="Die Person fliegt aus dem Gespräch und kann sofort wieder beitreten. Der Grund erscheint bei ihr."
      tone="warn"
      confirm-label="Trennen"
      @confirm="bestaetigen"
      @cancel="dialog = null"
    >
      <UI.WpInput v-model="eingabeGrund" placeholder="Grund (optional)" />
    </UI.WpDialog>

    <UI.WpDialog
      :open="dialog === 'sperren'"
      :title="`${zielName} sperren`"
      description="Die Kennung wird gesperrt; ein Wiederkommen ist mit diesem Gerät nicht möglich. Das lässt sich nur über das Prüfprotokoll zurücknehmen."
      tone="error"
      confirm-label="Sperren"
      @cancel="dialog = null"
      @confirm="bestaetigen"
    >
      <UI.WpInput v-model="eingabeGrund" placeholder="Grund (erscheint im Protokoll)" />
    </UI.WpDialog>
  </div>
</template>

<style scoped>
.ansicht {
  display: flex;
  flex: 1;
  min-height: 0;
}

/* ---- Räume ---- */
.raeume {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
  padding: var(--wp-space-1);
}

.gruppe {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
}

.kachelzeile {
  position: relative;
  display: flex;
  align-items: stretch;
  gap: var(--wp-space-1);
}

.kachelzeile > :first-child {
  flex: 1;
  min-width: 0;
}

/* Der Menü-Knopf erscheint erst bei Hover oder Fokus — die Kachel bleibt ruhig. */
.kachelzeile > :last-child {
  align-self: center;
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.kachelzeile:hover > :last-child,
.kachelzeile:focus-within > :last-child {
  opacity: 1;
}

.kinder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
  margin-left: var(--wp-space-3);
  padding-left: var(--wp-space-3);
  border-left: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

/* ---- Menschen ---- */
.menschen {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.mensch {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding: var(--wp-space-1) var(--wp-space-2);
}

.mensch:hover {
  background: var(--wp-color-bg-hover);
}

.mensch__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}

.mensch__name {
  font-size: var(--wp-font-size-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mensch__ich {
  color: var(--wp-color-text-faint);
}

.mensch__rolle {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.mensch > :last-child {
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.mensch:hover > :last-child,
.mensch:focus-within > :last-child {
  opacity: 1;
}

.leer {
  padding: var(--wp-space-2);
}

/* ---- Verlauf ---- */
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

.beitrag {
  display: flex;
  gap: var(--wp-space-3);
  padding: var(--wp-space-1) 0 var(--wp-space-3);
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

<script setup lang="ts">
// Prüfprotokoll: das, was dieses Projekt von TeamSpeak unterscheidet.
//
// Nicht „Logdatei für den Betreiber", sondern im Fenster sichtbar: wer hat was
// getan, und — genauso wichtig — was wurde **abgelehnt**. Abgelehnte Versuche
// sind die interessanten Zeilen: dort steht, dass jemand etwas wollte, was er
// nicht durfte.
//
// Die Ansicht rechnet nichts: sie zeigt `modell.protokoll`, so wie es aus dem
// Modell kommt (AGENTS.md §2.1). Geladen wird ausdrücklich über den Knopf, weil
// das Protokoll wächst und nicht bei jeder Änderung mitgeschickt werden soll.
import { computed, onMounted } from "vue";
import * as UI from "@weppelmann/components";
import { IconRefresh } from "@weppelmann/icons";
import { darfProtokollLesen, ladeProtokoll, modell, t } from "../store";

/** Darf ich das Protokoll lesen? */
const darf = computed(() => darfProtokollLesen.value);

/** Zeilen mit Zeit, Handelndem, Aktion, Ziel und Ergebnis. */
const zeilen = computed(() =>
  modell.protokoll.map((e) => ({
    ...e,
    zeitText: zeitText(e.zeit),
    wer: e.handelnder_name || t("audit.system"),
    // Aktion übersetzen, sonst den technischen Namen zeigen — sichtbar statt leer.
    was: t(`audit.action.${e.aktion}`),
    abgelehnt: e.ergebnis.startsWith("abgelehnt"),
  })),
);

function zeitText(ms: number): string {
  if (!ms) return "—";
  const d = new Date(ms);
  const zwei = (n: number) => String(n).padStart(2, "0");
  return `${zwei(d.getHours())}:${zwei(d.getMinutes())}:${zwei(d.getSeconds())}`;
}

function laden() {
  if (darf.value) void ladeProtokoll();
}

onMounted(laden);
</script>

<template>
  <section class="protokoll">
    <header class="kopf">
      <div>
        <h2>{{ t("audit.title") }}</h2>
        <p class="unter">{{ t("audit.sub") }}</p>
      </div>
      <UI.WpButton
        :label="t('audit.refresh')"
        :icon="IconRefresh"
        variant="secondary"
        size="sm"
        :state="darf ? undefined : 'disabled'"
        @press="laden"
      />
    </header>

    <p v-if="!darf" class="hinweis">{{ t("audit.no_right") }}</p>

    <template v-else>
      <UI.WpEmptyState
        v-if="zeilen.length === 0"
        :title="t('audit.empty.title')"
        :desc="t('audit.empty.desc')"
      />

      <table v-else class="tabelle">
        <thead>
          <tr>
            <th class="zeit">{{ t("audit.col.time") }}</th>
            <th class="wer">{{ t("audit.col.who") }}</th>
            <th>{{ t("audit.col.what") }}</th>
            <th class="ziel">{{ t("audit.col.target") }}</th>
            <th class="ergebnis">{{ t("audit.col.result") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="z in zeilen" :key="z.laufnummer" :class="{ abgelehnt: z.abgelehnt }">
            <td class="zeit">{{ z.zeitText }}</td>
            <td class="wer">{{ z.wer }}</td>
            <td>
              {{ z.was }}
              <span v-if="z.vorher || z.nachher" class="wechsel">
                {{ z.vorher ?? "—" }} → {{ z.nachher ?? "—" }}
              </span>
              <span v-if="z.grund" class="grund">· {{ z.grund }}</span>
            </td>
            <td class="ziel">{{ z.ziel || "—" }}</td>
            <td class="ergebnis">
              <span v-if="z.abgelehnt" class="abzeichen">{{ t("audit.refused") }}</span>
              {{ z.ergebnis }}
            </td>
          </tr>
        </tbody>
      </table>
    </template>
  </section>
</template>

<style scoped>
.protokoll {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-4);
  padding: var(--wp-space-5);
  height: 100%;
  overflow: auto;
}

.kopf {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--wp-space-4);
}

h2 {
  margin: 0;
  font-size: var(--wp-font-size-lg);
  font-weight: var(--wp-font-weight-semibold);
  color: var(--wp-color-text-default);
}

.unter {
  margin: var(--wp-space-1) 0 0;
  color: var(--wp-color-text-muted);
  max-width: 60ch;
}

.hinweis {
  margin: 0;
  padding: var(--wp-space-4);
  border-radius: var(--wp-radius-md);
  background: var(--wp-color-bg-panel);
  color: var(--wp-color-text-muted);
}

.tabelle {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--wp-font-size-md);
}

th {
  text-align: left;
  padding: var(--wp-space-2) var(--wp-space-3);
  color: var(--wp-color-text-muted);
  font-weight: var(--wp-font-weight-semibold);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-strong);
}

td {
  padding: var(--wp-space-2) var(--wp-space-3);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
  vertical-align: top;
}

/* Abgelehnte Versuche sind die interessante Hälfte — sie bleiben gedämpft,
   aber erkennbar: kein Alarmrot, nur eine ruhige Markierung. */
tr.abgelehnt td {
  color: var(--wp-color-text-muted);
}

.zeit,
.wer {
  white-space: nowrap;
}

.wechsel {
  color: var(--wp-color-text-muted);
  margin-left: var(--wp-space-2);
}

/* Der Grund gehört zur Handlung, nicht zum Ziel — deshalb hier und nicht in
   der Zielspalte. */
.grund {
  color: var(--wp-color-text-muted);
  margin-left: var(--wp-space-2);
}

.abzeichen {
  display: inline-block;
  padding: 0 var(--wp-space-2);
  margin-right: var(--wp-space-2);
  border-radius: var(--wp-radius-sm);
  background: var(--wp-color-bg-raised);
  color: var(--wp-color-text-muted);
  font-size: 0.85em;
}

.ergebnis {
  max-width: 32ch;
}
</style>

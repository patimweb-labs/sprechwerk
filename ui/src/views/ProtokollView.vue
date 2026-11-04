<script setup lang="ts">
// Prüfprotokoll: wer hat wann was getan, mit Ergebnis. Auch abgelehnte
// Versuche stehen hier — gerade die sind interessant.
import { computed, onMounted } from "vue";
import * as UI from "@weppelmann/components";
import { holeProtokoll, modell, uhrzeit } from "../store";

onMounted(holeProtokoll);

const spalten = [
  { key: "zeit", label: "Zeit", width: "5rem" },
  { key: "wer", label: "Wer", width: "10rem" },
  { key: "aktion", label: "Aktion", width: "12rem" },
  { key: "ziel", label: "Ziel" },
  { key: "ergebnis", label: "Ergebnis", width: "14rem" },
];

const tabelle = computed<UI.TableState>(() => {
  if (modell.protokoll.length === 0) {
    return { kind: "leer", text: "Noch keine Einträge." };
  }
  return {
    kind: "fertig",
    rows: modell.protokoll.map((e) => ({
      zeit: uhrzeit(e.zeit),
      wer: e.handelnder_name || "—",
      aktion: e.aktion,
      ziel: e.ziel,
      ergebnis: e.ergebnis,
    })),
  };
});
</script>

<template>
  <div class="ansicht">
    <UI.WpToolbar
      title="Prüfprotokoll"
      subtitle="Jede Änderung, auch jeder abgelehnte Versuch."
    >
      <template #actions>
        <UI.WpButton label="Neu laden" size="sm" @press="holeProtokoll" />
      </template>
    </UI.WpToolbar>
    <div class="tabelle">
      <UI.WpTable :columns="spalten" :state="tabelle" caption="Die letzten Einträge" />
    </div>
  </div>
</template>

<style scoped>
.ansicht {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.tabelle {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-3) var(--wp-space-4);
}
</style>

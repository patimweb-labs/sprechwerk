<script setup lang="ts">
// Fensterhülle: Kopfleiste, Ansichtenleiste, Bildschirm, Fußzeile.
// Alles, was ein Baustein sein kann, kommt aus der Bibliothek
// (`@weppelmann/components`); in dieser Datei steht nur, was sprechwerk eigen ist.
//
// Die Ansichten hängen am Modell: „Protokoll" erscheint nur mit Leserecht,
// „Räume" nur mit Verbindung.
import { computed } from "vue";
import type { Component } from "vue";
import * as UI from "@weppelmann/components";
import { IconFolder, IconInfo, IconSettings, IconWifi, IconWifiOff } from "@weppelmann/icons";
import { ansicht, darfProtokollLesen, modell, trennen, verbunden } from "./store";
import VerbindenView from "./views/VerbindenView.vue";
import RaeumeView from "./views/RaeumeView.vue";
import ProtokollView from "./views/ProtokollView.vue";
import EinstellungenView from "./views/EinstellungenView.vue";

const seiten = computed(() => {
  const liste: { id: string; label: string; icon: Component; count?: string }[] = [
    { id: "raeume", label: "Räume", icon: IconFolder, count: String(modell.raeume.length || "") },
  ];
  if (darfProtokollLesen.value) {
    liste.push({ id: "protokoll", label: "Protokoll", icon: IconInfo });
  }
  liste.push({ id: "einstellungen", label: "Einstellungen", icon: IconSettings });
  return liste;
});

/** Kopfzeile: Name und Zustand. */
const kopf = computed(() => {
  const v = modell.verbindung;
  if (v?.art === "verbunden") {
    return { titel: v.server_name, untertitel: `${v.name} · ${v.rolle}` };
  }
  if (v?.art === "verbindet") return { titel: "Sprechwerk", untertitel: `verbinde zu ${v.server}` };
  if (v?.art === "fehler") return { titel: "Sprechwerk", untertitel: v.text };
  return { titel: "Sprechwerk", untertitel: "nicht verbunden" };
});

const anwesende = computed(() => modell.mitglieder.length);
const nachrichten = computed(() => modell.nachrichten.length);
</script>

<template>
  <div class="fenster">
    <UI.WpAppBar :title="kopf.titel" :subtitle="kopf.untertitel" :icon="verbunden ? IconWifi : IconWifiOff">
      <template #status>
        <UI.WpBadge v-if="verbunden" label="verbunden" tone="ok" />
        <UI.WpBadge v-else label="getrennt" tone="neutral" />
      </template>
      <template #actions>
        <UI.WpButton v-if="verbunden" label="Trennen" size="sm" @press="trennen" />
      </template>
    </UI.WpAppBar>

    <div class="rumpf">
      <UI.WpNavList
        v-model="ansicht"
        :entries="seiten"
        title="Ansicht"
        label="Ansichten"
        class="leiste"
      />

      <main class="bildschirm">
        <VerbindenView v-if="!verbunden" />
        <template v-else>
          <RaeumeView v-if="ansicht === 'raeume'" />
          <ProtokollView v-else-if="ansicht === 'protokoll'" />
          <EinstellungenView v-else />
        </template>
      </main>
    </div>

    <UI.WpStatusBar
      brand="SPRECHWERK"
      :state="kopf.untertitel"
      :counts="verbunden ? `${anwesende} anwesend · ${nachrichten} im Raum` : ''"
    >
      <template #lead>
        <span v-if="modell.meldung" class="meldung">{{ modell.meldung }}</span>
      </template>
    </UI.WpStatusBar>
  </div>
</template>

<style scoped>
.fenster {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.rumpf {
  display: flex;
  flex: 1;
  min-height: 0;
}

.leiste {
  flex: none;
  border-right: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.bildschirm {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.meldung {
  color: var(--wp-color-status-error);
}
</style>

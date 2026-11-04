<script setup lang="ts">
// Startseite: gemerkte Server (Favoriten). Von hier verbindet man sich, hier
// verwaltet man die Liste. Ohne Verbindung ist das der erste Bildschirm.
//
// Kein Hauptweg versteckt: „Verbinden" steht auf jeder Zeile, nicht in einem
// Menü (Kontextmenüs sind für Nebenwege, siehe ADR 0008).
import { computed, ref } from "vue";
import * as UI from "@weppelmann/components";
import { IconPlug, IconPlus, IconRefresh, IconTrash } from "@weppelmann/icons";
import {
  einstellungen,
  favoritEntfernen,
  favoritHinzufuegen,
  laeuft,
  modell,
  verbindeMit,
  verbunden,
} from "../store";

const zeigeHinzufuegen = ref(false);
const neuerName = ref("");
const neueAdresse = ref("");
const zeigeEntfernen = ref<string | null>(null);

/** Server, die zur Auswahl stehen: die Favoriten und der zuletzt benutzte. */
const server = computed(() => {
  const liste = einstellungen.favoriten.map((f) => ({
    name: f.name || f.server,
    server: f.server,
    gemerkt: true,
  }));
  const letzter = einstellungen.server;
  if (letzter && !liste.some((s) => s.server === letzter)) {
    liste.unshift({ name: letzter, server: letzter, gemerkt: false });
  }
  return liste;
});

async function verbindenMit(adresse: string) {
  await verbindeMit(adresse);
}

async function hinzufuegen() {
  const name = neuerName.value.trim();
  const adresse = neueAdresse.value.trim();
  zeigeHinzufuegen.value = false;
  neuerName.value = "";
  neueAdresse.value = "";
  if (!adresse) return;
  await favoritHinzufuegen({ name: name || adresse, server: adresse });
}

async function entfernen() {
  const adresse = zeigeEntfernen.value;
  zeigeEntfernen.value = null;
  if (adresse) await favoritEntfernen(adresse);
}
</script>

<template>
  <div class="ansicht">
    <header class="kopf">
      <div>
        <h1>Server</h1>
        <p>Gemerkte Server. Ein Klick verbindet — kein Suchen nach der Adresse.</p>
      </div>
      <div class="kopf__rechts">
        <UI.WpButton
          label="Server merken"
          size="sm"
          variant="secondary"
          :icon="IconPlus"
          @press="zeigeHinzufuegen = true"
        />
      </div>
    </header>

    <div class="inhalt">
      <UI.WpEmptyState
        v-if="server.length === 0"
        title="Noch kein Server gemerkt"
        description="Trage die Adresse deines Servers ein — danach steht sie hier."
      >
        <template #action>
          <UI.WpButton label="Server merken" variant="primary" @press="zeigeHinzufuegen = true" />
        </template>
      </UI.WpEmptyState>

      <ul v-else class="liste">
        <li v-for="s in server" :key="s.server" class="zeile">
          <UI.WpChoiceCard
            :title="s.name"
            :meta="s.server"
            :active="verbunden && einstellungen.server === s.server"
            :clickable="false"
          />
          <div class="zeile__aktionen">
            <UI.WpBadge v-if="s.gemerkt" label="gemerkt" tone="neutral" />
            <UI.WpButton
              :label="laeuft ? 'verbinde …' : 'Verbinden'"
              :state="laeuft ? 'busy' : 'default'"
              size="sm"
              variant="primary"
              :icon="IconPlug"
              @press="verbindenMit(s.server)"
            />
            <UI.WpIconButton
              v-if="s.gemerkt"
              :icon="IconTrash"
              label="Aus der Liste entfernen"
              size="sm"
              variant="ghost"
              @press="zeigeEntfernen = s.server"
            />
          </div>
        </li>
      </ul>

      <UI.WpText variant="caption" tone="muted">
        <IconRefresh /> Die Kennung liegt auf diesem Rechner (Schlüsseldatei). Auf einem anderen
        Gerät nimmst du sie mit — sonst bist du dort ein neues Mitglied.
      </UI.WpText>

      <p v-if="modell.meldung" class="meldung">{{ modell.meldung }}</p>
    </div>

    <UI.WpDialog
      :open="zeigeHinzufuegen"
      title="Server merken"
      description="Name ist frei wählbar; die Adresse ist Host und Port, etwa verein.example:7443."
      confirm-label="Merken"
      @confirm="hinzufuegen"
      @cancel="zeigeHinzufuegen = false"
    >
      <div class="dialogfelder">
        <UI.WpInput v-model="neuerName" placeholder="Name, z. B. Trainingsabend" />
        <UI.WpInput v-model="neueAdresse" placeholder="Adresse, z. B. 192.0.2.10:7443" />
      </div>
    </UI.WpDialog>

    <UI.WpDialog
      :open="zeigeEntfernen !== null"
      title="Server aus der Liste entfernen"
      description="Die Verbindung bleibt bestehen; nur der Eintrag verschwindet."
      tone="warn"
      confirm-label="Entfernen"
      @confirm="entfernen"
      @cancel="zeigeEntfernen = null"
    />
  </div>
</template>

<style scoped>
.ansicht {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.kopf {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  padding: var(--wp-space-4);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.kopf h1 {
  margin: 0;
  font-size: var(--wp-font-size-xl);
  font-weight: var(--wp-font-weight-semibold);
}

.kopf p {
  margin: var(--wp-space-1) 0 0;
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
}

.kopf__rechts {
  margin-left: auto;
}

.inhalt {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-4);
  max-width: 56rem;
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-4);
}

.liste {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-2);
}

.zeile {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
}

.zeile > :first-child {
  flex: 1;
  min-width: 0;
}

.zeile__aktionen {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
}

.dialogfelder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-2);
}

.meldung {
  margin: 0;
  color: var(--wp-color-status-error);
  font-size: var(--wp-font-size-sm);
}
</style>

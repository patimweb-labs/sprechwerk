<script setup lang="ts">
// Einstellungen des Fensters. Nur, was der Client selbst braucht: Adresse,
// Name, Farbschema, Sprache und die Ton-Auswahl. Was den Server betrifft,
// steht in dessen Konfigurationsdatei — nicht hier (eine Wahrheit).
import { computed } from "vue";
import * as UI from "@weppelmann/components";
import { IconLanguage, IconPalette } from "@weppelmann/icons";
import { einstellungen, meinRaum, modell, speichere, verbunden, wendeThema } from "../store";

const themen = [
  { value: "dark", label: "Dunkel" },
  { value: "light", label: "Hell" },
  { value: "mono", label: "Mono" },
];

const sprachen = [
  { value: "de", label: "Deutsch" },
  { value: "en", label: "English" },
];

const geraete = [{ value: "", label: "Vorgabe des Systems", disabled: false }];

/** Verbindungsdaten als Liste — nur wenn verbunden. */
const fakten = computed(() => {
  const v = modell.verbindung;
  if (v?.art !== "verbunden") return [];
  return [
    { key: "Server", value: v.server, mono: true },
    { key: "Name", value: v.server_name },
    { key: "Ich", value: v.name },
    { key: "Rolle", value: v.rolle },
    { key: "Kennung", value: v.ich, mono: true },
  ];
});

async function thema(wert: string) {
  wendeThema(wert);
  await speichere();
}
</script>

<template>
  <div class="ansicht">
    <UI.WpToolbar title="Einstellungen" subtitle="Was dieses Fenster betrifft — der Rest steht in der Serverkonfiguration." />

    <div class="inhalt">
      <UI.WpCard title="Verbindung">
        <div class="felder">
          <UI.WpFormField label="Server">
            <template #default="{ id }">
              <UI.WpInput :id="id" v-model="einstellungen.server" placeholder="127.0.0.1:7443" />
            </template>
          </UI.WpFormField>
          <UI.WpFormField label="Name">
            <template #default="{ id }">
              <UI.WpInput :id="id" v-model="einstellungen.name" placeholder="Wie heißt du?" />
            </template>
          </UI.WpFormField>
          <UI.WpFormField label="Serverpasswort" hint="Nur wenn der Server eines verlangt.">
            <template #default="{ id }">
              <UI.WpInput :id="id" v-model="einstellungen.passwort" type="password" />
            </template>
          </UI.WpFormField>
          <UI.WpFactList v-if="fakten.length" :facts="fakten" />
        </div>
      </UI.WpCard>

      <UI.WpCard title="Darstellung">
        <div class="felder">
          <div class="feld">
            <UI.WpToggleGroup
              :model-value="einstellungen.thema"
              :options="themen"
              label="Farbschema"
              @update:model-value="thema"
            />
          </div>
          <UI.WpFormField label="Sprache">
            <template #default="{ id }">
              <UI.WpSelect :id="id" v-model="einstellungen.sprache" :options="sprachen" />
            </template>
          </UI.WpFormField>
          <UI.WpText variant="caption" tone="muted">
            <IconPalette /> Drei Schemata, kein Selbstbau: dunkel, hell, mono.
          </UI.WpText>
        </div>
      </UI.WpCard>

      <UI.WpCard title="Ton">
        <div class="felder">
          <UI.WpFormField
            label="Mikrofon"
            hint="Die Aufnahme braucht die Systempakete für ALSA und Opus (docs/AUDIO.md)."
          >
            <template #default="{ id }">
              <UI.WpSelect :id="id" v-model="einstellungen.mikrofon" :options="geraete" state="disabled" />
            </template>
          </UI.WpFormField>
          <UI.WpFormField label="Wiedergabe">
            <template #default="{ id }">
              <UI.WpSelect :id="id" v-model="einstellungen.lautsprecher" :options="geraete" state="disabled" />
            </template>
          </UI.WpFormField>
          <UI.WpSwitch v-model="einstellungen.push_to_talk" label="Push-to-Talk" />
          <UI.WpFormField
            label="Taste"
            hint="Nur bei Push-to-Talk. TeamSpeak wartet 300 ms — wir schicken den Vorlauf mit."
          >
            <template #default="{ id }">
              <UI.WpInput :id="id" v-model="einstellungen.ptt_taste" />
            </template>
          </UI.WpFormField>
          <UI.WpSlider v-model="einstellungen.schwelle" label="Sprechschwelle" :min="0" :max="100" suffix="%" />
          <UI.WpSlider v-model="einstellungen.vorlauf_ms" label="Vorlauf" :min="0" :max="200" :step="10" suffix="ms" />
          <UI.WpText variant="caption" tone="muted">
            <IconLanguage /> Sprache der Oberfläche; die Tonauswahl ist noch gesperrt, bis der Audiopfad gebaut ist.
          </UI.WpText>
        </div>
      </UI.WpCard>

      <div class="aktionen">
        <UI.WpButton label="Speichern" variant="primary" @press="speichere" />
        <UI.WpText v-if="verbunden && meinRaum" variant="caption" tone="muted">
          Im Raum: {{ meinRaum.name }}
        </UI.WpText>
      </div>
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

.inhalt {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-3) var(--wp-space-4);
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
  max-width: 46rem;
}

.felder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
}

.feld {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
}

.aktionen {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  padding-bottom: var(--wp-space-4);
}
</style>

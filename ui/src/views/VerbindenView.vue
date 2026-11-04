<script setup lang="ts">
// Anmeldung. Kein Konto, kein Passwort für die Person — nur Server, Name und
// (falls der Server es verlangt) das Serverpasswort.
import * as UI from "@weppelmann/components";
import { IconPlug } from "@weppelmann/icons";
import { einstellungen, laeuft, modell, verbinden } from "../store";

async function absenden() {
  if (!einstellungen.name.trim()) return;
  await verbinden();
}
</script>

<template>
  <div class="mitte">
    <UI.WpCard title="Verbinden" subtitle="Ein Name, eine Adresse — mehr braucht es nicht.">
      <div class="felder">
        <UI.WpFormField label="Server">
          <template #default="{ id, describedby }">
            <UI.WpInput
              :id="id"
              v-model="einstellungen.server"
              :aria-describedby="describedby"
              placeholder="127.0.0.1:7443"
            />
          </template>
        </UI.WpFormField>

        <UI.WpFormField label="Name" hint="So sehen dich die anderen.">
          <template #default="{ id, describedby }">
            <UI.WpInput
              :id="id"
              v-model="einstellungen.name"
              :aria-describedby="describedby"
              placeholder="Wie heißt du?"
            />
          </template>
        </UI.WpFormField>

        <UI.WpFormField label="Serverpasswort" hint="Nur wenn der Server eines verlangt.">
          <template #default="{ id, describedby }">
            <UI.WpInput
              :id="id"
              v-model="einstellungen.passwort"
              type="password"
              :aria-describedby="describedby"
            />
          </template>
        </UI.WpFormField>

        <UI.WpButton
          label="Verbinden"
          variant="primary"
          :state="laeuft ? 'busy' : !einstellungen.name ? 'disabled' : 'default'"
          :icon="IconPlug"
          @press="absenden"
        />
        <p v-if="modell.verbindung?.art === 'fehler'" class="fehler">
          {{ modell.verbindung.text }}
        </p>
      </div>
    </UI.WpCard>
  </div>
</template>

<style scoped>
.mitte {
  display: flex;
  justify-content: center;
  padding: var(--wp-space-7) var(--wp-space-4);
}

.felder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
  width: 22rem;
}

.fehler {
  color: var(--wp-color-status-error);
  font-size: var(--wp-font-size-sm);
  margin: 0;
}
</style>

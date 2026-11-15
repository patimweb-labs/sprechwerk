<script setup lang="ts">
// Sperrliste: wer ist gesperrt, warum, von wem — und ein Weg zurück.
//
// Bis eben gab es nur „Sperren". Eine Sperre, die man nicht aufheben kann, ist
// im Betrieb eine Falle: man muss an die Datenbank. Hier steht beides
// nebeneinander, und die Liste kommt aus dem Server (nicht aus dem Gedächtnis
// des Fensters).
import { computed, onMounted, ref } from "vue";
import * as UI from "@weppelmann/components";
import { darfSperren, entsperren, ladeSperrliste, modell, t } from "../store";

const darf = computed(() => darfSperren.value);

/** Welche Zeile wird gerade aufgehoben? (Doppelklick verhindern) */
const laeuft = ref<string | null>(null);

const zeilen = computed(() =>
  modell.sperren.map((s, i) => ({
    ...s,
    schluessel: `${s.zeit}-${i}`,
    was: s.kennung ? kurz(s.kennung) : s.adresse || "—",
    art: s.kennung ? t("bans.kind_key") : t("bans.kind_address"),
    zeitText: zeitText(s.zeit),
  })),
);

/** Kennungen sind lang; die ersten acht Zeichen genügen zum Erkennen. */
function kurz(kennung: string): string {
  return kennung.length > 12 ? `${kennung.slice(0, 8)}…` : kennung;
}

function zeitText(ms: number): string {
  if (!ms) return "—";
  const d = new Date(ms);
  const zwei = (n: number) => String(n).padStart(2, "0");
  return `${zwei(d.getDate())}.${zwei(d.getMonth() + 1)}. ${zwei(d.getHours())}:${zwei(d.getMinutes())}`;
}

async function aufheben(kennung: string | null, adresse: string | null, marke: string) {
  if (laeuft.value) return;
  laeuft.value = marke;
  await entsperren(kennung, adresse);
  await ladeSperrliste();
  laeuft.value = null;
}

onMounted(() => {
  if (darf.value) void ladeSperrliste();
});
</script>

<template>
  <section class="sperren">
    <header class="kopf">
      <div>
        <h2>{{ t("bans.title") }}</h2>
        <p class="unter">{{ t("bans.sub") }}</p>
      </div>
      <UI.WpButton
        :label="t('audit.refresh')"
        variant="secondary"
        size="sm"
        :state="darf ? undefined : 'disabled'"
        @press="ladeSperrliste()"
      />
    </header>

    <p v-if="!darf" class="hinweis">{{ t("bans.no_right") }}</p>

    <template v-else>
      <UI.WpEmptyState
        v-if="zeilen.length === 0"
        :title="t('bans.empty.title')"
        :desc="t('bans.empty.desc')"
      />

      <table v-else class="tabelle">
        <thead>
          <tr>
            <th>{{ t("bans.col.what") }}</th>
            <th class="art">{{ t("bans.col.kind") }}</th>
            <th>{{ t("bans.col.reason") }}</th>
            <th class="wer">{{ t("bans.col.by") }}</th>
            <th class="zeit">{{ t("audit.col.time") }}</th>
            <th class="aktion">{{ t("server.col.action") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="z in zeilen" :key="z.schluessel">
            <td class="werte">{{ z.was }}</td>
            <td class="art">{{ z.art }}</td>
            <td>{{ z.grund }}</td>
            <td class="wer">{{ z.handelnder_name || "—" }}</td>
            <td class="zeit">{{ z.zeitText }}</td>
            <td class="aktion">
              <UI.WpButton
                :label="t('bans.lift')"
                size="sm"
                variant="ghost"
                :state="laeuft === z.schluessel ? 'disabled' : undefined"
                @press="aufheben(z.kennung, z.adresse, z.schluessel)"
              />
            </td>
          </tr>
        </tbody>
      </table>
    </template>
  </section>
</template>

<style scoped>
.sperren {
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
  vertical-align: middle;
}

/* Kennungen und Adressen sind Maschinenwerte: gleiche Breite, gut lesbar. */
.werte {
  font-family: var(--wp-font-family-mono);
  font-size: 0.9em;
}

.art,
.wer,
.zeit {
  white-space: nowrap;
}

.aktion {
  text-align: right;
}
</style>

<script setup lang="ts">
// Serverliste: die Startseite. Eine Zeile je Server, gleiche Höhe, gleiche
// Spalten — kein Karten-Layout, keine Abzeichen, nichts, was unterschiedlich
// groß wirkt.
//
// „Gemerkte Server" ist der Normalfall; nur der zuletzt benutzte Eintrag, der
// nicht gemerkt ist, wird als solcher benannt (kleine Beschriftung, kein
// Abzeichen). Entfernen liegt im Menü der Zeile — Nebenwege gehören ins Menü,
// der Hauptweg („Verbinden") steht sichtbar.
import { computed, ref } from "vue";
import * as UI from "@weppelmann/components";
import { IconMore, IconPlug, IconPlus } from "@weppelmann/icons";
import {
  einstellungen,
  favoritEntfernen,
  favoritHinzufuegen,
  laeuft,
  modell,
  t,
  verbindeMit,
} from "../store";

const zeigeHinzufuegen = ref(false);
const neuerName = ref("");
const neueAdresse = ref("");
const zeigeEntfernen = ref<string | null>(null);
/** Welche Zeile hat ihr Menü offen? */
const offen = ref<string | null>(null);

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

/** Menü einer Zeile: Verbinden, Bearbeiten, Entfernen. */
function zeilenMenue(s: { server: string; gemerkt: boolean }): UI.MenuItem[] {
  const punkte: UI.MenuItem[] = [{ id: "connect", label: t("server.connect") }];
  if (s.gemerkt) {
    punkte.push({ id: "remove", label: t("server.remove"), separatorBefore: true, danger: true });
  }
  return punkte;
}

function zeilenMenueGewaehlt(s: { server: string; gemerkt: boolean }, wahl: string) {
  offen.value = null;
  if (wahl === "connect") void verbindeMit(s.server);
  else if (wahl === "remove") zeigeEntfernen.value = s.server;
}

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
      <div class="kopf__text">
        <h1>{{ t("server.title") }}</h1>
        <p>{{ t("server.sub") }}</p>
      </div>
      <UI.WpButton
        :label="t('server.add')"
        size="sm"
        variant="secondary"
        :icon="IconPlus"
        @press="zeigeHinzufuegen = true"
      />
    </header>

    <div class="inhalt">
      <div class="tabelle">
        <div class="tabelle__kopf">
          <span class="spalte spalte--name">{{ t("server.col.name") }}</span>
          <span class="spalte spalte--adresse">{{ t("server.col.address") }}</span>
          <span class="spalte spalte--aktion">{{ t("server.col.action") }}</span>
        </div>

        <p v-if="server.length === 0" class="leer">{{ t("server.empty") }}</p>

        <div v-for="s in server" :key="s.server" class="zeile">
          <span class="spalte spalte--name">
            <span class="name">{{ s.name }}</span>
            <span v-if="!s.gemerkt" class="hinweis">{{ t("server.last_used") }}</span>
          </span>
          <span class="spalte spalte--adresse adresse">{{ s.server }}</span>
          <span class="spalte spalte--aktion aktionen">
            <UI.WpButton
              :label="laeuft ? t('server.connecting') : t('server.connect')"
              :state="laeuft ? 'busy' : 'default'"
              size="sm"
              variant="secondary"
              :icon="IconPlug"
              @press="verbindenMit(s.server)"
            />
            <UI.WpMenu
              :items="zeilenMenue(s)"
              :label="t('server.menu_for', s.name)"
              :open="offen === s.server"
              @update:open="(istOffen: boolean) => (offen = istOffen ? s.server : null)"
              @select="(wahl: string) => zeilenMenueGewaehlt(s, wahl)"
            >
              <UI.WpIconButton :icon="IconMore" :label="t('server.menu_for', s.name)" size="sm" variant="ghost" />
            </UI.WpMenu>
          </span>
        </div>
      </div>

      <p class="fussnote">{{ t("server.footnote") }}</p>

      <p v-if="modell.meldung" class="meldung">{{ modell.meldung }}</p>
    </div>

    <UI.WpDialog
      :open="zeigeHinzufuegen"
      :title="t('server.add.title')"
      :description="t('server.add.desc')"
      :confirm-label="t('server.add')"
      @confirm="hinzufuegen"
      @cancel="zeigeHinzufuegen = false"
    >
      <div class="dialogfelder">
        <UI.WpInput v-model="neuerName" :placeholder="t('server.add.name')" />
        <UI.WpInput v-model="neueAdresse" :placeholder="t('server.add.address')" />
      </div>
    </UI.WpDialog>

    <UI.WpDialog
      :open="zeigeEntfernen !== null"
      :title="t('server.remove.title')"
      :description="t('server.remove.desc')"
      tone="warn"
      :confirm-label="t('server.remove.confirm')"
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
  padding: var(--wp-space-3) var(--wp-space-4);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.kopf__text h1 {
  margin: 0;
  font-size: var(--wp-font-size-lg);
  font-weight: var(--wp-font-weight-semibold);
}

.kopf__text p {
  margin: 2px 0 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
}

.kopf > :last-child {
  margin-left: auto;
}

.inhalt {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-4) var(--wp-space-4) var(--wp-space-5);
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
}

/* Eine Tabelle, keine Karten: gleiche Spalten, gleiche Zeilenhöhe. */
.tabelle {
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-panel);
}

.tabelle__kopf,
.zeile {
  display: grid;
  grid-template-columns: minmax(12rem, 1fr) minmax(14rem, 1fr) auto;
  align-items: center;
  gap: var(--wp-space-3);
  padding: var(--wp-space-2) var(--wp-space-3);
}

.tabelle__kopf {
  min-height: var(--wp-size-control-md);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
  font-size: var(--wp-font-size-xs);
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--wp-color-text-faint);
}

.zeile {
  min-height: var(--wp-size-control-lg);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.zeile:last-child {
  border-bottom: 0;
}

.zeile:hover {
  background: var(--wp-color-bg-hover);
}

.spalte--name {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 1px;
}

.name {
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-default);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hinweis {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.adresse {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.aktionen {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--wp-space-1);
}

.leer {
  margin: 0;
  padding: var(--wp-space-4) var(--wp-space-3);
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
}

.fussnote {
  margin: 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.meldung {
  margin: 0;
  color: var(--wp-color-status-error);
  font-size: var(--wp-font-size-sm);
}

.dialogfelder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-2);
}
</style>

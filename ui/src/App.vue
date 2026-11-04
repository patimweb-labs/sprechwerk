<script setup lang="ts">
// Fensterhülle mit Kopfzeile wie in einem Verwaltungsprogramm üblich:
// links Marke, daneben die Bereiche, rechts Verbindungszustand, eigenes
// Namenszeichen, Mikrofon/Ton und das Kontomenü (Einstellungen, Serverliste,
// Trennen).
//
// Keine Leiste links: die Breite gehört dem Inhalt — im Gespräch sind das
// Räume, Menschen und Verlauf.
import { computed } from "vue";
import * as UI from "@weppelmann/components";
import {
  IconBrand,
  IconPlug,
  IconHeadphoneOff,
  IconHeadphones,
  IconMic,
  IconMicOff,
  IconMore,
  IconUnplug,
} from "@weppelmann/icons";
import {
  ansicht,
  eigeneStumm,
  eigeneTaub,
  einstellungen,
  geheZu,
  modell,
  t,
  stummschalten,
  trennen,
  verbunden,
  meineRolle,
} from "./store";
import ServerView from "./views/ServerView.vue";
import RaeumeView from "./views/RaeumeView.vue";
import EinstellungenView from "./views/EinstellungenView.vue";

/** Eigene Kennung, falls verbunden. */
const eigeneId = computed(() => {
  const v = modell.verbindung;
  return v && v.art === "verbunden" ? v.ich : null;
});

const eigenerName = computed(() => {
  const v = modell.verbindung;
  return v && v.art === "verbunden" ? v.name : "";
});

/** Kontomenü — Nebenwege, der Hauptweg steht sichtbar in der Leiste. */
const kontomenue = computed<UI.MenuItem[]>(() => {
  const punkte: UI.MenuItem[] = [];
  if (verbunden.value) {
    punkte.push({ id: "raeume", label: t("app.rooms") });
  }
  punkte.push({ id: "server", label: t("app.menu_servers") });
  punkte.push({ id: "einstellungen", label: t("app.settings") });
  if (verbunden.value) {
    punkte.push({ id: "trennen", label: t("app.disconnect"), separatorBefore: true, danger: true });
  }
  return punkte;
});

function kontomenueGewaehlt(wahl: string) {
  if (wahl === "trennen") {
    void trennen();
    ansicht.value = "server";
    return;
  }
  geheZu(wahl as typeof ansicht.value);
}

const verbindungsText = computed(() => {
  const v = modell.verbindung;
  if (!v) return "…";
  if (v.art === "verbunden") return v.server_name;
  if (v.art === "verbindet") return t("app.connecting", v.server);
  if (v.art === "fehler") return v.text;
  return t("app.not_connected");
});

async function mikrofonUmschalten() {
  if (!eigeneId.value) return;
  await stummschalten(eigeneId.value, false, !eigeneStumm.value);
}

/** Server verlassen: trennen und zur Serverliste zurück. */
async function verlassenServer() {
  await trennen();
  ansicht.value = "server";
}

async function tonUmschalten() {
  if (!eigeneId.value) return;
  await stummschalten(eigeneId.value, true, !eigeneTaub.value);
}
</script>

<template>
  <div class="fenster">
    <header class="kopf">
      <div class="marke">
        <IconBrand class="marke__zeichen" />
        <div class="marke__text">
          <span class="marke__name">Sprechwerk</span>
          <span class="marke__server">
            <!-- Serververbindung: immer der Stecker. Eingesteckt = verbunden. -->
            <IconPlug class="marke__stecker" :class="{ 'ist-drin': verbunden }" />
            {{ verbindungsText }}
          </span>
        </div>
      </div>

      <div class="rechts">
        <template v-if="verbunden">
          <UI.WpIconButton
            :icon="eigeneStumm ? IconMicOff : IconMic"
            :label="eigeneStumm ? t('app.mic_unmute') : t('app.mic_mute')"
            :active="!eigeneStumm"
            size="sm"
            variant="ghost"
            @press="mikrofonUmschalten"
          />
          <UI.WpIconButton
            :icon="eigeneTaub ? IconHeadphoneOff : IconHeadphones"
            :label="eigeneTaub ? t('app.sound_unmute') : t('app.sound_mute')"
            :active="!eigeneTaub"
            size="sm"
            variant="ghost"
            @press="tonUmschalten"
          />
          <UI.WpAvatar :name="eigenerName" size="sm" tone="accent" :muted="eigeneStumm" />
          <span class="eigener">
            <span class="eigener__name">{{ eigenerName }}</span>
            <span class="eigener__rolle">{{ meineRolle }}</span>
          </span>
          <UI.WpIconButton
            :icon="IconUnplug"
            :label="t('app.leave_server')"
            size="sm"
            variant="ghost"
            @press="verlassenServer"
          />
        </template>

        <UI.WpMenu
          :items="kontomenue"
          :label="t('app.menu')"
          @select="kontomenueGewaehlt"
        >
          <UI.WpIconButton :icon="IconMore" :label="t('app.menu')" size="sm" variant="ghost" />
        </UI.WpMenu>
      </div>
    </header>

    <main class="bildschirm">
      <ServerView v-if="ansicht === 'server' || !verbunden" />
      <RaeumeView v-else-if="ansicht === 'raeume'" />
      <EinstellungenView v-else />
    </main>

    <UI.WpStatusBar
      brand="SPRECHWERK"
      :state="verbindungsText"
      :counts="
        verbunden
          ? t('app.counts', modell.mitglieder.length, modell.nachrichten.length)
          : t('app.saved_servers', einstellungen.favoriten.length)
      "
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

.kopf {
  display: flex;
  align-items: stretch;
  gap: var(--wp-space-3);
  height: var(--wp-size-control-lg);
  padding: 0 0 0 var(--wp-space-4);
  background: var(--wp-color-bg-panel);
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.marke {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding-right: var(--wp-space-2);
}

.marke__zeichen {
  width: 18px;
  height: 18px;
  color: var(--wp-color-accent-default);
}

.marke__text {
  display: flex;
  flex-direction: column;
  line-height: var(--wp-font-line-tight);
}

.marke__name {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-sm);
  letter-spacing: 0.06em;
}

.marke__server {
  display: flex;
  align-items: center;
  gap: var(--wp-space-1);
  line-height: var(--wp-font-line-tight);
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  max-width: 18rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Der Stecker: grau, wenn getrennt; Akzent, wenn die Verbindung steht. */
.marke__stecker {
  width: 12px;
  height: 12px;
  flex: none;
  color: var(--wp-color-text-faint);
}

.marke__stecker.ist-drin {
  color: var(--wp-color-status-ok);
}

.rechts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding-right: var(--wp-space-3);
}

.eigener {
  display: flex;
  flex-direction: column;
  line-height: var(--wp-font-line-tight);
}

.eigener__name {
  font-size: var(--wp-font-size-sm);
}

.eigener__rolle {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.bildschirm {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.meldung {
  color: var(--wp-color-status-error);
}
</style>

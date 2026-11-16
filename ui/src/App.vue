<script setup lang="ts">
// Fensterhülle mit Kopfzeile wie in einem Verwaltungsprogramm üblich:
// links Marke, daneben die Bereiche, rechts Verbindungszustand, eigenes
// Namenszeichen, Mikrofon/Ton und das Kontomenü (Einstellungen, Serverliste,
// Trennen).
//
// Keine Leiste links: die Breite gehört dem Inhalt — im Gespräch sind das
// Räume, Menschen und Verlauf.
import { computed, onMounted, onUnmounted } from "vue";
import * as UI from "@weppelmann/components";
import {
  IconPlug,
  IconHeadphoneOff,
  IconHeadphones,
  IconMic,
  IconMicOff,
  IconMore,
  IconUnplug,
} from "@weppelmann/icons";
import {
  beobachteVerschluesselung,
  holeZaehler,
  latenzText,
  messeLatenz,
  qualitaet,
  ansicht,
  eigeneTaub,
  eigenesHoeren,
  eigenesMikrofon,
  einstellungen,
  geheZu,
  lokalStumm,
  modell,
  pegel,
  sprechtaste,
  t,
  trennen,
  verbunden,
  meineRolle,
  rolle,
} from "./store";
import SprechwerkMarke from "./marke/SprechwerkMarke.vue";
import ServerView from "./views/ServerView.vue";
import RaeumeView from "./views/RaeumeView.vue";
import EinstellungenView from "./views/EinstellungenView.vue";
import ProtokollView from "./views/ProtokollView.vue";
import SperrenView from "./views/SperrenView.vue";

const eigenerName = computed(() => {
  const v = modell.verbindung;
  return v && v.art === "verbunden" ? v.name : "";
});

/** Kontomenü — Nebenwege, der Hauptweg steht sichtbar in der Leiste. */
const kontomenue = computed<UI.MenuItem[]>(() => {
  const punkte: UI.MenuItem[] = [];
  if (verbunden.value) {
    punkte.push({ id: "raeume", label: t("app.rooms") });
    // Das Prüfprotokoll ist eine Administrator- und Moderatorenansicht; für
    // alle anderen wäre der Menüpunkt eine Sackgasse.
    if (["administrator", "moderator"].includes(meineRolle.value ?? "")) {
      punkte.push({ id: "protokoll", label: t("app.audit") });
    }
    if (meineRolle.value === "administrator") {
      punkte.push({ id: "sperren", label: t("app.bans") });
    }
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
  await eigenesMikrofon(!lokalStumm.value);
}

/**
 * Push-to-Talk: Taste gedrückt halten.
 *
 * Erkannt werden die Taste aus den Einstellungen (Leertaste, Strg links, Alt
 * links) und die Maustasten 4/5. „Maus 4" ist `button === 3`, „Maus 5" ist
 * `button === 4` — die Zählung im Browser beginnt bei 0.
 */
const TASTEN: Record<string, (e: KeyboardEvent) => boolean> = {
  Leertaste: (e) => e.code === "Space",
  "Strg links": (e) => e.code === "ControlLeft",
  "Alt links": (e) => e.code === "AltLeft",
};

const MAUSTASTEN: Record<string, number> = { "Maus 4": 3, "Maus 5": 4 };

function tastePasst(e: KeyboardEvent): boolean {
  const pruefung = TASTEN[einstellungen.ptt_taste];
  return pruefung ? pruefung(e) : false;
}

function beimTastendruck(e: KeyboardEvent) {
  if (!tastePasst(e) || e.repeat) return;
  e.preventDefault();
  void sprechtaste(true);
}

function beimTastenloslassen(e: KeyboardEvent) {
  if (!tastePasst(e)) return;
  e.preventDefault();
  void sprechtaste(false);
}

function beimMausdruck(e: MouseEvent) {
  if (MAUSTASTEN[einstellungen.ptt_taste] !== e.button) return;
  e.preventDefault();
  void sprechtaste(true);
}

function beimMausloslassen(e: MouseEvent) {
  if (MAUSTASTEN[einstellungen.ptt_taste] !== e.button) return;
  void sprechtaste(false);
}

// Betriebszahlen alle 10 s: sie ändern sich langsam, und ein Takt von einer
// Sekunde wäre Funklärm für eine Zahl, die niemand so genau braucht.
let zaehlerTakt: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  beobachteVerschluesselung();
  void holeZaehler();
  void messeLatenz();
  zaehlerTakt = setInterval(() => {
    void holeZaehler();
    void messeLatenz();
  }, 10_000);
  window.addEventListener("keydown", beimTastendruck);
  window.addEventListener("keyup", beimTastenloslassen);
  window.addEventListener("mousedown", beimMausdruck);
  window.addEventListener("mouseup", beimMausloslassen);
});

onUnmounted(() => {
  if (zaehlerTakt !== null) clearInterval(zaehlerTakt);
  window.removeEventListener("keydown", beimTastendruck);
  window.removeEventListener("keyup", beimTastenloslassen);
  window.removeEventListener("mousedown", beimMausdruck);
  window.removeEventListener("mouseup", beimMausloslassen);
});

/** Server verlassen: trennen und zur Serverliste zurück. */
async function verlassenServer() {
  await trennen();
  ansicht.value = "server";
}

async function tonUmschalten() {
  await eigenesHoeren(!eigeneTaub.value);
}
</script>

<template>
  <div class="fenster">
    <header class="kopf">
      <div class="marke">
        <SprechwerkMarke class="marke__zeichen" />
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
          <!-- Eigener Pegel: zeigt, ob man über der Schwelle ist. Ohne
               Sprechpfad bleibt der Balken stehen — dann läuft kein Mikrofon. -->
          <div class="pegel" :title="t('app.level')">
            <UI.WpLevelMeter
              :level="pegel"
              :threshold="einstellungen.schwelle"
              :label="t('app.level')"
            />
          </div>
          <UI.WpIconButton
            :icon="lokalStumm ? IconMicOff : IconMic"
            :label="lokalStumm ? t('app.mic_unmute') : t('app.mic_mute')"
            :active="!lokalStumm"
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
          <UI.WpAvatar :name="eigenerName" size="sm" tone="accent" :muted="lokalStumm" />
          <span class="eigener">
            <span class="eigener__name">{{ eigenerName }}</span>
            <span class="eigener__rolle">{{ meineRolle ? rolle(meineRolle) : "" }}</span>
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
      <ProtokollView v-else-if="ansicht === 'protokoll'" />
      <SperrenView v-else-if="ansicht === 'sperren'" />
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
      <template #trail>
        <span v-if="latenzText" class="latenz" :title="t('app.latency')">{{ latenzText }}</span>
        <span v-if="qualitaet" class="guete" :class="qualitaet.tone">{{ qualitaet.text }}</span>
      </template>

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

/* Pegel in der Kopfzeile: schmal, damit er nicht mit dem Namen konkurriert. */
.latenz {
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.guete {
  white-space: nowrap;
}

.guete.ok {
  color: var(--wp-color-text-faint);
}

.guete.warn {
  color: var(--wp-color-status-warn);
  font-weight: var(--wp-font-weight-semibold);
}

.pegel {
  width: calc(var(--wp-space-8) + var(--wp-space-5));
  margin-right: var(--wp-space-1);
}

.marke {
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding-right: var(--wp-space-2);
}

.marke__zeichen {
  width: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
  height: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
  /* Weiß, wie die Zeichnung in der Marke selbst — nicht der Akzent. */
  color: var(--wp-color-text-default);
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
  width: var(--wp-font-size-sm);
  height: var(--wp-font-size-sm);
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
  /* Kein waagerechtes Scrollen: was zu breit ist, bricht um. */
  overflow-x: hidden;
}

.meldung {
  color: var(--wp-color-status-error);
}

/* ---------------------------------------------------------------------------
 * Schmale Fenster (bis „ICQ-Größe")
 *
 * Der Anspruch: bei 340 px Breite muss alles noch bedienbar sein. Was Platz
 * kostet und nicht nötig ist, verschwindet — in dieser Reihenfolge:
 * Serverzeile, Pegelbalken, Name und Rolle (das Zeichen bleibt), Zähler.
 * ------------------------------------------------------------------------- */
@media (max-width: 760px) {
  .kopf {
    gap: var(--wp-space-2);
    padding-left: var(--wp-space-3);
  }

  /* Der Pegel ist Beiwerk — er verschwindet zuerst. */
  .pegel {
    display: none;
  }

  .marke__server {
    max-width: 12rem;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
}

@media (max-width: 560px) {
  /* Name und Rolle stehen zusätzlich im Menü — hier zählt jede Spalte. */
  .eigener {
    display: none;
  }

  .marke__server {
    display: none;
  }
}

@media (max-width: 400px) {
  .kopf {
    padding-left: var(--wp-space-2);
  }

  .rechts {
    gap: var(--wp-space-1);
    padding-right: var(--wp-space-2);
  }
}
</style>

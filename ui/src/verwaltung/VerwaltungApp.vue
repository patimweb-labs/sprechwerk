<script setup lang="ts">
// Verwaltung des Servers im Browser — dieselben Bausteine, Tokens und Themes
// wie das Fenster, damit beides wie ein Programm aussieht.
//
// Drei Teile: der Server (Adresse und Fingerabdruck zum Weitergeben), die
// Mitglieder mit ihrer Rolle (hier wird man Administrator) und das Protokoll,
// das der Server gerade schreibt. Die Ansicht rechnet nichts: sie zeigt
// `ansicht`, was `logik.ts` und `store.ts` daraus machen.
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import * as UI from "@weppelmann/components";
import { IconUnplug } from "@weppelmann/icons";
import { rolle as rollenName, sprache, t } from "../i18n";
import { fingerabdruckGruppiert, kennungGruppiert, zuletztText } from "../format";
import SprechwerkMarke from "../marke/SprechwerkMarke.vue";
import {
  dauer,
  ROLLEN,
  rolleWaehlbar,
  stufe,
  zeitpunkt,
  type Mitgliedszeile,
  type Rolle,
} from "./logik";
import {
  abmelden,
  anmelden,
  ansicht,
  meldung,
  protokoll,
  rolleSetzen,
  starten,
  stoppen,
  thema,
  wendeThema,
} from "./store";

const code = ref("");

const stand = computed(() => (ansicht.value.art === "fertig" ? ansicht.value.stand : null));

const themen = computed(() => [
  { value: "dark", label: t("admin.theme.dark") },
  { value: "light", label: t("admin.theme.light") },
  { value: "mono", label: t("admin.theme.mono") },
]);

const fakten = computed(() => {
  const s = stand.value;
  if (!s) return [];
  return [
    { key: t("admin.server.name"), value: s.name },
    { key: t("admin.server.address"), value: t("admin.server.port_udp", s.port), mono: true },
    { key: t("admin.server.version"), value: s.version, mono: true },
    { key: t("admin.server.uptime"), value: dauer(s.jetzt - s.gestartet), mono: true },
    { key: t("admin.server.connected"), value: String(s.verbundene), mono: true },
    { key: t("admin.server.rooms"), value: String(s.raeume), mono: true },
    {
      key: t("admin.server.backup"),
      value: s.sicherung.aktiv
        ? s.sicherung.letzte
          ? zeitpunkt(s.sicherung.letzte, sprache.value)
          : t("admin.server.backup_none")
        : t("admin.server.backup_off"),
    },
  ];
});

const fingerabdruck = computed(() =>
  stand.value ? fingerabdruckGruppiert(stand.value.fingerabdruck) : "",
);

function rollenAuswahl(m: Mitgliedszeile) {
  const s = stand.value;
  return ROLLEN.map((r) => ({
    value: r,
    label: rollenName(r),
    disabled: s ? !rolleWaehlbar(s, m, r) : true,
  }));
}

function zuletzt(m: Mitgliedszeile): string {
  if (m.verbunden) return m.raum ? t("admin.members.in_room", m.raum) : t("admin.members.online");
  return zuletztText(m.zuletzt_gesehen, stand.value?.jetzt) ?? "—";
}

function rolleGewaehlt(m: Mitgliedszeile, wert: string | null) {
  if (!wert || wert === m.rolle) return;
  void rolleSetzen(m.id, m.name, wert as Rolle);
}

const meldungText = computed(() =>
  meldung.value ? t(meldung.value.schluessel, meldung.value.name ?? "") : "",
);

const anmeldeFehler = computed(() =>
  ansicht.value.art === "anmelden" && ansicht.value.fehler ? t(ansicht.value.fehler) : undefined,
);

function absenden() {
  if (code.value.trim()) void anmelden(code.value);
}

// Protokoll: unten bleiben, solange man unten ist. Wer hochscrollt, um etwas zu
// lesen, wird nicht weggerissen.
const fenster = ref<HTMLElement | null>(null);
let amEnde = true;
function gescrollt() {
  const f = fenster.value;
  if (f) amEnde = f.scrollHeight - f.scrollTop - f.clientHeight < f.clientHeight / 4;
}
watch(
  () => protokoll.zeilen.length,
  async () => {
    if (!amEnde) return;
    await nextTick();
    const f = fenster.value;
    if (f) f.scrollTop = f.scrollHeight;
  },
);

const statuszeile = computed(() =>
  stand.value
    ? t("admin.status.counts", stand.value.verbundene, stand.value.mitglieder.length)
    : "",
);

onMounted(starten);
onUnmounted(stoppen);
</script>

<template>
  <div class="fenster">
    <header class="kopf">
      <div class="marke">
        <SprechwerkMarke class="marke__zeichen" />
        <div class="marke__text">
          <span class="marke__name">Sprechwerk</span>
          <span class="marke__unter">{{
            stand ? t("admin.title_server", stand.name) : t("admin.title")
          }}</span>
        </div>
      </div>
      <div class="rechts">
        <UI.WpToggleGroup
          class="still"
          :model-value="thema"
          :options="themen"
          :label="t('admin.theme')"
          size="sm"
          @update:model-value="wendeThema"
        />
        <UI.WpIconButton
          v-if="stand"
          :icon="IconUnplug"
          :label="t('admin.logout')"
          size="sm"
          variant="ghost"
          @press="abmelden"
        />
      </div>
    </header>

    <main class="bildschirm">
      <div v-if="ansicht.art === 'laedt'" class="mitte">
        <UI.WpSpinner :label="t('admin.loading')" />
      </div>

      <div v-else-if="ansicht.art === 'fehler'" class="mitte">
        <UI.WpEmptyState
          kind="fehler"
          :title="t('admin.error.server')"
          :description="t('admin.error.server_desc')"
        />
      </div>

      <div v-else-if="ansicht.art === 'anmelden'" class="mitte">
        <UI.WpCard class="anmeldung" :title="t('admin.login.title')" raised>
          <form class="anmeldung__form" @submit.prevent="absenden">
            <p class="unter">{{ t("admin.login.desc") }}</p>
            <UI.WpFormField :label="t('admin.login.code')" :error="anmeldeFehler">
              <UI.WpInput v-model="code" type="password" :state="anmeldeFehler ? 'invalid' : 'default'" />
            </UI.WpFormField>
            <UI.WpButton
              :label="t('admin.login.submit')"
              variant="primary"
              :state="ansicht.sendet ? 'busy' : code.trim() ? 'default' : 'disabled'"
              @press="absenden"
            />
          </form>
        </UI.WpCard>
      </div>

      <div v-else class="raster">
        <div class="spalte">
          <UI.WpToast
            v-if="ansicht.stand.erster_wird_administrator"
            tone="warn"
            :title="t('admin.first.title')"
            :description="t('admin.first.desc')"
            :dismissible="false"
          />

          <UI.WpCard :title="t('admin.server.title')">
            <UI.WpFactList :facts="fakten" label-width="10rem" />
            <div class="fingerabdruck">
              <UI.WpSectionLabel :label="t('admin.server.fingerprint')" />
              <code class="fingerabdruck__wert">{{ fingerabdruck }}</code>
              <p class="unter">{{ t("admin.server.fingerprint_hint") }}</p>
            </div>
          </UI.WpCard>

          <UI.WpCard :title="t('admin.members.title')" :subtitle="t('admin.members.hint')">
            <UI.WpToast
              v-if="meldung"
              :tone="meldung.ton"
              :title="meldungText"
              :close-label="t('admin.close')"
              @close="meldung = null"
            />
            <UI.WpEmptyState
              v-if="!ansicht.stand.mitglieder.length"
              :title="t('admin.members.empty')"
              :description="t('admin.members.empty_desc')"
            />
            <ul v-else class="mitglieder">
              <li v-for="m in ansicht.stand.mitglieder" :key="m.id" class="mitglied">
                <UI.WpAvatar :name="m.name" size="sm" :away="!m.verbunden" />
                <div class="mitglied__text">
                  <span class="mitglied__name">{{ m.name }}</span>
                  <span class="mitglied__unter">
                    <code>{{ kennungGruppiert(m.kennung) }}</code> · {{ zuletzt(m) }}
                  </span>
                </div>
                <UI.WpBadge v-if="m.verbunden" :label="t('admin.members.online')" tone="ok" />
                <div class="mitglied__rolle still">
                  <UI.WpSelect
                    :model-value="m.rolle"
                    :options="rollenAuswahl(m)"
                    :label="t('admin.members.role_for', m.name)"
                    @update:model-value="(w) => rolleGewaehlt(m, w)"
                  />
                </div>
              </li>
            </ul>
          </UI.WpCard>
        </div>

        <UI.WpCard class="protokoll" :title="t('admin.log.title')" :subtitle="t('admin.log.hint')" flush>
          <div
            ref="fenster"
            class="protokoll__zeilen"
            role="log"
            aria-live="polite"
            :aria-label="t('admin.log.title')"
            tabindex="0"
            @scroll="gescrollt"
          >
            <div
              v-for="z in protokoll.zeilen"
              :key="z.nr"
              class="zeile"
              :class="`zeile--${stufe(z.text)}`"
            >{{ z.text }}</div>
            <p v-if="!protokoll.zeilen.length" class="unter">{{ t("admin.log.empty") }}</p>
          </div>
        </UI.WpCard>
      </div>
    </main>

    <UI.WpStatusBar brand="SPRECHWERK" :state="t('admin.title')" :counts="statuszeile" />
  </div>
</template>

<style scoped>
.fenster {
  display: flex;
  flex-direction: column;
  height: 100%;
}

/* Kopfzeile wie im Fenster (`App.vue`): dieselbe Höhe, Fläche und Marke. */
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
}

.marke__zeichen {
  width: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
  height: calc(var(--wp-space-4) + var(--wp-space-1) / 2);
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

.marke__unter {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.rechts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--wp-space-2);
  padding-right: var(--wp-space-3);
}

.bildschirm {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.mitte {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100%;
  padding: var(--wp-space-5);
}

.anmeldung {
  width: min(100%, 28rem);
}

.anmeldung__form {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
}

/* Beschriftung nur für Vorleseprogramme: der Name der Auswahl steht schon in
   der Zeile daneben, die Darstellung erklärt sich in der Kopfzeile selbst. Die
   Bibliothek verknüpft die Beschriftung über `aria-labelledby` — ausgeblendet
   wird sie deshalb so, dass sie im Zugänglichkeitsbaum bleibt. */
.still :deep(.wp-select__label),
.still :deep(.wp-togglegroup__label) {
  position: absolute;
  width: 0.0625rem;
  height: 0.0625rem;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.unter {
  margin: 0;
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
}

/* Links Server und Mitglieder, rechts das Protokoll; schmal untereinander. */
.raster {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
  gap: var(--wp-space-4);
  padding: var(--wp-space-4);
  min-height: 100%;
  /* Jede Karte so hoch wie ihr Inhalt — sonst streckt die längere Spalte die
     andere, und unter dem Protokoll bliebe eine leere Fläche. */
  align-items: start;
}

@media (max-width: 960px) {
  .raster {
    grid-template-columns: minmax(0, 1fr);
  }
}

.spalte {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-4);
  min-width: 0;
}

.fingerabdruck {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
  margin-top: var(--wp-space-3);
}

.fingerabdruck__wert {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-sm);
  overflow-wrap: anywhere;
  user-select: all;
}

.mitglieder {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
}

.mitglied {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  padding: var(--wp-space-2) 0;
  border-bottom: var(--wp-size-border) solid var(--wp-color-border-subtle);
}

.mitglied:last-child {
  border-bottom: none;
}

.mitglied__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: var(--wp-font-line-tight);
}

.mitglied__name {
  font-size: var(--wp-font-size-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mitglied__unter {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mitglied__unter code {
  font-family: var(--wp-font-family-mono);
}

.mitglied__rolle {
  flex: none;
  width: 11rem;
}

.protokoll {
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.protokoll__zeilen {
  height: calc(100vh - 3 * var(--wp-size-control-lg) - 2 * var(--wp-space-4) - var(--wp-space-8));
  min-height: 20rem;
  overflow: auto;
  padding: var(--wp-space-3);
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-xs);
  line-height: var(--wp-font-line-normal);
  background: var(--wp-color-bg-canvas);
}

.protokoll__zeilen:focus-visible {
  outline: var(--wp-size-focus) solid var(--wp-color-focus-ring);
  outline-offset: calc(var(--wp-size-focus) * -1);
}

.zeile {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.zeile--leise {
  color: var(--wp-color-text-faint);
}

.zeile--warnung {
  color: var(--wp-color-status-warn);
}

.zeile--fehler {
  color: var(--wp-color-status-error);
}
</style>

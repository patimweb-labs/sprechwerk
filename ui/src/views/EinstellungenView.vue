<script setup lang="ts">
// Einstellungen: alles untereinander, volle Breite. Nur, was dieses Fenster
// betrifft — Server und Verbinden stehen in der Serverliste (eine Wahrheit).
//
// Zu den Geräten: Liste und Test laufen über `crates/desktop/src/audio.rs`
// (PipeWire/PulseAudio-Werkzeuge). Der eigentliche Sprachpfad (Opus) kommt
// später; hier wird nur ausgewählt und geprüft.
import { computed, onMounted, ref } from "vue";
import * as UI from "@weppelmann/components";
import { IconMic, IconRefresh, IconSpeaker } from "@weppelmann/icons";
import { api, type Geraet } from "../api";
import { SPRACHEN } from "../i18n";
import { kennungGruppiert } from "../format";
import { tasteName } from "../sprechtaste";
import {
  einstellungen,
  modell,
  rolle as rollenName,
  sprache,
  speichere,
  t,
  waehleSprache,
  wendeThema,
} from "../store";

const themen = [
  { value: "dark", label: "Dunkel" },
  { value: "light", label: "Hell" },
  { value: "mono", label: "Mono" },
];

const sprachen = SPRACHEN;

const mikrofonGeraete = ref<Geraet[]>([]);
const lautsprecherGeraete = ref<Geraet[]>([]);
const geraeteFehler = ref<string | null>(null);
const pegel = ref(0);
const messend = ref(false);
const tonGesendet = ref(false);
const tasteAufnahme = ref(false);
const tastenFehler = ref<string | null>(null);

/**
 * Auswahlliste bauen: „Vorgabe des Systems" zuerst, dann die Geräte.
 *
 * Ist das **gespeicherte** Gerät nicht (mehr) in der Liste — abgezogen,
 * umbenannt, anderer Rechner —, wird es trotzdem als Eintrag gezeigt. Sonst
 * stünde dort „Auswählen …", und niemand wüsste, welches Gerät gemeint ist.
 */
function auswahl(geraete: Geraet[], gespeichert: string) {
  const liste = [
    { value: "", label: t("set.default_device") },
    ...geraete.map((g) => ({
      value: g.id,
      label: g.vorgabe ? t("set.default_device_marked", g.name) : g.name,
    })),
  ];
  if (gespeichert && !liste.some((e) => e.value === gespeichert)) {
    liste.push({ value: gespeichert, label: t("set.device_missing", gespeichert) });
  }
  return liste;
}

const mikrofonWahl = computed(() => auswahl(mikrofonGeraete.value, einstellungen.mikrofon));
const lautsprecherWahl = computed(() =>
  auswahl(lautsprecherGeraete.value, einstellungen.lautsprecher),
);

onMounted(async () => {
  try {
    const liste = await api.audioGeraete();
    mikrofonGeraete.value = liste.mikrofon;
    lautsprecherGeraete.value = liste.lautsprecher;
  } catch (fehler) {
    geraeteFehler.value = String(fehler);
  }
});

/** Sprechtest: Pegel für 1,5 s messen. */
/**
 * Sprechtest: den Pegel ~1,2 s lang mitlesen.
 *
 * Bewusst **viele kleine Abfragen** statt einer langen: der Balken soll leben,
 * und keine einzelne Abfrage darf die Oberfläche anhalten. Läuft der
 * Sprechpfad, kostet eine Abfrage nichts (der Wert liegt schon vor).
 */
async function sprechtest() {
  if (messend.value) return;
  messend.value = true;
  geraeteFehler.value = null;
  const ende = Date.now() + 1200;
  try {
    while (Date.now() < ende) {
      pegel.value = await api.audioPegel(einstellungen.mikrofon, 100);
      await new Promise((f) => setTimeout(f, 100));
    }
  } catch (fehler) {
    geraeteFehler.value = String(fehler);
  } finally {
    messend.value = false;
  }
}

/** Hörtest: kurzer Ton auf dem gewählten Gerät. */
async function hoertest() {
  tonGesendet.value = false;
  try {
    await api.audioTestton(einstellungen.lautsprecher);
    tonGesendet.value = true;
    setTimeout(() => (tonGesendet.value = false), 2500);
  } catch (fehler) {
    geraeteFehler.value = String(fehler);
  }
}

/**
 * Taste für Push-to-Talk aufnehmen.
 *
 * Empfehlung im Hinweis darunter: eine **Maustaste an der Seite** oder eine
 * Modifikatortaste. Die Leertaste ist belegt — beim Tippen im Chat würde man
 * dauernd senden, deshalb wird davor gewarnt.
 */
function tasteAufnehmen() {
  tasteAufnahme.value = true;
  tastenFehler.value = null;
}

function tasteBelegt(text: string) {
  einstellungen.ptt_taste = text;
  tasteAufnahme.value = false;
}

function beiTaste(ereignis: KeyboardEvent) {
  if (!tasteAufnahme.value) return;
  ereignis.preventDefault();
  if (ereignis.code === "Escape") {
    tasteAufnahme.value = false;
    return;
  }
  tasteBelegt(tasteName(ereignis.code));
}

function beiMaus(ereignis: MouseEvent) {
  if (!tasteAufnahme.value) return;
  // Seitentasten (3 = Maus 4, 4 = Maus 5) sind die beste Wahl: sie stören
  // weder beim Tippen noch bei Fensterkürzeln.
  if (ereignis.button === 3) tasteBelegt("Maus 4");
  else if (ereignis.button === 4) tasteBelegt("Maus 5");
}

const leertasteGewaehlt = computed(() => einstellungen.ptt_taste === "Leertaste");

const fakten = computed(() => {
  const v = modell.verbindung;
  if (v?.art !== "verbunden") return [];
  return [
    { key: t("set.fact.server"), value: v.server, mono: true },
    { key: t("set.fact.name"), value: v.server_name },
    { key: t("set.fact.me"), value: v.name },
    { key: t("set.fact.role"), value: rollenName(v.rolle) },
    // Die **Kennung** ist der öffentliche Schlüssel, nicht die Mitgliedsnummer:
    // nur damit ist der Umzug auf ein anderes Gerät nachvollziehbar.
    { key: t("set.fact.key"), value: kennungGruppiert(v.kennung ?? ""), mono: true },
  ];
});

async function thema(wert: string) {
  wendeThema(wert);
  await speichere();
}
</script>

<template>
  <div class="ansicht" tabindex="0" @keydown="beiTaste" @mousedown="beiMaus">
    <header class="kopf">
      <div class="kopf__text">
        <h1>{{ t("set.title") }}</h1>
        <p>{{ t("set.sub") }}</p>
      </div>
    </header>

    <div class="inhalt">
      <UI.WpCard :title="t('set.me')" :subtitle="t('set.me.sub')">
        <div class="felder">
          <UI.WpFormField :label="t('set.name')" :hint="t('set.name.hint')">
            <template #default="{ id }">
              <UI.WpInput :id="id" v-model="einstellungen.name" :placeholder="t('set.name.placeholder')" />
            </template>
          </UI.WpFormField>
          <UI.WpFactList v-if="fakten.length" :facts="fakten" />
          <p v-if="fakten.length" class="kennung-hinweis">{{ t("set.key_hint") }}</p>
        </div>
      </UI.WpCard>

      <UI.WpCard :title="t('set.mic')" :subtitle="t('set.mic.sub')">
        <div class="felder">
          <UI.WpFormField :label="t('set.device')">
            <template #default="{ id }">
              <UI.WpSelect :id="id" v-model="einstellungen.mikrofon" :options="mikrofonWahl" />
            </template>
          </UI.WpFormField>

          <div class="test">
            <UI.WpButton
              :label="t('set.mic_test')"
              :icon="IconMic"
              :state="messend ? 'busy' : 'default'"
              size="sm"
              variant="secondary"
              @press="sprechtest"
            />
            <UI.WpLevelMeter
              :level="pegel"
              :threshold="einstellungen.schwelle"
              :label="t('set.level')"
              class="meter"
            />
            <span class="wert">{{ pegel }}</span>
          </div>
          <p class="hinweis">{{ t("set.mic.hint") }}</p>
        </div>
      </UI.WpCard>

      <UI.WpCard :title="t('set.speaker')" :subtitle="t('set.speaker.sub')">
        <div class="felder">
          <UI.WpFormField :label="t('set.device')">
            <template #default="{ id }">
              <UI.WpSelect :id="id" v-model="einstellungen.lautsprecher" :options="lautsprecherWahl" />
            </template>
          </UI.WpFormField>
          <div class="test">
            <UI.WpButton
              :label="t('set.speaker_test')"
              :icon="IconSpeaker"
              size="sm"
              variant="secondary"
              @press="hoertest"
            />
            <span v-if="tonGesendet" class="ok">{{ t("set.speaker.done") }}</span>
          </div>
          <p v-if="geraeteFehler" class="fehler">{{ geraeteFehler }}</p>
        </div>
      </UI.WpCard>

      <UI.WpCard :title="t('set.talk')" :subtitle="t('set.talk.sub')">
        <div class="felder">
          <UI.WpSwitch v-model="einstellungen.push_to_talk" :label="t('set.ptt')" />

          <UI.WpFormField
            :label="t('set.key')"
            :hint="t('set.key.hint')"
          >
            <div class="taste">
              <span class="taste__wert">{{ einstellungen.ptt_taste || "—" }}</span>
              <UI.WpButton
                :label="tasteAufnahme ? t('set.key.press') : t('set.key.change')"
                size="sm"
                variant="secondary"
                @press="tasteAufnehmen"
              />
            </div>
          </UI.WpFormField>
          <p v-if="leertasteGewaehlt" class="warnung">{{ t("set.key.space_warn") }}</p>

          <UI.WpSwitch v-model="einstellungen.rauschunterdrueckung" :label="t('set.denoise')" />
          <p class="hinweis">{{ t("set.denoise.hint") }}</p>
          <UI.WpSwitch v-model="einstellungen.noisegate" :label="t('set.noisegate')" />
          <p class="hinweis">{{ t("set.noisegate.hint") }}</p>
          <UI.WpSwitch v-model="einstellungen.echounterdrueckung" :label="t('set.aec')" />
          <p class="hinweis">{{ t("set.aec.hint") }}</p>

          <UI.WpSlider
            v-model="einstellungen.schwelle"
            :label="t('set.threshold')"
            :min="0"
            :max="100"
            suffix="%"
          />
          <UI.WpSlider
            v-model="einstellungen.vorlauf_ms"
            :label="t('set.lead')"
            :min="0"
            :max="200"
            :step="10"
            suffix="ms"
          />
          <p class="hinweis">{{ t("set.lead.hint") }}</p>
        </div>
      </UI.WpCard>

      <UI.WpCard :title="t('set.chat')">
        <div class="felder">
          <UI.WpSwitch v-model="einstellungen.link_vorschau" :label="t('set.link_preview')" />
          <p class="hinweis">{{ t("set.link_preview.hint") }}</p>
        </div>
      </UI.WpCard>

      <UI.WpCard :title="t('set.appearance')">
        <div class="felder">
          <UI.WpToggleGroup
            :model-value="einstellungen.thema"
            :options="themen"
            :label="t('set.scheme')"
            @update:model-value="thema"
          />
          <UI.WpFormField :label="t('set.language')">
            <template #default="{ id }">
              <UI.WpSelect
                :id="id"
                :model-value="sprache"
                :options="sprachen"
                @update:model-value="(wert: string | null) => wert && waehleSprache(wert)"
              />
            </template>
          </UI.WpFormField>
        </div>
      </UI.WpCard>

      <div class="aktionen">
        <UI.WpButton :label="t('set.save')" variant="primary" :icon="IconRefresh" @press="speichere" />
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
  outline: none;
}

/* Kopfzeile wie in „Server" und „Räume": gleiche Abstände, gleiche Schrift. */
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
  margin: calc(var(--wp-space-1) / 2) 0 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
}

.inhalt {
  flex: 1;
  overflow-y: auto;
  padding: var(--wp-space-3) var(--wp-space-4) var(--wp-space-5);
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
}

/* Alles untereinander, aber über die volle Breite — Karten dehnen sich. */
.inhalt > * {
  width: 100%;
}

.felder {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-3);
}

.test {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
}

.meter {
  flex: 1;
  min-width: 8rem;
}

.wert {
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-muted);
  min-width: 2.5rem;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.taste {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
}

.taste__wert {
  flex: 1;
  padding: var(--wp-space-2);
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  background: var(--wp-color-bg-input);
  font-family: var(--wp-font-family-mono);
  font-size: var(--wp-font-size-sm);
}

.hinweis {
  margin: 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
}

.warnung {
  margin: 0;
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-status-warn);
}

.ok {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-status-ok);
}

.fehler {
  margin: 0;
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-status-error);
}

.aktionen {
  display: flex;
  align-items: center;
  gap: var(--wp-space-3);
  flex-wrap: wrap;
  padding-bottom: var(--wp-space-4);
}

.kennung-hinweis {
  margin: var(--wp-space-2) 0 0;
  color: var(--wp-color-text-muted);
  font-size: var(--wp-font-size-sm, 0.875rem);
  max-width: 60ch;
}
</style>
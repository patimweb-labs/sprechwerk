<script setup lang="ts">
// Ein Beitrag im Verlauf: Kopf, Inhalt (Text mit Links, Vorschau), Reaktionen.
//
// Alles, was hier steht, kommt aus dem Modell: was entschlüsselt ist, welche
// Reaktionen zählen, ob ich dabei bin — gerechnet im Rust-Teil. Die Ansicht
// zerlegt nur Text in Stücke (reine Funktion `teile`) und zeichnet.
import { computed } from "vue";
import * as UI from "@weppelmann/components";
import { IconMore } from "@weppelmann/icons";
import type { Nachricht } from "../api";
import { REAKTIONEN, teile } from "../chat";
import { effektZeigen, linkOeffnen, loesche, modell, reagiere, t, uhrzeit } from "../store";

const props = defineProps<{ n: Nachricht; eigene: boolean }>();

const stuecke = computed(() => (props.n.inhalt.zustand === "text" ? teile(props.n.inhalt.text) : []));
const vorschau = computed(() => (props.n.inhalt.zustand === "text" ? (props.n.inhalt.vorschau ?? null) : null));
const effekt = computed(() => (props.n.inhalt.zustand === "text" ? (props.n.inhalt.effekt ?? null) : null));
/** Bebt gerade? Der Schlüssel `nr` startet die Bewegung bei jedem Auftritt neu. */
const beben = computed(() =>
  modell.effekt && modell.effekt.effekt === "beben" && modell.effekt.nachricht === props.n.id ? modell.effekt.nr : 0,
);
const lesbar = computed(() => props.n.inhalt.zustand === "text");

function menue(): UI.MenuItem[] {
  const punkte: UI.MenuItem[] = REAKTIONEN.map((z) => ({ id: `r:${z}`, label: `${z}  ${t(`react.${z}`)}` }));
  if (effekt.value) {
    punkte.push({ id: "effekt", label: t("effect.replay", t(`effect.${effekt.value}`)), separatorBefore: true });
  }
  if (props.n.loeschbar) punkte.push({ id: "loeschen", label: t("chat.delete"), separatorBefore: true, danger: true });
  return punkte;
}

function gewaehlt(wahl: string) {
  if (wahl.startsWith("r:")) void reagiere(props.n.id, wahl.slice(2));
  else if (wahl === "effekt") void effektZeigen(props.n.id);
  else if (wahl === "loeschen") void loesche(props.n.id);
}
</script>

<template>
  <article class="beitrag" :class="{ 'ist-eigene': eigene }">
    <div :key="beben" class="beitrag__text" :class="{ bebt: beben > 0 }">
      <header class="beitrag__kopf">
        <span class="beitrag__wer" :class="{ 'ist-eigene': eigene }">{{ n.verfasser_name }}</span>
        <span class="beitrag__wann">{{ uhrzeit(n.zeit) }}</span>
        <span v-if="effekt" class="beitrag__effekt">{{ t(`effect.${effekt}`) }}</span>
        <UI.WpMenu
          v-if="lesbar"
          class="beitrag__menue"
          :items="menue()"
          :label="t('chat.actions')"
          align="right"
          @select="gewaehlt"
        >
          <UI.WpIconButton :icon="IconMore" :label="t('chat.actions')" size="sm" variant="ghost" />
        </UI.WpMenu>
        <button v-else-if="n.loeschbar" class="beitrag__aktion" type="button" @click="loesche(n.id)">
          {{ t("chat.delete") }}
        </button>
      </header>

      <p v-if="n.inhalt.zustand === 'text'" class="beitrag__inhalt">
        <template v-for="(s, i) in stuecke" :key="i">
          <a
            v-if="s.art === 'link'"
            class="beitrag__link"
            :href="s.url"
            rel="noopener noreferrer"
            @click.prevent="linkOeffnen(s.url)"
          >{{ s.text }}</a>
          <template v-else>{{ s.text }}</template>
        </template>
      </p>
      <p v-else-if="n.inhalt.zustand === 'geloescht'" class="beitrag__weg">{{ t("chat.deleted") }}</p>
      <p v-else-if="n.inhalt.zustand === 'wartet'" class="beitrag__weg">{{ t("chat.waiting") }}</p>
      <p v-else-if="n.inhalt.zustand === 'neuer'" class="beitrag__weg">{{ t("chat.newer", n.inhalt.art) }}</p>
      <p v-else class="beitrag__weg beitrag__weg--fehler">{{ t("chat.unreadable") }}</p>

      <!-- Vorschau: vom Absender abgerufen, hier nur gezeigt. Ein Klick öffnet
           den Link im Browser — nicht im Fenster. -->
      <a
        v-if="vorschau"
        class="vorschau"
        :href="vorschau.url"
        rel="noopener noreferrer"
        @click.prevent="linkOeffnen(vorschau.url)"
      >
        <img v-if="vorschau.bild" class="vorschau__bild" :src="`data:image/jpeg;base64,${vorschau.bild}`" alt="" />
        <span class="vorschau__text">
          <span class="vorschau__titel">{{ vorschau.titel }}</span>
          <span v-if="vorschau.beschreibung" class="vorschau__beschreibung">{{ vorschau.beschreibung }}</span>
          <span class="vorschau__adresse">{{ vorschau.url }}</span>
        </span>
      </a>

      <div v-if="n.reaktionen.length" class="reaktionen">
        <button
          v-for="r in n.reaktionen"
          :key="r.zeichen"
          class="reaktion"
          :class="{ 'ist-meine': r.ich }"
          type="button"
          :aria-pressed="r.ich"
          :title="r.namen.join(', ')"
          :aria-label="t('react.count', r.zeichen, r.anzahl, r.namen.join(', '))"
          @click="reagiere(n.id, r.zeichen)"
        >
          <span aria-hidden="true">{{ r.zeichen }}</span>
          <span class="reaktion__zahl" aria-hidden="true">{{ r.anzahl }}</span>
        </button>
      </div>
    </div>
  </article>
</template>

<style scoped>
.beitrag {
  padding: var(--wp-space-1) 0 var(--wp-space-3);
  border-left: calc(var(--wp-size-border) * 2) solid transparent;
  padding-left: var(--wp-space-2);
}

/* Eigene Beiträge: Akzentkante links statt Farbe im Text. */
.beitrag.ist-eigene {
  border-left-color: var(--wp-color-accent-default);
}

.beitrag__wer.ist-eigene {
  color: var(--wp-color-accent-default);
}

.beitrag__text {
  min-width: 0;
  flex: 1;
}

.beitrag__kopf {
  display: flex;
  align-items: baseline;
  gap: var(--wp-space-2);
}

.beitrag__wer {
  font-size: var(--wp-font-size-sm);
  font-weight: var(--wp-font-weight-semibold);
}

.beitrag__wann,
.beitrag__effekt {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  font-variant-numeric: tabular-nums;
}

.beitrag__menue,
.beitrag__aktion {
  margin-left: auto;
  opacity: 0;
  transition: opacity var(--wp-duration-fast) ease;
}

.beitrag__aktion {
  border: 0;
  background: transparent;
  color: var(--wp-color-text-faint);
  font: inherit;
  font-size: var(--wp-font-size-xs);
  padding: 0;
}

.beitrag:hover .beitrag__menue,
.beitrag:focus-within .beitrag__menue,
.beitrag:hover .beitrag__aktion,
.beitrag:focus-within .beitrag__aktion {
  opacity: 1;
}

.beitrag__aktion:hover {
  color: var(--wp-color-status-error);
}

.beitrag__inhalt {
  margin: var(--wp-space-1) 0 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: var(--wp-font-line-normal);
}

.beitrag__link {
  color: var(--wp-color-accent-default);
  text-decoration: underline;
  text-underline-offset: 0.15em;
}

.beitrag__link:hover {
  color: var(--wp-color-accent-hover);
}

.beitrag__link:focus-visible,
.vorschau:focus-visible,
.reaktion:focus-visible {
  outline: var(--wp-size-focus) solid var(--wp-color-focus-ring);
  outline-offset: var(--wp-size-border);
}

.beitrag__weg {
  margin: var(--wp-space-1) 0 0;
  color: var(--wp-color-text-faint);
  font-style: italic;
}

.beitrag__weg--fehler {
  color: var(--wp-color-status-warn);
}

/* ---- Vorschau ---- */
.vorschau {
  display: flex;
  gap: var(--wp-space-3);
  margin-top: var(--wp-space-2);
  max-width: 36rem;
  padding: var(--wp-space-2);
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  border-left: calc(var(--wp-size-border) * 2) solid var(--wp-color-border-strong);
  background: var(--wp-color-bg-raised);
  color: inherit;
  text-decoration: none;
}

.vorschau:hover {
  background: var(--wp-color-bg-hover);
}

.vorschau__bild {
  flex: none;
  width: 6rem;
  height: 6rem;
  object-fit: cover;
}

.vorschau__text {
  display: flex;
  flex-direction: column;
  gap: var(--wp-space-1);
  min-width: 0;
}

.vorschau__titel {
  font-weight: var(--wp-font-weight-semibold);
  line-height: var(--wp-font-line-tight);
}

.vorschau__beschreibung {
  font-size: var(--wp-font-size-sm);
  color: var(--wp-color-text-muted);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.vorschau__adresse {
  font-size: var(--wp-font-size-xs);
  color: var(--wp-color-text-faint);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---- Reaktionen ---- */
.reaktionen {
  display: flex;
  flex-wrap: wrap;
  gap: var(--wp-space-1);
  margin-top: var(--wp-space-2);
}

.reaktion {
  display: inline-flex;
  align-items: center;
  gap: var(--wp-space-1);
  padding: 0 var(--wp-space-2);
  min-height: var(--wp-size-control-sm);
  border: var(--wp-size-border) solid var(--wp-color-border-subtle);
  border-radius: var(--wp-radius-pill);
  background: var(--wp-color-bg-raised);
  color: var(--wp-color-text-default);
  font: inherit;
  font-size: var(--wp-font-size-sm);
  cursor: pointer;
}

.reaktion:hover {
  background: var(--wp-color-bg-hover);
}

/* Meine Reaktion: Akzent als Zustand („ich bin dabei"), nicht als Schmuck. */
.reaktion.ist-meine {
  border-color: var(--wp-color-accent-default);
  background: var(--wp-color-bg-selected);
}

.reaktion__zahl {
  font-variant-numeric: tabular-nums;
  color: var(--wp-color-text-muted);
}

/* ---- Beben ---- */
.bebt {
  animation: beben calc(var(--wp-duration-normal) * 4) ease-in-out;
}

@keyframes beben {
  0%,
  100% {
    transform: translateX(0);
  }
  15%,
  45%,
  75% {
    transform: translateX(-1.5%);
  }
  30%,
  60%,
  90% {
    transform: translateX(1.5%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .bebt {
    animation: none;
  }
}
</style>

<script setup lang="ts">
// Die Bühne für Konfetti und Herzen: liegt über dem Verlauf, fängt keine
// Klicks und spielt, sobald im Modell ein neuer Auftritt steht (`nr`). Danach
// bleibt sie leer stehen — kein Zeitgeber, kein eigener Zustand.
//
// Wer weniger Bewegung eingestellt hat (`prefers-reduced-motion`), sieht
// nichts; der Effekt steht dann nur als Wort am Beitrag.
import { computed } from "vue";
import { modell } from "../store";
import { teilchen } from "../chat";

const auftritt = computed(() => {
  const e = modell.effekt;
  return e && (e.effekt === "konfetti" || e.effekt === "herzen") ? e : null;
});
const stuecke = computed(() => (auftritt.value ? teilchen(auftritt.value.nr, auftritt.value.effekt === "herzen" ? 24 : 70) : []));
</script>

<template>
  <div v-if="auftritt" :key="auftritt.nr" class="buehne" :class="`buehne--${auftritt.effekt}`" aria-hidden="true">
    <span
      v-for="(s, i) in stuecke"
      :key="i"
      class="teil"
      :class="`farbe-${s.farbe}`"
      :style="{
        '--x': `${s.x}%`,
        '--verzug': `${s.verzug}s`,
        '--dauer': `${s.dauer}s`,
        '--drehung': `${s.drehung}deg`,
        '--drift': `${s.drift}vw`,
      }"
    >{{ auftritt.effekt === "herzen" ? "❤" : "" }}</span>
  </div>
</template>

<style scoped>
.buehne {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
  z-index: 1;
}

.teil {
  position: absolute;
  left: var(--x);
  opacity: 0;
  animation-delay: var(--verzug);
  animation-duration: var(--dauer);
  animation-fill-mode: forwards;
}

/* Konfetti fällt von oben, dreht sich und driftet zur Seite. */
.buehne--konfetti .teil {
  top: 0;
  width: var(--wp-space-2);
  height: var(--wp-space-3);
  background: currentColor;
  animation-name: fallen;
  animation-timing-function: cubic-bezier(0.25, 0.4, 0.6, 1);
}

/* Herzen steigen von unten auf und verblassen. */
.buehne--herzen .teil {
  bottom: 0;
  font-size: var(--wp-font-size-2xl);
  line-height: 1;
  animation-name: steigen;
  animation-timing-function: ease-out;
}

.farbe-0 {
  color: var(--wp-color-accent-default);
}
.farbe-1 {
  color: var(--wp-color-status-ok);
}
.farbe-2 {
  color: var(--wp-color-status-warn);
}
.farbe-3 {
  color: var(--wp-color-status-error);
}
.farbe-4 {
  color: var(--wp-color-text-default);
}

/* Herzen sind rot — in „mono" folgt das Token dem Thema. */
.buehne--herzen .teil {
  color: var(--wp-color-status-error);
}

@keyframes fallen {
  0% {
    opacity: 1;
    transform: translate(0, -10vh) rotate(0deg);
  }
  85% {
    opacity: 1;
  }
  100% {
    opacity: 0;
    transform: translate(var(--drift), 100vh) rotate(var(--drehung));
  }
}

@keyframes steigen {
  0% {
    opacity: 0;
    transform: translate(0, 10%) scale(0.6);
  }
  15% {
    opacity: 1;
  }
  100% {
    opacity: 0;
    transform: translate(var(--drift), -70vh) scale(1.2);
  }
}

@media (prefers-reduced-motion: reduce) {
  .buehne {
    display: none;
  }
}
</style>

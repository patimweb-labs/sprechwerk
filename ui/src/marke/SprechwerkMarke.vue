<script setup lang="ts">
// Die Marke von Sprechwerk — erzeugt von `markenwerk` aus
// `weppelmann-ui/apps/markenwerk/marken/sprechwerk.pm`.
//
// Warum als Maske und nicht als eingebettetes SVG: die Zeichnung soll die Farbe
// des Umfelds annehmen (`currentColor`) wie jedes andere Symbol. Als Maske
// färbt CSS sie mit `background`, und die Datei bleibt die **erzeugte** Quelle —
// niemand zeichnet sie hier von Hand nach.
</script>

<template>
  <span class="marke" role="img" aria-label="Sprechwerk" />
</template>

<style scoped>
.marke {
  display: inline-block;
  flex: none;
  /* Ein Bildpunkt des Rasters ist ein Gerätepunkt — die Maske skaliert nicht
     weich, sondern bleibt kantig. */
  width: 1.5rem;
  height: 1.5rem;
  background: currentColor;
  -webkit-mask-image: url("./sprechwerk-symbolic.svg");
  mask-image: url("./sprechwerk-symbolic.svg");
  -webkit-mask-size: contain;
  mask-size: contain;
  -webkit-mask-repeat: no-repeat;
  mask-repeat: no-repeat;
  -webkit-mask-position: center;
  mask-position: center;
  image-rendering: pixelated;
}
</style>

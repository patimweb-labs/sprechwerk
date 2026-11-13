// Kleine Anzeigehelfer. Hier und nicht in den Ansichten, weil sie prüfbar
// sein sollen — und weil zwei Ansichten dieselbe Kennung zeigen.

import { t } from "./i18n";

/**
 * Kennung in Gruppen zu vier Zeichen: `3ff0:5a78:eb5d:…`.
 *
 * Der Server druckt seinen Zertifikat-Fingerabdruck genauso; wer beide
 * vergleicht, soll nicht zwei Schreibweisen im Kopf haben.
 */
export function kennungGruppiert(kennung: string, zeichen = 16): string {
  const roh = kennung.replace(/[^0-9a-fA-F]/g, "");
  if (!roh) return "—";
  const teil = roh.slice(0, zeichen);
  const gruppen = teil.match(/.{1,4}/g) ?? [];
  const rest = roh.length > zeichen ? "…" : "";
  return gruppen.join(":") + rest;
}

/**
 * „zuletzt vor 3 Tagen" — grob, aber ehrlich.
 *
 * Bewusst ohne Sekunden: bei „wann war ich zuletzt auf diesem Server" ist die
 * Größenordnung die Information, nicht die Genauigkeit.
 */
export function zuletztText(ms: number | null | undefined, jetzt = Date.now()): string | null {
  if (!ms || ms <= 0) return null;
  const sekunden = Math.max(0, Math.round((jetzt - ms) / 1000));
  if (sekunden < 90) return t("time.now");
  const minuten = Math.round(sekunden / 60);
  if (minuten < 60) return t("time.minutes", minuten);
  const stunden = Math.round(minuten / 60);
  if (stunden < 24) return t("time.hours", stunden);
  const tage = Math.round(stunden / 24);
  if (tage < 30) return t("time.days", tage);
  const monate = Math.round(tage / 30);
  if (monate < 12) return t("time.months", monate);
  return t("time.years", Math.round(monate / 12));
}

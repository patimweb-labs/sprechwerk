// Sprache der Oberfläche: Deutsch, Englisch, Spanisch, Französisch.
//
// Die Texte stehen in `texte.json` — eine Wahrheit, vier Sprachen. Fehlt ein
// Schlüssel in der gewählten Sprache, gilt Deutsch; fehlt er ganz, steht der
// Schlüssel da (sichtbar, nicht leer).
//
// Platzhalter sind `{}` und werden der Reihe nach gefüllt.
import { ref } from "vue";
import texte from "./texte.json";

export type Sprache = "de" | "en" | "es" | "fr";

export const SPRACHEN: { value: Sprache; label: string }[] = [
  { value: "de", label: "Deutsch" },
  { value: "en", label: "English" },
  { value: "es", label: "Español" },
  { value: "fr", label: "Français" },
];

/** Die gerade gewählte Sprache. Reaktiv, damit die Oberfläche sofort umschaltet. */
export const sprache = ref<Sprache>("de");

type Tabelle = Record<string, Partial<Record<Sprache, string>>>;
// Je nach Bündelung kommt das JSON direkt oder unter `default` — beides
// annehmen, sonst liefert `t()` nur die Schlüssel.
const tabelle = (((texte as unknown as { default?: Tabelle }).default ??
  texte) as Tabelle);

export function setSprache(neu: string) {
  sprache.value = (["de", "en", "es", "fr"] as const).includes(neu as Sprache)
    ? (neu as Sprache)
    : "de";
}

/** Text zu einem Schlüssel, Platzhalter `{}` werden der Reihe nach gefüllt. */
export function t(key: string, ...werte: (string | number)[]): string {
  const eintrag = tabelle[key];
  if (!eintrag) return key;
  let text = eintrag[sprache.value] ?? eintrag.de ?? key;
  for (const wert of werte) text = text.replace("{}", String(wert));
  return text;
}

/** Rollenname in der gewählten Sprache. */
export function rolle(wert: string): string {
  return t(`rolle.${wert}`);
}

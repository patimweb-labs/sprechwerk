// Kleine Anzeigehelfer. Hier und nicht in den Ansichten, weil sie prüfbar
// sein sollen — und weil zwei Ansichten dieselbe Kennung zeigen.

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

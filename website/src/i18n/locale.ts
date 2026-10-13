/*
 * Sprache der Website, fest zur Build-Zeit.
 *
 * `npm run build`    -> Deutsch, dist/     (deutsche-deadlock-community.de/streamer)
 * `npm run build:en` -> Englisch, dist-en/ (dead-lock.de/streamer)
 *
 * Kein Umschalter und keine Browser-Erkennung: Jede Domain liefert genau eine
 * Sprache aus, `hreflang` verknuepft beide Fassungen (Plan 4.10, E3). Weil die
 * Sprache eine Konstante ist, wirft Vite den jeweils anderen Text beim Bauen weg.
 */
export type SiteLang = "de" | "en";

export const SITE_LANG: SiteLang = import.meta.env.VITE_SITE_LANG === "en" ? "en" : "de";

export const IS_EN = SITE_LANG === "en";

/** Waehlt den Text der Build-Sprache. Funktioniert fuer Strings, JSX und Daten. */
export function tx<T>(de: T, en: T): T {
  return IS_EN ? en : de;
}

/** BCP-47-Tag fuer `Intl`/`toLocaleString`. */
export const SITE_LOCALE = IS_EN ? "en-US" : "de-DE";

/** Oeffentliche Basis-URLs beider Fassungen, fuer hreflang und Sprachverweise. */
export const STREAMER_URL_DE = "https://deutsche-deadlock-community.de/streamer/";
export const STREAMER_URL_EN = "https://dead-lock.de/streamer/";

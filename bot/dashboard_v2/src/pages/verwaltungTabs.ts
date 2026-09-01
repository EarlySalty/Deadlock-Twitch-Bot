/**
 * Tab-Aufteilung der Verwaltungsseite. Reine Logik, damit sich Deeplink und
 * Fallback ohne DOM testen lassen — das JSX pro Tab liegt in Verwaltung.tsx.
 */

export const VERWALTUNG_TAB_IDS = ['konto', 'chat', 'bot', 'overlay', 'werbung'] as const;

export type VerwaltungTabId = (typeof VERWALTUNG_TAB_IDS)[number];

/**
 * Liest den aktiven Tab aus dem URL-Hash. Unbekannte oder leere Werte landen
 * auf dem ersten Tab, statt eine leere Seite zu zeigen.
 */
export function resolveVerwaltungTab(hash: string | undefined | null): VerwaltungTabId {
  const wanted = String(hash ?? '')
    .trim()
    .replace(/^#/, '')
    .trim()
    .toLowerCase();
  const match = VERWALTUNG_TAB_IDS.find((id) => id === wanted);
  return match ?? VERWALTUNG_TAB_IDS[0];
}

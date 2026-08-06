import type { PartnerAccessEntry } from '@/api/types';

/**
 * Sucht den Eintrag eines Streamers in der Partner-Access-Liste.
 *
 * Case-insensitive, weil das Backend `social_media_partner_access` ebenfalls
 * über `LOWER(streamer_login)` vergleicht. Einzige Stelle, die diese
 * Zuordnung kennt — Anzeige und Status lesen beide hier.
 */
export function findPartnerAccessEntry(
  entries: PartnerAccessEntry[] | undefined | null,
  login: string | undefined | null,
): PartnerAccessEntry | undefined {
  const wanted = String(login ?? '')
    .trim()
    .toLowerCase();
  if (!wanted || !Array.isArray(entries)) {
    return undefined;
  }
  return entries.find(
    (entry) => String(entry?.streamer_login ?? '').trim().toLowerCase() === wanted,
  );
}

/**
 * Freigabestatus eines Streamers. Fehlt der Eintrag, gilt der Streamer als
 * nicht freigegeben — dieselbe fail-closed-Regel wie im Backend-Guard.
 */
export function resolvePartnerGranted(
  entries: PartnerAccessEntry[] | undefined | null,
  login: string | undefined | null,
): boolean {
  return Boolean(findPartnerAccessEntry(entries, login)?.granted);
}

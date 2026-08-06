import type { PartnerAccessEntry } from '@/api/types';

/**
 * Liest den Freigabestatus eines Streamers aus der Partner-Access-Liste.
 *
 * Case-insensitive, weil das Backend `social_media_partner_access` ebenfalls
 * über `LOWER(streamer_login)` vergleicht. Fehlt der Eintrag, gilt der
 * Streamer als nicht freigegeben — dieselbe fail-closed-Regel wie im
 * Backend-Guard.
 */
export function resolvePartnerGranted(
  entries: PartnerAccessEntry[] | undefined | null,
  login: string | undefined | null,
): boolean {
  const wanted = String(login ?? '')
    .trim()
    .toLowerCase();
  if (!wanted || !Array.isArray(entries)) {
    return false;
  }
  const match = entries.find(
    (entry) => String(entry?.streamer_login ?? '').trim().toLowerCase() === wanted,
  );
  return Boolean(match?.granted);
}

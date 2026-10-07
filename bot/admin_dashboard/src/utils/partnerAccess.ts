import type { PartnerAccessEntry } from '@/api/types';

export function findPartnerAccessEntry(
  entries: PartnerAccessEntry[] | undefined | null,
  login: string | undefined | null,
  twitchUserId?: string,
): PartnerAccessEntry | undefined {
  if (!Array.isArray(entries)) return undefined;
  if (twitchUserId) {
    return entries.find((entry) => entry.twitch_user_id === twitchUserId);
  }
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

import { fetchJson, withCookieCredentials } from './core';

const BASE = '/twitch/api/v2/streamer/community-hints';

export interface CommunityHintsSettings {
  enabled: boolean;
  can_disable: boolean;
}

export async function fetchCommunityHints(): Promise<CommunityHintsSettings> {
  return fetchJson(BASE, withCookieCredentials());
}

export async function saveCommunityHints(enabled: boolean): Promise<CommunityHintsSettings> {
  return fetchJson(
    BASE,
    withCookieCredentials({
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    }),
  );
}

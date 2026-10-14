import { withLanguageHeader } from './languageHeader';

const BASE = '/twitch/api/v2/streamer/silent-settings';

export interface SilentSettings {
  silent_ban: boolean;
  silent_raid: boolean;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(
    path,
    withLanguageHeader(path, { credentials: 'same-origin', ...init })
  );
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as T;
}

export async function fetchSilentSettings(): Promise<SilentSettings> {
  return fetchJson(BASE);
}

export async function saveSilentSettings(settings: SilentSettings): Promise<SilentSettings> {
  return fetchJson(BASE, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  });
}

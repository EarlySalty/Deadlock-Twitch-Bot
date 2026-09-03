const BASE = '/twitch/api/v2/streamer/moderation';

export interface ModerationSettings {
  global_ban_enabled: boolean;
  scam_pitch_enabled: boolean;
  spam_autoban_enabled: boolean;
  sus_invite_enabled: boolean;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new Error(payload?.error || `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
}

export async function fetchModerationSettings(): Promise<ModerationSettings> {
  return fetchJson(`${BASE}/settings`);
}

export async function saveModerationSettings(
  settings: ModerationSettings,
): Promise<ModerationSettings> {
  return fetchJson(`${BASE}/settings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  });
}

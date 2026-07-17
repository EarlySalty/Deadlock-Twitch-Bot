export interface LurkCommandSettingsResponse {
  lurk_command_enabled: boolean;
}

export interface LurkCommandUpdateResponse {
  ok: boolean;
  lurk_command_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/lurk-command-settings';

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const body = await response.json();
      if (body?.error) message = String(body.error);
      else if (body?.message) message = String(body.message);
    } catch {
      // keep default message
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

export async function fetchLurkCommandSettings(
  streamer?: string,
): Promise<LurkCommandSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleLurkCommand(
  enabled: boolean,
  streamer?: string,
): Promise<LurkCommandUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ lurk_command_enabled: enabled }),
  });
}

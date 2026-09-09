export interface TitleCommandSettingsResponse {
  title_command_enabled: boolean;
}

export interface TitleCommandUpdateResponse {
  ok: boolean;
  title_command_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/title-command-settings';

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

export async function fetchTitleCommandSettings(
  streamer?: string,
): Promise<TitleCommandSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleTitleCommand(
  enabled: boolean,
  streamer?: string,
): Promise<TitleCommandUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ title_command_enabled: enabled }),
  });
}

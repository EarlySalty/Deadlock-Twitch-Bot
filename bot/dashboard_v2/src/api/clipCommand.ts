export interface ClipCommandSettingsResponse {
  clip_command_enabled: boolean;
}

export interface ClipCommandUpdateResponse {
  ok: boolean;
  clip_command_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/clip-command-settings';

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

export async function fetchClipCommandSettings(
  streamer?: string,
): Promise<ClipCommandSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleClipCommand(
  enabled: boolean,
  streamer?: string,
): Promise<ClipCommandUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ clip_command_enabled: enabled }),
  });
}

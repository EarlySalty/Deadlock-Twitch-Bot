export interface GreetingSettingsResponse {
  greeting_reply_enabled: boolean;
}

export interface GreetingUpdateResponse {
  ok: boolean;
  greeting_reply_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/greeting-settings';

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

export async function fetchGreetingSettings(
  streamer?: string,
): Promise<GreetingSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleGreeting(
  enabled: boolean,
  streamer?: string,
): Promise<GreetingUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ greeting_reply_enabled: enabled }),
  });
}

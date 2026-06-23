export interface LurkerTaxSettingsResponse {
  lurker_tax_enabled: boolean;
  has_moderator_read_chatters: boolean;
}

export interface LurkerTaxUpdateResponse {
  ok: boolean;
  lurker_tax_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/lurker-tax-settings';

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
      // ignore json parse error, keep default message
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

export async function fetchLurkerTaxSettings(
  streamer?: string,
): Promise<LurkerTaxSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleLurkerTax(
  enabled: boolean,
  streamer?: string,
): Promise<LurkerTaxUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ lurker_tax_enabled: enabled }),
  });
}

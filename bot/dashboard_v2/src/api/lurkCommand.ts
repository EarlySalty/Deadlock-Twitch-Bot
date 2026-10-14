import { withLanguageHeader } from './languageHeader';

export interface LurkCommandSettingsResponse {
  lurk_command_enabled: boolean;
}

export interface LurkCommandUpdateResponse {
  ok: boolean;
  lurk_command_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/lurk-command-settings';

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(
    path,
    withLanguageHeader(path, { credentials: 'same-origin', ...init })
  );
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: unknown;
      message?: unknown;
    } | null;
    const message = body?.error
      ? String(body.error)
      : body?.message
        ? String(body.message)
        : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return (await response.json()) as T;
}

export async function fetchLurkCommandSettings(
  streamer?: string
): Promise<LurkCommandSettingsResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`);
}

export async function toggleLurkCommand(
  enabled: boolean,
  streamer?: string
): Promise<LurkCommandUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ lurk_command_enabled: enabled }),
  });
}
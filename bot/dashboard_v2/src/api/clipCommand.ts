import { tr } from '../i18n/dictionary';
import { withLanguageHeader } from './languageHeader';

export interface ClipCommandSettingsResponse {
  clip_command_enabled: boolean;
}

export interface ClipCommandUpdateResponse {
  ok: boolean;
  clip_command_enabled: boolean;
}

const BASE = '/twitch/api/v2/streamer/clip-command-settings';

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, withLanguageHeader(path, { credentials: 'same-origin', ...init }));
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { error?: unknown; message?: unknown } | null;
    const message = body?.error ? String(body.error) : body?.message ? String(body.message) : `HTTP ${response.status}`;
    throw new Error(tr(message));
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

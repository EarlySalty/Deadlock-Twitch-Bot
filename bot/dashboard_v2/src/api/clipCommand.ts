import { tr } from '../i18n/dictionary';
import { withLanguageHeader } from './languageHeader';

export interface ClipCommandSettingsResponse {
  clip_command_enabled: boolean;
  clip_command_duration_seconds: number;
  clip_command_duration_configured: boolean;
}

export interface ClipCommandUpdateResponse extends ClipCommandSettingsResponse {
  ok: boolean;
}

export interface PrivateVodJob {
  id: string;
  video_id: string;
  start_seconds: number;
  end_seconds: number;
  status: 'queued' | 'running' | 'ready' | 'failed' | 'expired';
  expires_at: string;
  file_bytes: number | null;
  error_code: string | null;
}

export interface PrivateVodClipsResponse {
  enabled: boolean;
  jobs: PrivateVodJob[];
}

export function fetchPrivateVodClips(): Promise<PrivateVodClipsResponse> {
  return fetchJson('/twitch/api/v2/streamer/private-vod-clips');
}

export function setPrivateVodCommand(enabled: boolean): Promise<{ enabled: boolean }> {
  return fetchJson('/twitch/api/v2/streamer/private-vod-clips', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  });
}

export function privateVodDownloadUrl(id: string): string {
  return `/twitch/api/v2/streamer/private-vod-clips/${encodeURIComponent(id)}/download`;
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

export async function setClipCommandDuration(
  seconds: number,
  streamer?: string,
): Promise<ClipCommandUpdateResponse> {
  const qs = streamer ? `?streamer=${encodeURIComponent(streamer)}` : '';
  return fetchJson(`${BASE}${qs}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ clip_command_duration_seconds: seconds }),
  });
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

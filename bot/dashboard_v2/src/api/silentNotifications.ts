/**
 * Silent-Notification-Flags (Verwaltung) — JSON-API gegen
 * /twitch/api/v2/streamer/silent-settings (nativer Rust-Handler auf tb-dashboard).
 *
 * Auth: same-origin cookie (gleicher Mechanismus wie der Rest des Dashboards).
 * Synchron zu den Chat-Befehlen !silentban / !silentraid — dieselben
 * twitch_partners-Spalten (silent_ban, silent_raid).
 */

const BASE = '/twitch/api/v2/streamer/silent-settings';

export interface SilentSettings {
  silent_ban: boolean;
  silent_raid: boolean;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });
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

import { tr } from '../i18n/dictionary';
import { withLanguageHeader } from './languageHeader';

const BASE = '/twitch/api/v2/streamer/scam-guard';

export type ScamGuardMode = 'auto_ban' | 'timeout' | 'alert_only';

export interface ScamGuardSettings {
  enabled: boolean;
  mode: ScamGuardMode;
  threshold: number;
  suggestion_floor: number;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, withLanguageHeader(path, { credentials: 'same-origin', ...init }));
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new Error(tr(payload?.error || `HTTP ${response.status}`));
  }
  return (await response.json()) as T;
}

export async function fetchScamSettings(): Promise<ScamGuardSettings> {
  return fetchJson(`${BASE}/settings`);
}

export async function saveScamSettings(settings: ScamGuardSettings): Promise<ScamGuardSettings> {
  return fetchJson(`${BASE}/settings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  });
}

/**
 * Einstellungen des Scam-Schutzes in der Verwaltung.
 * Die JSON-API unter /twitch/api/v2/streamer/scam-guard nutzt das Session-Cookie.
 */

const BASE = '/twitch/api/v2/streamer/scam-guard';

/** Verhalten bei hoher Sicherheit. Muss exakt zu VALID_MODES im Backend passen. */
export type ScamGuardMode = 'auto_ban' | 'timeout' | 'alert_only';

export interface ScamGuardSettings {
  enabled: boolean;
  mode: ScamGuardMode;
  /** Schwelle für die automatische Aktion (0–1, Default 0.90). */
  threshold: number;
  /** Schwelle, ab der ein Fall in die Vorschlags-Queue wandert (0–1, Default 0.70). */
  suggestion_floor: number;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });
  if (!response.ok) {
    // Fehlertext aus { error: "..." } ziehen, sonst HTTP-Code.
    const payload = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new Error(payload?.error || `HTTP ${response.status}`);
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

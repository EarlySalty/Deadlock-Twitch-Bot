/**
 * AI-Engagement Verwaltung — JSON-API gegen /twitch/api/v2/engagement/*.
 *
 * Auth: same-origin cookie (gleicher Mechanismus wie der Rest des Dashboards).
 * Permission: super_mod sieht/togglet alle Channels, andere nur den eigenen.
 */

export interface EngagementSettings {
  channelLogin: string;
  enabled: boolean;
  steamId: string | null;
  personaOverride: string | null;
  tabuTopics: string[];
  enabledAt: string | null;
  enabledBy: string | null;
  updatedAt: string | null;
}

export interface EngagementLogEntry {
  decision: string;
  responseText: string | null;
  model: string | null;
  promptTokens: number | null;
  completionTokens: number | null;
  costUsdEstimate: number | null;
  latencyMs: number | null;
  ts: string | null;
}

export interface EngagementSettingsResponse {
  settings: EngagementSettings[];
  isSuperMod: boolean;
  actorLogin: string | null;
}

export interface EngagementLogResponse {
  channelLogin: string;
  entries: EngagementLogEntry[];
}

const BASE = '/twitch/api/v2/engagement';

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

export async function fetchEngagementSettings(
  channel?: string,
): Promise<EngagementSettingsResponse> {
  const qs = channel ? `?channel=${encodeURIComponent(channel)}` : '';
  return fetchJson(`${BASE}/settings${qs}`);
}

export async function toggleEngagement(
  channelLogin: string,
  enabled: boolean,
): Promise<{ settings: EngagementSettings | null }> {
  return fetchJson(`${BASE}/toggle`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ channelLogin, enabled }),
  });
}

export interface EngagementUpdatePayload {
  steamId?: string | null;
  personaOverride?: string | null;
  tabuTopics?: string[] | null;
}

export async function updateEngagement(
  channelLogin: string,
  payload: EngagementUpdatePayload,
): Promise<{ settings: EngagementSettings | null }> {
  return fetchJson(`${BASE}/update`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ channelLogin, ...payload }),
  });
}

export async function fetchEngagementLog(
  channel: string,
  limit = 25,
): Promise<EngagementLogResponse> {
  return fetchJson(
    `${BASE}/log?channel=${encodeURIComponent(channel)}&limit=${limit}`,
  );
}

import { fetchJson, withCookieCredentials } from './core';

export const AD_DURATION_OPTIONS = [30, 60, 90, 120, 150, 180] as const;

export type AdManagerStrategy = 'monitor' | 'snooze' | 'smart';

export interface AdManagerSettingsInput {
  enabled: boolean;
  strategy: AdManagerStrategy;
  adDurationSeconds: number;
  minIntervalMinutes: number;
  startupDelayMinutes: number;
  quietWindowMinutes: number;
  actionLeadSeconds: number;
}

export interface AdManagerSettings extends AdManagerSettingsInput {
  updatedAt: string | null;
}

export interface AdManagerLastAction {
  kind: string;
  outcome: string;
  detail: string | null;
  at: string;
}

export interface AdManagerStatus {
  isLive: boolean;
  nextAdAt: string | null;
  lastAdAt: string | null;
  durationSeconds: number | null;
  prerollFreeSeconds: number | null;
  snoozeCount: number | null;
  snoozeRefreshAt: string | null;
  observedAt: string | null;
  workerHealthy: boolean;
  workerHeartbeatAt: string | null;
  lastAction: AdManagerLastAction | null;
  scopes: {
    read: boolean;
    snooze: boolean;
    commercial: boolean;
  };
}

export interface AdManagerResponse {
  settings: AdManagerSettings;
  status: AdManagerStatus;
}

export type AdManagerAction =
  | { action: 'snooze' }
  | { action: 'commercial'; durationSeconds: number };

const BASE = '/twitch/api/v2/streamer/ad-manager';
const REAUTH_FALLBACK = '/twitch/raid/auth?scope_profile=dashboard_reauth';

function clampInteger(value: number, min: number, max: number, fallback: number): number {
  const finite = Number.isFinite(value) ? Math.round(value) : fallback;
  return Math.min(max, Math.max(min, finite));
}

function normalizeDuration(value: number): number {
  if (!Number.isFinite(value)) return 90;
  return AD_DURATION_OPTIONS.reduce((nearest, option) =>
    Math.abs(option - value) < Math.abs(nearest - value) ? option : nearest,
  );
}

export function normalizeAdManagerSettings(
  settings: AdManagerSettingsInput,
): AdManagerSettingsInput {
  const strategy: AdManagerStrategy = ['monitor', 'snooze', 'smart'].includes(settings.strategy)
    ? settings.strategy
    : 'monitor';
  return {
    enabled: Boolean(settings.enabled),
    strategy,
    adDurationSeconds: normalizeDuration(settings.adDurationSeconds),
    minIntervalMinutes: clampInteger(settings.minIntervalMinutes, 8, 180, 30),
    startupDelayMinutes: clampInteger(settings.startupDelayMinutes, 0, 180, 15),
    quietWindowMinutes: clampInteger(settings.quietWindowMinutes, 0, 60, 5),
    actionLeadSeconds: clampInteger(settings.actionLeadSeconds, 10, 300, 60),
  };
}

export function adManagerSettingsInput(settings: AdManagerSettings): AdManagerSettingsInput {
  return normalizeAdManagerSettings(settings);
}

export function adManagerReauthUrl(reconnectUrl: string): string {
  try {
    const base = new URL('https://dashboard.invalid');
    const parsed = new URL(reconnectUrl, base);
    if (parsed.origin !== base.origin || !parsed.pathname.startsWith('/twitch/')) {
      return REAUTH_FALLBACK;
    }
    parsed.searchParams.set('scope_profile', 'dashboard_reauth');
    return `${parsed.pathname}${parsed.search}${parsed.hash}`;
  } catch {
    return REAUTH_FALLBACK;
  }
}

export async function fetchAdManager(signal?: AbortSignal): Promise<AdManagerResponse> {
  return fetchJson<AdManagerResponse>(BASE, withCookieCredentials({ signal }));
}

export async function saveAdManagerSettings(
  settings: AdManagerSettingsInput,
): Promise<AdManagerResponse> {
  return fetchJson<AdManagerResponse>(BASE, withCookieCredentials({
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(normalizeAdManagerSettings(settings)),
  }));
}

export async function queueAdManagerAction(
  action: AdManagerAction,
  idempotencyKey: string,
): Promise<{ queued: boolean }> {
  const normalized = action.action === 'commercial'
    ? { ...action, durationSeconds: normalizeDuration(action.durationSeconds) }
    : action;
  return fetchJson<{ queued: boolean }>(`${BASE}/action`, withCookieCredentials({
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...normalized, idempotencyKey }),
  }));
}

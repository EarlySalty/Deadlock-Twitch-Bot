import { DASHBOARD_V2_LOGIN_FALLBACK, fetchJson, withCookieCredentials } from './core';

const ONBOARDING_PATH = '/twitch/api/v2/streamer/onboarding';
const TIP_SETTINGS_PATH = '/twitch/api/v2/streamer/tip-settings';

export interface OnboardingStatus {
  current_step: number;
  completed: boolean;
  discord_linked: boolean;
  steam_linked: boolean;
}

export interface OnboardingUpdate {
  current_step?: number;
  completed?: boolean;
}

export interface OnboardingSaveResponse {
  ok: boolean;
  current_step: number;
  completed: boolean;
}

export interface TipSettings {
  opt_out: boolean;
}

export async function fetchOnboardingStatus(): Promise<OnboardingStatus> {
  return fetchJson<OnboardingStatus>(
    new URL(ONBOARDING_PATH, window.location.origin),
    withCookieCredentials({
      headers: { Accept: 'application/json' },
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK },
  );
}

export async function saveOnboardingProgress(
  update: OnboardingUpdate,
): Promise<OnboardingSaveResponse> {
  return fetchJson<OnboardingSaveResponse>(
    new URL(ONBOARDING_PATH, window.location.origin),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(update),
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK },
  );
}

export async function fetchTipSettings(): Promise<TipSettings> {
  return fetchJson<TipSettings>(
    new URL(TIP_SETTINGS_PATH, window.location.origin),
    withCookieCredentials({
      headers: { Accept: 'application/json' },
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK },
  );
}

export async function saveTipSettings(settings: TipSettings): Promise<TipSettings> {
  return fetchJson<TipSettings>(
    new URL(TIP_SETTINGS_PATH, window.location.origin),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(settings),
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK },
  );
}

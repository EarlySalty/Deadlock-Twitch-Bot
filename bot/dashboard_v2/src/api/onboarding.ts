import { DASHBOARD_V2_LOGIN_FALLBACK, fetchJson, withCookieCredentials } from './core';
import type { OnboardingStepId } from '../components/onboarding/steps';

const ONBOARDING_PATH = '/twitch/api/v2/streamer/onboarding';
export type ConnectionStatus = 'connected' | 'missing' | 'error';
export interface OnboardingProgress {
  current_step: number;
  completed: boolean;
  active_step: OnboardingStepId;
  completed_step_ids: OnboardingStepId[];
  paused: boolean;
}
export interface OnboardingStatus extends OnboardingProgress {
  discord_linked: boolean;
  steam_linked: boolean;
  discord_status: ConnectionStatus;
  steam_status: ConnectionStatus;
}
export interface OnboardingUpdate {
  active_step?: OnboardingStepId;
  complete_step?: OnboardingStepId;
  paused?: boolean;
  completed?: true;
}
export async function fetchOnboardingStatus(): Promise<OnboardingStatus> {
  return fetchJson(new URL(ONBOARDING_PATH, window.location.origin), withCookieCredentials({
    headers: { Accept: 'application/json' },
  }), { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK });
}
export async function saveOnboardingProgress(update: OnboardingUpdate, csrfToken?: string | null): Promise<{ok: boolean; progress: OnboardingProgress}> {
  return fetchJson(new URL(ONBOARDING_PATH, window.location.origin), withCookieCredentials({
    method: 'POST',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json', ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}) },
    body: JSON.stringify(update),
  }), { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK });
}

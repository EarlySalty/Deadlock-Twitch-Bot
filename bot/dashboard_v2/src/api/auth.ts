import { DASHBOARD_V2_LOGIN_FALLBACK, fetchApi, fetchJson, withCookieCredentials } from './core';
import type { EntitlementId, PlanTier } from '../types/billing';

export interface AuthStatus {
  authenticated: boolean;
  level: 'localhost' | 'admin' | 'partner' | 'none';
  demoMode: boolean;
  isAdmin: boolean;
  isLocalhost: boolean;
  /** Twitch-OAuth-Admin (z. B. earlysalty), der den Admin-Modus umschalten darf. */
  adminEligible?: boolean;
  /** `true`, solange der Admin-Vollzugriff per Schalter aktiv ist. */
  adminMode?: boolean;
  canViewAllStreamers: boolean;
  twitchLogin?: string | null;
  adminDefaultStreamer?: string | null;
  displayName?: string | null;
  partnerStatus?: 'active' | 'archived' | 'departnered' | 'non_partner' | 'token_error' | 'blocked' | null;
  technicalPauseReason?: string | null;
  operationalState?: string | null;
  canAccessAnalyticsDashboard?: boolean;
  tokenErrorGraceExpiresAt?: string | null;
  csrfToken?: string | null;
  csrf_token?: string | null;
  access?: {
    landing: boolean;
    analytics: boolean;
  };
  permissions: {
    viewAllStreamers: boolean;
    viewComparison: boolean;
    viewChatAnalytics: boolean;
    viewOverlap: boolean;
  };
  plan?: {
    planId: string | null;
    planName: string | null;
    tier: PlanTier;
    isExtended: boolean;
    expiresAt: string | null;
    source: string | null;
    entitlements: EntitlementId[];
  } | null;
}

export async function fetchAuthStatus(): Promise<AuthStatus> {
  return fetchApi<AuthStatus>('/auth-status');
}

/**
 * Schaltet den dedizierten Admin-Modus für die aktuelle Session um.
 *
 * Default (ohne aktiven Modus) sieht ein Admin das Dashboard wie ein normaler
 * Nutzer (echter Plan, gesperrte Inhalte). `enabled: true` setzt ein
 * Session-Cookie und liefert wieder den vollen Admin-Zugriff. Das CSRF-Token
 * wird nur mitgesendet, wenn die Session eines bereitstellt (Parität zu
 * `title`-/`admin`-Mutations).
 */
export async function setAdminMode(
  enabled: boolean,
  csrfToken?: string | null,
): Promise<{ adminMode: boolean }> {
  return fetchJson<{ adminMode: boolean }>(
    new URL('/twitch/api/v2/admin-mode', window.location.origin),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
      },
      body: JSON.stringify({ enabled }),
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK },
  );
}

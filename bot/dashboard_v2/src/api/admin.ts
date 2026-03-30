import { DASHBOARD_V2_LOGIN_FALLBACK, fetchJson, withCookieCredentials } from './core';

export interface Affiliate {
  login: string;
  display_name: string;
  active: boolean;
  total_claims: number;
  total_provision: number;
  created_at: string;
  last_claim_at: string | null;
}

export interface AffiliateStats {
  total_affiliates: number;
  active_affiliates: number;
  total_claims: number;
  total_provision: number;
  this_month_claims: number;
  this_month_provision: number;
}

export interface AffiliateDetail {
  affiliate: { login: string; display_name: string; active: boolean; created_at: string };
  claims: Array<{
    id: number;
    customer_login: string;
    claimed_at: string;
    commission_cents: number;
    commission_count: number;
  }>;
  stats: { total_claims: number; total_provision: number; avg_provision: number; active_customers: number };
}

export async function fetchAdminAffiliates(): Promise<{ affiliates: Affiliate[] }> {
  return fetchJson<{ affiliates: Affiliate[] }>(
    new URL('/twitch/api/admin/affiliates', window.location.origin),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

export async function fetchAdminAffiliateStats(): Promise<AffiliateStats> {
  return fetchJson<AffiliateStats>(
    new URL('/twitch/api/admin/affiliates/stats', window.location.origin),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

export async function fetchAdminAffiliateDetail(login: string): Promise<AffiliateDetail> {
  return fetchJson<AffiliateDetail>(
    new URL(`/twitch/api/admin/affiliates/${login}`, window.location.origin),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

export async function toggleAffiliate(
  login: string,
  csrfToken: string | null | undefined
): Promise<{ login: string; active: boolean }> {
  if (!csrfToken) {
    throw new Error('Missing CSRF token');
  }

  return fetchJson<{ login: string; active: boolean }>(
    new URL(`/twitch/api/admin/affiliates/${login}/toggle`, window.location.origin),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'X-CSRF-Token': csrfToken,
      },
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

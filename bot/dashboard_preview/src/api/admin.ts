import { getPreviewAdminFixture } from '../preview/fixtures';
import { isPreviewLocalhost } from '../preview/routes';
import { DASHBOARD_V2_LOGIN_FALLBACK, fetchJson, withCookieCredentials } from './core';

export interface Affiliate {
  login: string;
  display_name: string;
  active: boolean;
  commission_rate_pct: number;
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
  affiliate: {
    login: string;
    display_name: string;
    active: boolean;
    commission_rate_pct: number;
    created_at: string;
  };
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
  if (isPreviewLocalhost()) {
    return getPreviewAdminFixture('/twitch/api/admin/affiliates') as { affiliates: Affiliate[] };
  }
  return fetchJson<{ affiliates: Affiliate[] }>(
    new URL('/twitch/api/admin/affiliates', window.location.origin),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

export async function fetchAdminAffiliateStats(): Promise<AffiliateStats> {
  if (isPreviewLocalhost()) {
    return getPreviewAdminFixture('/twitch/api/admin/affiliates/stats') as AffiliateStats;
  }
  return fetchJson<AffiliateStats>(
    new URL('/twitch/api/admin/affiliates/stats', window.location.origin),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

export async function fetchAdminAffiliateDetail(login: string): Promise<AffiliateDetail> {
  if (isPreviewLocalhost()) {
    return getPreviewAdminFixture(`/twitch/api/admin/affiliates/${login}`) as AffiliateDetail;
  }
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

  if (isPreviewLocalhost()) {
    return getPreviewAdminFixture(`/twitch/api/admin/affiliates/${login}/toggle`) as {
      login: string;
      active: boolean;
    };
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

export async function setAffiliateCommissionRate(
  login: string,
  commissionRatePct: number,
  csrfToken: string | null | undefined
): Promise<{ login: string; commission_rate_pct: number }> {
  if (!csrfToken) {
    throw new Error('Missing CSRF token');
  }

  if (isPreviewLocalhost()) {
    return getPreviewAdminFixture(`/twitch/api/admin/affiliates/${login}/commission-rate`) as {
      login: string;
      commission_rate_pct: number;
    };
  }

  return fetchJson<{ login: string; commission_rate_pct: number }>(
    new URL(`/twitch/api/admin/affiliates/${login}/commission-rate`, window.location.origin),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      body: JSON.stringify({ commission_rate_pct: commissionRatePct }),
    }),
    { loginFallback: DASHBOARD_V2_LOGIN_FALLBACK }
  );
}

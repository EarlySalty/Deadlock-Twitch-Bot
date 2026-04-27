import { buildApiUrl, DASHBOARD_V2_LOGIN_FALLBACK, fetchJson, withCookieCredentials } from './core';

export interface AffiliatePortalData {
  affiliate: { login: string; display_name: string; active: boolean; referral_code: string; referral_url: string };
  stats: { total_claims: number; total_provision: number; this_month_claims: number; this_month_provision: number; pending_payout: number };
  recent_claims: Array<{ customer_display_name: string; plan_name: string | null; amount: number; created_at: string }>;
}

export async function fetchAffiliatePortal(): Promise<AffiliatePortalData> {
  return fetchJson<AffiliatePortalData>(
    buildApiUrl('/affiliate/portal'),
    withCookieCredentials({ headers: { Accept: 'application/json' } }),
    {
      loginFallback: DASHBOARD_V2_LOGIN_FALLBACK,
      notFoundMessage: 'affiliate_not_found',
    }
  );
}

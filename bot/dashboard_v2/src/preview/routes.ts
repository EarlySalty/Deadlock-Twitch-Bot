const PREVIEW_MODE = import.meta.env.MODE === 'preview';
const LOCALHOST_HOSTNAMES = new Set(['localhost', '127.0.0.1']);

export const PREVIEW_ANALYTICS_ROUTE = PREVIEW_MODE ? '/' : '/analyse';
export const PREVIEW_HOME_ROUTE = PREVIEW_MODE ? '/dashboard' : '/twitch/dashboard';
export const PREVIEW_VERWALTUNG_ROUTE = PREVIEW_MODE ? '/verwaltung' : '/twitch/verwaltung';
export const PREVIEW_PRICING_ROUTE = PREVIEW_MODE ? '/pricing' : '/twitch/pricing';
export const PREVIEW_BILLING_ROUTE = `${PREVIEW_PRICING_ROUTE}#plans`;
export const PREVIEW_CHANGELOG_ROUTE = `${PREVIEW_HOME_ROUTE}#changelog`;

export function isPreviewModeEnabled(): boolean {
  return PREVIEW_MODE;
}

export function analyticsTabHref(tab: string = 'overview'): string {
  const search = new URLSearchParams();
  if (tab && tab !== 'overview') {
    search.set('tab', tab);
  }
  const query = search.toString();
  return query ? `${PREVIEW_ANALYTICS_ROUTE}?${query}` : PREVIEW_ANALYTICS_ROUTE;
}

export function isPreviewLocalhost(): boolean {
  return PREVIEW_MODE && LOCALHOST_HOSTNAMES.has(window.location.hostname);
}

export function getPlanCheckoutHref(planId?: string | null, isFreePlan = false, cycle: 1 | 12 = 1): string {
  if (PREVIEW_MODE) {
    return PREVIEW_BILLING_ROUTE;
  }
  if (isFreePlan || !planId) {
    return '/twitch/pricing';
  }
  return `/twitch/abbo/bezahlen?plan_id=${encodeURIComponent(planId)}&cycle=${cycle}&quantity=1`;
}

const PREVIEW_MODE = import.meta.env?.MODE === 'preview';

export const PREVIEW_ANALYTICS_ROUTE = PREVIEW_MODE ? '/' : '/analyse';
export const PREVIEW_HOME_ROUTE = PREVIEW_MODE ? '/dashboard' : '/twitch/dashboard';
export const PREVIEW_VERWALTUNG_ROUTE = PREVIEW_MODE ? '/verwaltung' : '/twitch/verwaltung';
export const PREVIEW_OVERLAY_ROUTE = PREVIEW_MODE ? '/overlay' : '/twitch/overlay';
export const PREVIEW_TITLE_ROUTE = PREVIEW_MODE ? '/titel' : '/twitch/titel';
export const PREVIEW_UPLINK_ROUTE = PREVIEW_MODE ? '/uplink' : '/twitch/uplink';
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
  return withPreviewVariant(query ? `${PREVIEW_ANALYTICS_ROUTE}?${query}` : PREVIEW_ANALYTICS_ROUTE);
}

export function isPreviewLocalhost(): boolean {
  return PREVIEW_MODE;
}

export function withPreviewVariant(href: string): string {
  if (!PREVIEW_MODE) return href;
  const variant = new URLSearchParams(window.location.search).get('gold');
  if (!variant) return href;
  const url = new URL(href, window.location.origin);
  url.searchParams.set('gold', variant);
  return `${url.pathname}${url.search}${url.hash}`;
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

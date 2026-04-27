export const PREVIEW_ANALYTICS_ROUTE = '/';
export const PREVIEW_HOME_ROUTE = '/dashboard';
export const PREVIEW_VERWALTUNG_ROUTE = '/verwaltung';
export const PREVIEW_PRICING_ROUTE = '/pricing';
export const PREVIEW_BILLING_ROUTE = '/pricing#plans';
export const PREVIEW_CHANGELOG_ROUTE = '/dashboard#changelog';

export function analyticsTabHref(tab: string = 'overview'): string {
  const search = new URLSearchParams();
  if (tab && tab !== 'overview') {
    search.set('tab', tab);
  }
  const query = search.toString();
  return query ? `${PREVIEW_ANALYTICS_ROUTE}?${query}` : PREVIEW_ANALYTICS_ROUTE;
}

export function isPreviewLocalhost(): boolean {
  return (
    window.location.hostname === 'localhost' ||
    window.location.hostname === '127.0.0.1'
  );
}

import type { MouseEvent } from 'react';
import { readInitialLanguage, translate } from './i18n/dictionary';
import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_SOCIAL_ROUTE,
  PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
} from './preview/routes';

const dashboardPaths = new Set([
  PREVIEW_ANALYTICS_ROUTE, PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE, PREVIEW_SOCIAL_ROUTE, PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE, PREVIEW_VERWALTUNG_ROUTE,
  '/twitch/analyse', '/twitch/dashboard-v2', '/dashboard-v2',
  '/twitch/challenges', '/twitch/hilfe', '/twitch/feedback',
  '/twitch/social-media', '/twitch/kategorie', '/twitch/onboarding',
]);

export function dashboardDestination(href: string): URL | null {
  const url = new URL(href, window.location.href);
  return url.origin === window.location.origin && dashboardPaths.has(url.pathname.replace(/\/+$/, '') || '/')
    ? url : null;
}

export function navigateDashboard(event: MouseEvent<HTMLAnchorElement>) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey
    || event.currentTarget.target || event.currentTarget.hasAttribute('download') || document.querySelector('[data-unsaved="true"]')) return;
  const destination = dashboardDestination(event.currentTarget.href);
  if (!destination || destination.hash) return;
  event.preventDefault();
  window.history.pushState(null, '', destination);
  window.dispatchEvent(new PopStateEvent('popstate'));
  window.scrollTo(0, 0);
}

export function subscribeNavigation(
  listener: () => void,
  confirmDiscard = () => window.confirm(translate(readInitialLanguage(), 'Ungespeicherte Änderungen verwerfen?')),
) {
  let currentUrl = window.location.href;
  const onNavigate = () => {
    const unload = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(unload);
    const saving = document.querySelector('[data-saving="true"]');
    const dirty = document.querySelector('[data-unsaved="true"]') || unload.defaultPrevented;
    if (saving || (dirty && !confirmDiscard())) {
      window.history.pushState(null, '', currentUrl);
      return;
    }
    currentUrl = window.location.href;
    listener();
  };
  window.addEventListener('popstate', onNavigate);
  return () => window.removeEventListener('popstate', onNavigate);
}

export function navigationSnapshot() {
  return window.location.pathname + window.location.search;
}

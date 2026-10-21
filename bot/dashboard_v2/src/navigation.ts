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
import { isPublicTourPath, livePathFromTourPath, tourHref } from './publicTour';

const dashboardPaths = new Set([
  PREVIEW_ANALYTICS_ROUTE, PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE, PREVIEW_SOCIAL_ROUTE, PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE, PREVIEW_VERWALTUNG_ROUTE,
  '/twitch/analyse', '/twitch/dashboard-v2', '/dashboard-v2',
  '/twitch/challenges', '/twitch/hilfe', '/twitch/feedback',
  '/twitch/social-media', '/twitch/kategorie', '/twitch/onboarding',
]);

function navigationIndex(): number | null {
  const index = window.navigation?.currentEntry?.index;
  return index !== undefined && index >= 0 ? index : null;
}

function isDashboardPath(pathname: string): boolean {
  const normalized = pathname.replace(/\/+$/, '') || '/';
  return dashboardPaths.has(normalized) || isPublicTourPath(normalized);
}

export function dashboardDestination(href: string): URL | null {
  const url = new URL(href, window.location.href);
  return url.origin === window.location.origin && isDashboardPath(url.pathname)
    ? url : null;
}

export function tourLinkHref(href: string, currentHref: string): string | null {
  const current = new URL(currentHref);
  if (!isPublicTourPath(current.pathname)) return null;
  const url = new URL(href, current);
  const livePath = url.pathname.replace(/\/+$/, '') || '/';
  if (url.origin !== current.origin || isPublicTourPath(livePath) || !dashboardPaths.has(livePath)) return null;
  const next = new URL(tourHref(`${url.pathname}${url.search}${url.hash}`, current.pathname), current.origin);
  return livePathFromTourPath(next.pathname) === livePath ? next.href : null;
}

export function navigateDashboard(event: MouseEvent<HTMLAnchorElement>) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey
    || event.currentTarget.target || event.currentTarget.hasAttribute('download') || document.querySelector('[data-unsaved="true"]')) return;
  const destination = dashboardDestination(event.currentTarget.href);
  if (!destination || destination.hash || navigationIndex() === null) return;
  event.preventDefault();
  const unload = new Event('beforeunload', { cancelable: true });
  window.dispatchEvent(unload);
  if (document.querySelector('[data-saving="true"]') || (unload.defaultPrevented && !window.confirm(translate(readInitialLanguage(), 'Ungespeicherte Änderungen verwerfen?')))) return;
  window.history.pushState(null, '', destination);
  window.dispatchEvent(new Event('dashboard:navigate'));
  window.scrollTo(0, 0);
}

export function subscribeNavigation(
  listener: () => void,
  confirmDiscard = () => window.confirm(translate(readInitialLanguage(), 'Ungespeicherte Änderungen verwerfen?')),
) {
  const initialIndex = navigationIndex();
  if (initialIndex === null) return () => undefined;
  let currentIndex = initialIndex;
  let restoring = false;
  const onNavigate = () => {
    const nextIndex = navigationIndex();
    if (nextIndex === null) return;
    if (restoring) {
      if (nextIndex === currentIndex) restoring = false;
      return;
    }
    const unload = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(unload);
    const saving = document.querySelector('[data-saving="true"]');
    const dirty = document.querySelector('[data-unsaved="true"]') || unload.defaultPrevented;
    if (saving || (dirty && !confirmDiscard())) {
      restoring = true;
      window.history.go(currentIndex - nextIndex);
      return;
    }
    currentIndex = nextIndex;
    listener();
  };
  const onPush = () => {
    currentIndex = navigationIndex() ?? currentIndex;
    listener();
  };
  window.addEventListener('dashboard:navigate', onPush);
  window.addEventListener('popstate', onNavigate);
  return () => {
    window.removeEventListener('dashboard:navigate', onPush);
    window.removeEventListener('popstate', onNavigate);
  };
}

export function navigationSnapshot() {
  return window.location.pathname + window.location.search;
}

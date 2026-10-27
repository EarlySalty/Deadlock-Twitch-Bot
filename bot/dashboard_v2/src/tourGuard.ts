import { getPreviewApiFixture, getPreviewPathFixture } from './preview/fixtures';
import { DEMO_API_BASE, LIVE_API_BASE } from './runtimeConfig';
import { isAnalyticsDemoPath, isPublicTourPath } from './publicTour';
import { tourLinkHref } from './navigation';

export const TOUR_WRITE_BLOCKED_MESSAGE = 'In der Demo kannst du nichts ändern.';
export const TOUR_UNAVAILABLE_MESSAGE = 'In der Demo nicht verfügbar.';
export const TOUR_BLOCKED_EVENT = 'rundgang:blocked';

const READ_METHODS = new Set(['GET', 'HEAD']);
const STATIC_PREFIXES = ['/twitch/dashboard-v2/', '/twitch/rundgang/dashboard-v2/', '/twitch/demo/dashboard-v2/'];
const PUBLIC_EXIT_PATHS = new Set(['/twitch/faq', '/faq', '/docs']);
const COMMUNITY_HOST = 'deutsche-deadlock-community.de';

type FetchFn = typeof fetch;

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json; charset=utf-8' },
  });
}

export function tourWriteBlockedResponse(): Response {
  return jsonResponse(403, { error: TOUR_WRITE_BLOCKED_MESSAGE, message: TOUR_WRITE_BLOCKED_MESSAGE });
}

function unavailableResponse(): Response {
  return jsonResponse(404, { error: TOUR_UNAVAILABLE_MESSAGE, message: TOUR_UNAVAILABLE_MESSAGE });
}

function isUnder(pathname: string, base: string): boolean {
  return pathname === base || pathname.startsWith(`${base}/`);
}

export function tourFixtureFor(pathname: string): unknown | undefined {
  const pathFixture = getPreviewPathFixture(pathname);
  if (pathFixture !== undefined) return structuredClone(pathFixture);
  for (const base of [DEMO_API_BASE, LIVE_API_BASE]) {
    if (isUnder(pathname, base)) {
      const fixture = getPreviewApiFixture(pathname.slice(base.length) || '/');
      if (fixture !== undefined) return structuredClone(fixture);
    }
  }
  return undefined;
}

function requestMethod(input: RequestInfo | URL, init?: RequestInit): string {
  const fromRequest = typeof Request !== 'undefined' && input instanceof Request ? input.method : undefined;
  return (init?.method ?? fromRequest ?? 'GET').toUpperCase();
}

function requestUrl(input: RequestInfo | URL, origin: string): URL {
  if (typeof Request !== 'undefined' && input instanceof Request) return new URL(input.url, origin);
  return new URL(input instanceof URL ? input.href : String(input), origin);
}

function readInit(input: RequestInfo | URL, init: RequestInit | undefined, method: string): RequestInit {
  const request = typeof Request !== 'undefined' && input instanceof Request ? input : null;
  return {
    method,
    headers: init?.headers ?? request?.headers,
    signal: init?.signal ?? request?.signal,
    cache: init?.cache ?? request?.cache,
    credentials: 'omit',
  };
}

export function createTourFetch(realFetch: FetchFn, origin: string, onBlocked: () => void = () => {}): FetchFn {
  return async (input, init) => {
    const method = requestMethod(input, init);
    if (!READ_METHODS.has(method)) {
      onBlocked();
      return tourWriteBlockedResponse();
    }
    const url = requestUrl(input, origin);
    const outgoing = readInit(input, init, method);
    if (url.origin !== origin) return realFetch(url.href, outgoing);
    if (STATIC_PREFIXES.some(prefix => url.pathname.startsWith(prefix))) return realFetch(url.href, outgoing);
    const fixture = tourFixtureFor(url.pathname);
    if (fixture !== undefined) return jsonResponse(200, fixture);
    if (isUnder(url.pathname, DEMO_API_BASE)) return realFetch(url.href, outgoing);
    if (isUnder(url.pathname, LIVE_API_BASE)) {
      const demoUrl = new URL(`${DEMO_API_BASE}${url.pathname.slice(LIVE_API_BASE.length)}${url.search}`, origin);
      return realFetch(demoUrl.href, outgoing);
    }
    return unavailableResponse();
  };
}

export type TourNavigation = { kind: 'allow' } | { kind: 'block' } | { kind: 'rewrite'; href: string };

function isCommunityHost(hostname: string): boolean {
  return hostname === COMMUNITY_HOST || hostname.endsWith(`.${COMMUNITY_HOST}`);
}

export function tourNavigation(destination: string, currentHref: string): TourNavigation {
  const current = new URL(currentHref);
  const url = new URL(destination, current);
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return { kind: 'block' };
  if (url.origin !== current.origin) {
    const authLike = /oauth|authorize|\/auth\//i.test(url.pathname) || url.hostname === 'id.twitch.tv';
    return isCommunityHost(url.hostname) || authLike ? { kind: 'block' } : { kind: 'allow' };
  }
  const rewrite = tourLinkHref(url.href, current.href);
  if (rewrite) return { kind: 'rewrite', href: rewrite };
  const path = url.pathname.replace(/\/+$/, '') || '/';
  if (isPublicTourPath(path) || isAnalyticsDemoPath(path) || PUBLIC_EXIT_PATHS.has(path)) return { kind: 'allow' };
  return { kind: 'block' };
}

function announceBlocked(win: Window) {
  win.dispatchEvent(new CustomEvent(TOUR_BLOCKED_EVENT, { detail: TOUR_WRITE_BLOCKED_MESSAGE }));
}

function guardClicks(win: Window, exits: Set<string>) {
  const onClick = (event: MouseEvent) => {
    const anchor = event.target instanceof Element ? event.target.closest('a[href]') : null;
    if (!(anchor instanceof HTMLAnchorElement)) return;
    if (anchor.hasAttribute('data-tour-exit')) {
      exits.add(anchor.href);
      return;
    }
    const decision = tourNavigation(anchor.href, win.location.href);
    if (decision.kind === 'rewrite') anchor.href = decision.href;
    if (decision.kind === 'block') {
      event.preventDefault();
      event.stopImmediatePropagation();
      announceBlocked(win);
    }
  };
  win.document.addEventListener('click', onClick, true);
  win.document.addEventListener('auxclick', onClick, true);
}

function guardSubmits(win: Window) {
  win.addEventListener('submit', event => {
    if (event.defaultPrevented) return;
    event.preventDefault();
    announceBlocked(win);
  });
}

function guardScriptNavigation(win: Window, exits: Set<string>) {
  const navigation = win.navigation;
  if (!navigation) return;
  navigation.addEventListener('navigate', event => {
    if (!event.cancelable || event.navigationType === 'traverse' || event.navigationType === 'reload') return;
    const destination = event.destination.url;
    if (exits.delete(destination)) return;
    const decision = tourNavigation(destination, win.location.href);
    if (decision.kind === 'allow') return;
    event.preventDefault();
    if (decision.kind === 'rewrite') win.location.assign(decision.href);
    else announceBlocked(win);
  });
}

export function installTourGuard(win: Window) {
  const realFetch = win.fetch.bind(win);
  win.fetch = createTourFetch(realFetch, win.location.origin, () => announceBlocked(win));
  win.navigator.sendBeacon = () => false;
  const exits = new Set<string>();
  guardClicks(win, exits);
  guardSubmits(win);
  guardScriptNavigation(win, exits);
}

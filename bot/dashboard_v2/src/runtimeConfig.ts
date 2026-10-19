import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  isPreviewModeEnabled,
} from './preview/routes';

export interface DashboardRuntimeConfig {
  apiBase: string;
  demoMode: boolean;
  allowedDemoProfiles: string[];
  defaultDemoProfile: string | null;
}

export const LIVE_API_BASE = '/twitch/api/v2';
export const DEMO_API_BASE = '/twitch/demo/api/v2';

const ALLOWED_API_BASES = new Set<string>([LIVE_API_BASE, DEMO_API_BASE]);

declare global {
  interface Window {
    __TWITCH_DASHBOARD_RUNTIME__?: Partial<DashboardRuntimeConfig>;
  }
}

const DEFAULT_CONFIG: DashboardRuntimeConfig = {
  apiBase: LIVE_API_BASE,
  demoMode: false,
  allowedDemoProfiles: [],
  defaultDemoProfile: null,
};

const LOCAL_PREVIEW_ALLOWED_PROFILES = ['smallcore_focus', 'midcore_live', 'largecore_peak'];
const LOCAL_PREVIEW_DEFAULT_PROFILE = 'midcore_live';

function isLocalPreviewRuntime(): boolean {
  return isPreviewModeEnabled();
}

function sanitizeApiBase(candidate: unknown): string {
  const value = typeof candidate === 'string' ? candidate.trim() : '';
  if (!ALLOWED_API_BASES.has(value)) {
    return DEFAULT_CONFIG.apiBase;
  }
  return value;
}

function sanitizeProfiles(candidate: unknown): string[] {
  if (!Array.isArray(candidate)) return [];
  return candidate
    .map((entry) => (typeof entry === 'string' ? entry.trim().toLowerCase() : ''))
    .filter((entry) => entry.length > 0);
}

function readRuntimeConfig(): DashboardRuntimeConfig {
  if (isLocalPreviewRuntime()) {
    return {
      apiBase: DEMO_API_BASE,
      demoMode: true,
      allowedDemoProfiles: LOCAL_PREVIEW_ALLOWED_PROFILES,
      defaultDemoProfile: LOCAL_PREVIEW_DEFAULT_PROFILE,
    };
  }

  const raw = window.__TWITCH_DASHBOARD_RUNTIME__ ?? {};

  // When visited at a demo URL, force demo settings regardless of whether the
  // injected runtime config was readable. The global Caddy CSP (script-src
  // 'self') blocks inline scripts, so window.__TWITCH_DASHBOARD_RUNTIME__ may
  // be undefined even though the backend injected it. Without this fallback the
  // SPA defaults to the live API base, gets a 401, and redirects to Twitch login.
  // isDemoDashboardPath is a function declaration and therefore hoisted.
  const onDemoPath = isDemoDashboardPath(window.location?.pathname ?? '');

  const allowedDemoProfiles = sanitizeProfiles(raw.allowedDemoProfiles);
  const defaultDemoProfileRaw =
    typeof raw.defaultDemoProfile === 'string' ? raw.defaultDemoProfile.trim().toLowerCase() : '';

  return {
    apiBase: onDemoPath ? DEMO_API_BASE : sanitizeApiBase(raw.apiBase),
    demoMode: onDemoPath || raw.demoMode === true,
    allowedDemoProfiles,
    defaultDemoProfile:
      defaultDemoProfileRaw && allowedDemoProfiles.includes(defaultDemoProfileRaw)
        ? defaultDemoProfileRaw
        : null,
  };
}

export const dashboardRuntimeConfig = Object.freeze(readRuntimeConfig());

function normalizeDashboardPath(pathname: string): string {
  return pathname.replace(/\/+$/, '') || '/';
}

export function isAnalyticsDemoPath(pathname: string): boolean {
  const normalized = normalizeDashboardPath(pathname);
  return (
    normalized === '/twitch/demo' ||
    normalized.startsWith('/twitch/demo/') ||
    normalized === '/demo/twitch/demo' ||
    normalized.startsWith('/demo/twitch/demo/')
  );
}

export function isPublicTourPath(pathname: string): boolean {
  const normalized = normalizeDashboardPath(pathname);
  return (
    normalized === '/twitch/rundgang' ||
    normalized.startsWith('/twitch/rundgang/') ||
    normalized === '/demo/twitch/rundgang' ||
    normalized.startsWith('/demo/twitch/rundgang/')
  );
}

export function tourBasePath(pathname: string): string {
  const normalized = normalizeDashboardPath(pathname);
  if (normalized === '/demo/twitch/rundgang' || normalized.startsWith('/demo/twitch/rundgang/')) {
    return '/demo/twitch/rundgang';
  }
  return '/twitch/rundgang';
}

export function livePathFromTourPath(pathname: string): string | null {
  const normalized = normalizeDashboardPath(pathname);
  for (const prefix of ['/demo/twitch/rundgang', '/twitch/rundgang'] as const) {
    if (normalized === prefix) {
      return '/twitch/dashboard';
    }
    if (normalized.startsWith(`${prefix}/`)) {
      const rest = normalized.slice(prefix.length);
      if (rest.startsWith('/dashboard-v2')) {
        return null;
      }
      if (rest === '/dashboard') {
        return '/twitch/dashboard';
      }
      return `/twitch${rest}`;
    }
  }
  return null;
}

export function tourHref(livePath: string, pathname: string): string {
  if (!isPublicTourPath(pathname)) {
    return livePath;
  }
  const url = new URL(livePath, 'https://deutsche-deadlock-community.de');
  const live = normalizeDashboardPath(url.pathname);
  let rest = '';
  if (live === '/twitch/dashboard' || live === '/twitch') {
    rest = '';
  } else if (live.startsWith('/twitch/')) {
    rest = live.slice('/twitch'.length);
  } else {
    rest = live;
  }
  const href = rest ? `${tourBasePath(pathname)}${rest}` : tourBasePath(pathname);
  return `${href}${url.search}${url.hash}`;
}

export function isDemoDashboardPath(pathname: string): boolean {
  const normalized = normalizeDashboardPath(pathname);
  if (isPreviewModeEnabled()) {
    return (
      normalized === PREVIEW_ANALYTICS_ROUTE ||
      normalized === PREVIEW_HOME_ROUTE ||
      normalized === PREVIEW_VERWALTUNG_ROUTE ||
      normalized === PREVIEW_PRICING_ROUTE
    );
  }
  return isAnalyticsDemoPath(normalized) || isPublicTourPath(normalized);
}

export function isPublicTourMode(pathname: string = typeof window === 'undefined' ? '' : window.location?.pathname ?? ''): boolean {
  return isPublicTourPath(pathname);
}

export function hasDemoRuntimeConfig(
  config: DashboardRuntimeConfig = dashboardRuntimeConfig
): boolean {
  return config.apiBase === DEMO_API_BASE && config.demoMode === true;
}

export function resolveEffectiveDemoMode({
  pathname,
  runtimeConfig = dashboardRuntimeConfig,
}: {
  pathname: string;
  runtimeConfig?: DashboardRuntimeConfig;
}): boolean {
  return isDemoDashboardPath(pathname) && hasDemoRuntimeConfig(runtimeConfig);
}

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
  const raw = window.__TWITCH_DASHBOARD_RUNTIME__ ?? {};

  // When visited at a demo URL, force demo settings regardless of whether the
  // injected runtime config was readable. The global Caddy CSP (script-src
  // 'self') blocks inline scripts, so window.__TWITCH_DASHBOARD_RUNTIME__ may
  // be undefined even though the backend injected it. Without this fallback the
  // SPA defaults to the live API base, gets a 401, and redirects to Twitch login.
  // isDemoDashboardPath is a function declaration and therefore hoisted.
  const onDemoPath = isDemoDashboardPath(window.location.pathname);

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

export function isDemoDashboardPath(pathname: string): boolean {
  const normalized = pathname.replace(/\/+$/, '') || '/';
  return (
    normalized === '/twitch/demo' ||
    normalized.startsWith('/twitch/demo/') ||
    normalized === '/demo/twitch/demo' ||
    normalized.startsWith('/demo/twitch/demo/')
  );
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

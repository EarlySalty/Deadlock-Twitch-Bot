import { dashboardRuntimeConfig } from '../runtimeConfig';
import { getPreviewApiFixture } from '../preview/fixtures';
import {
  PREVIEW_HOME_ROUTE,
  isPreviewLocalhost,
  isPreviewModeEnabled,
} from '../preview/routes';

const API_BASE = dashboardRuntimeConfig.apiBase;
const INTERNAL_REDIRECT_PREFIX = '/twitch';
const LIVE_LOGIN_FALLBACK = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2';

export const DASHBOARD_V2_LOGIN_FALLBACK = isPreviewModeEnabled()
  ? PREVIEW_HOME_ROUTE
  : LIVE_LOGIN_FALLBACK;
const COOKIE_FETCH_CREDENTIALS: RequestCredentials = 'same-origin';

interface ApiErrorPayload {
  error?: string;
  message?: string;
  loginUrl?: string;
}

export interface FetchJsonOptions {
  loginFallback?: string;
  notFoundMessage?: string;
}

function isAllowedInternalRedirectPath(pathname: string): boolean {
  return pathname === INTERNAL_REDIRECT_PREFIX || pathname.startsWith(`${INTERNAL_REDIRECT_PREFIX}/`);
}

export function sanitizeInternalRedirectUrl(rawUrl: string | null | undefined, fallback: string): string {
  if (isPreviewLocalhost()) {
    return fallback || PREVIEW_HOME_ROUTE;
  }

  const fallbackCandidate = (fallback || '').trim();
  let safeFallback = DASHBOARD_V2_LOGIN_FALLBACK;
  if (fallbackCandidate && fallbackCandidate.startsWith('/') && !fallbackCandidate.startsWith('//')) {
    try {
      const parsedFallback = new URL(fallbackCandidate, window.location.origin);
      if (
        parsedFallback.origin === window.location.origin &&
        isAllowedInternalRedirectPath(parsedFallback.pathname)
      ) {
        safeFallback = `${parsedFallback.pathname}${parsedFallback.search}${parsedFallback.hash}`;
      }
    } catch {
      safeFallback = DASHBOARD_V2_LOGIN_FALLBACK;
    }
  }

  const candidate = (rawUrl || '').trim();
  if (!candidate) {
    return safeFallback;
  }

  if (!candidate.startsWith('/') || candidate.startsWith('//') || candidate.includes('\\')) {
    return safeFallback;
  }

  try {
    const parsed = new URL(candidate, window.location.origin);
    if (parsed.origin !== window.location.origin) {
      return safeFallback;
    }
    const normalized = `${parsed.pathname}${parsed.search}${parsed.hash}`;
    if (!isAllowedInternalRedirectPath(parsed.pathname)) {
      return safeFallback;
    }
    return normalized;
  } catch {
    return safeFallback;
  }
}

export function withCookieCredentials(init: RequestInit = {}): RequestInit {
  return {
    credentials: COOKIE_FETCH_CREDENTIALS,
    ...init,
  };
}

export function buildApiUrl(
  endpoint: string,
  params: Record<string, string | number | boolean> = {}
): string {
  const url = new URL(`${API_BASE}${endpoint}`, window.location.origin);
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== null) {
      url.searchParams.set(key, String(value));
    }
  });
  return url.toString();
}

export function getBrowserTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

async function readErrorPayload(response: Response): Promise<ApiErrorPayload | null> {
  return response.json().catch(() => null) as Promise<ApiErrorPayload | null>;
}

async function handleUnauthorizedResponse(
  response: Response,
  loginFallback: string
): Promise<never> {
  const unauthorized = await readErrorPayload(response);
  if (unauthorized?.loginUrl) {
    window.location.href = sanitizeInternalRedirectUrl(unauthorized.loginUrl, loginFallback);
    throw new Error('Redirecting to Twitch login');
  }
  throw new Error(unauthorized?.error || 'Unauthorized');
}

export async function fetchJson<T>(
  input: RequestInfo | URL,
  init: RequestInit = {},
  options: FetchJsonOptions = {}
): Promise<T> {
  const response = await fetch(input, init);

  if (response.status === 401) {
    await handleUnauthorizedResponse(response, options.loginFallback || DASHBOARD_V2_LOGIN_FALLBACK);
  }

  if (options.notFoundMessage && response.status === 404) {
    throw new Error(options.notFoundMessage);
  }

  if (!response.ok) {
    const error = await readErrorPayload(response);
    throw new Error(error?.message || error?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return response.json() as Promise<T>;
}

export async function fetchApi<T>(
  endpoint: string,
  params: Record<string, string | number | boolean> = {},
  timeoutMs?: number
): Promise<T> {
  if (isPreviewLocalhost()) {
    const fixture = getPreviewApiFixture(endpoint, params);
    if (fixture !== undefined) {
      return structuredClone(fixture) as T;
    }
  }

  const url = buildApiUrl(endpoint, params);
  const abortCtrl = timeoutMs ? new AbortController() : null;
  const timer = abortCtrl ? setTimeout(() => abortCtrl.abort(), timeoutMs!) : null;

  try {
    return await fetchJson<T>(
      url,
      withCookieCredentials({
        headers: { Accept: 'application/json' },
        signal: abortCtrl?.signal,
      })
    );
  } finally {
    if (timer) clearTimeout(timer);
  }
}

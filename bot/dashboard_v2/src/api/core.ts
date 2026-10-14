import { dashboardRuntimeConfig } from '../runtimeConfig';
import { getPreviewApiFixture, getPreviewPathFixture } from '../preview/fixtures';
import {
  PREVIEW_HOME_ROUTE,
  isPreviewLocalhost,
  isPreviewModeEnabled,
} from '../preview/routes';
import { ApiHttpError } from './httpError';
import { messageFromText } from '../i18n/errorMessage';
import { withLanguageHeader } from './languageHeader';

const API_BASE = dashboardRuntimeConfig.apiBase;
const INTERNAL_REDIRECT_PREFIX = '/twitch';
const LIVE_LOGIN_FALLBACK = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard';

export const DASHBOARD_V2_LOGIN_FALLBACK = isPreviewModeEnabled()
  ? PREVIEW_HOME_ROUTE
  : LIVE_LOGIN_FALLBACK;
const COOKIE_FETCH_CREDENTIALS: RequestCredentials = 'same-origin';

interface ApiErrorPayload {
  error?: string;
  message?: string;
  code?: string;
  loginUrl?: string;
}

export { ApiHttpError } from './httpError';
export { LANGUAGE_HEADER, withLanguageHeader } from './languageHeader';

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
  const raw = await response.text().catch(() => '');
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== 'object') return { message: raw };
    const payload = value as Record<string, unknown>;
    return {
      error: typeof payload.error === 'string' ? payload.error : undefined,
      message: typeof payload.message === 'string' ? payload.message : undefined,
      code: typeof payload.code === 'string' ? payload.code : undefined,
      loginUrl: typeof payload.loginUrl === 'string' ? payload.loginUrl : undefined,
    };
  } catch {
    return { message: raw };
  }
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
  const message = unauthorized?.error || unauthorized?.message || 'Unauthorized';
  throw new ApiHttpError(message, response.status, messageFromText(message), unauthorized ?? undefined);
}

export async function fetchJson<T>(
  input: RequestInfo | URL,
  init: RequestInit = {},
  options: FetchJsonOptions = {}
): Promise<T> {
  if (isPreviewLocalhost() && (init.method ?? 'GET').toUpperCase() === 'GET') {
    const pfad = typeof input === 'string' ? input : input.toString();
    const fixture = getPreviewPathFixture(pfad.split('?')[0]);
    if (fixture !== undefined) {
      return structuredClone(fixture) as T;
    }
  }

  const response = await fetch(input, withLanguageHeader(input, init));

  if (response.status === 401) {
    await handleUnauthorizedResponse(response, options.loginFallback || DASHBOARD_V2_LOGIN_FALLBACK);
  }

  if (options.notFoundMessage && response.status === 404) {
    throw new ApiHttpError(options.notFoundMessage, response.status, messageFromText(options.notFoundMessage));
  }

  if (!response.ok) {
    const error = await readErrorPayload(response);
    const message = error?.message || error?.error;
    throw new ApiHttpError(
      message || `Server-Fehler (HTTP ${response.status})`,
      response.status,
      message ? messageFromText(message) : { key: 'Server-Fehler (HTTP {status})', params: { status: response.status } },
      error ?? undefined,
    );
  }

  return response.json() as Promise<T>;
}

export async function fetchApi<T>(
  endpoint: string,
  params: Record<string, string | number | boolean> = {},
  timeoutMs?: number,
  signal?: AbortSignal
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
  const forwardAbort = () => abortCtrl?.abort(signal?.reason);
  if (abortCtrl && signal) {
    if (signal.aborted) forwardAbort();
    else signal.addEventListener('abort', forwardAbort, { once: true });
  }

  try {
    return await fetchJson<T>(
      url,
      withCookieCredentials({
        headers: { Accept: 'application/json' },
        signal: abortCtrl?.signal ?? signal,
      })
    );
  } finally {
    if (timer) clearTimeout(timer);
    if (abortCtrl && signal) signal.removeEventListener('abort', forwardAbort);
  }
}

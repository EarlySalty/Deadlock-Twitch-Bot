import { getActiveLanguage } from '../i18n/dictionary';

export const LANGUAGE_HEADER = 'X-Dashboard-Language';

export function withLanguageHeader(input: RequestInfo | URL, init: RequestInit = {}): RequestInit {
  if (typeof Request !== 'undefined' && input instanceof Request) return init;
  try {
    if (new URL(String(input), window.location.origin).origin !== window.location.origin) return init;
  } catch {
    return init;
  }
  const headers = new Headers(init.headers);
  headers.set(LANGUAGE_HEADER, getActiveLanguage());
  return { ...init, headers };
}

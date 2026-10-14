import { isEnglishDashboardHost, readInitialLanguage, type Language } from './dictionary';

export const MAIN_DASHBOARD_ORIGIN = 'https://deutsche-deadlock-community.de';

export function mainDomainOAuthUrl(rawUrl: string, language: Language = readInitialLanguage()): string {
  if (!isEnglishDashboardHost()) return rawUrl;
  try {
    const url = new URL(rawUrl, window.location.origin);
    if (url.origin !== window.location.origin && url.origin !== MAIN_DASHBOARD_ORIGIN) return rawUrl;
    url.protocol = 'https:';
    url.host = new URL(MAIN_DASHBOARD_ORIGIN).host;
    if (!url.searchParams.has('lang')) url.searchParams.set('lang', language);
    if (url.pathname === '/twitch/auth/login' || url.pathname === '/twitch/auth/discord/link') {
      const fallback = url.pathname === '/twitch/auth/discord/link' ? '/twitch/verwaltung' : '/twitch/dashboard';
      const candidate = url.searchParams.get('next') ?? fallback;
      const next = new URL(candidate, MAIN_DASHBOARD_ORIGIN);
      if (next.origin === MAIN_DASHBOARD_ORIGIN && candidate.startsWith('/') && !candidate.startsWith('//') && !candidate.includes('\\')) {
        if (!next.searchParams.has('lang')) next.searchParams.set('lang', language);
        url.searchParams.set('next', `${next.pathname}${next.search}${next.hash}`);
      } else {
        url.searchParams.set('next', `${fallback}?lang=${language}`);
      }
    }
    return url.toString();
  } catch {
    return rawUrl;
  }
}

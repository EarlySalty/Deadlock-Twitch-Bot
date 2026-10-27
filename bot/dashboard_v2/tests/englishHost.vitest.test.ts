import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  LANGUAGE_STORAGE_KEY,
  LEGACY_LANGUAGE_STORAGE_KEY,
  readInitialLanguage,
  storeLanguage,
} from '../src/i18n/dictionary';
import { MAIN_DASHBOARD_ORIGIN, mainDomainOAuthUrl } from '../src/i18n/oauthLinks';

function browser(host = 'dead-lock.de', search = '', saved: string | null = null, browserLocale: string | string[] = 'de-DE') {
  const values = new Map<string, string>();
  if (saved !== null) values.set(LANGUAGE_STORAGE_KEY, saved);
  const location = new URL(`https://${host}/twitch/dashboard${search}`);
  vi.stubGlobal('window', {
    location,
    navigator: { languages: Array.isArray(browserLocale) ? browserLocale : [browserLocale] },
    localStorage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    },
  });
  return values;
}

afterEach(() => vi.unstubAllGlobals());

describe('dashboard language resolution', () => {
  it.each(['dead-lock.de', 'www.dead-lock.de'])('defaults %s to English even with a German browser', (host) => {
    browser(host);
    expect(readInitialLanguage()).toBe('en');
  });

  it.each(['de', 'en'])('query %s overrides a saved choice', (language) => {
    const values = browser('dead-lock.de', `?lang=${language}`, language === 'de' ? 'en' : 'de');
    const resolved = readInitialLanguage();
    expect(resolved).toBe(language);
    storeLanguage(resolved);
    expect(values.get(LANGUAGE_STORAGE_KEY)).toBe(language);
  });

  it('uses a saved German choice before the English host default', () => {
    browser('dead-lock.de', '', 'de');
    expect(readInitialLanguage()).toBe('de');
  });

  it('ignores an invalid query and saved value', () => {
    browser('www.dead-lock.de', '?lang=fr', 'fr');
    expect(readInitialLanguage()).toBe('en');
  });

  it.each([['de-DE', 'de'], ['en-US', 'en']])('keeps main-domain browser selection for %s', (locale, expected) => {
    browser('deutsche-deadlock-community.de', '', null, locale);
    expect(readInitialLanguage()).toBe(expected);
  });

  it('detects German anywhere in the browser language list', () => {
    browser('deutsche-deadlock-community.de', '', null, ['en-US', 'de-DE']);
    expect(readInitialLanguage()).toBe('de');
  });

  it.each(['en', 'de'])('resets a legacy saved %s choice to German', (legacy) => {
    const values = browser('deutsche-deadlock-community.de', '', null, 'en-US');
    values.set(LEGACY_LANGUAGE_STORAGE_KEY, legacy);
    expect(readInitialLanguage()).toBe('de');
  });

  it('keeps the English host default for a legacy saved choice', () => {
    const values = browser('dead-lock.de');
    values.set(LEGACY_LANGUAGE_STORAGE_KEY, 'de');
    expect(readInitialLanguage()).toBe('en');
  });

  it('accepts a main-domain query hint before saved German', () => {
    browser('deutsche-deadlock-community.de', '?lang=en', 'de');
    expect(readInitialLanguage()).toBe('en');
  });

  it('does not mistake another domain for the English host', () => {
    browser('dead-lock.de.example.com');
    expect(readInitialLanguage()).toBe('de');
  });

  it('survives unavailable storage', () => {
    browser();
    window.localStorage.getItem = () => { throw new Error('blocked'); };
    window.localStorage.setItem = () => { throw new Error('blocked'); };
    expect(readInitialLanguage()).toBe('en');
    expect(() => storeLanguage('en')).not.toThrow();
  });

  it('keeps server rendering German', () => {
    vi.stubGlobal('window', undefined);
    expect(readInitialLanguage()).toBe('de');
  });
});

describe('main-domain OAuth navigation', () => {
  it('carries English through the existing login next parameter', () => {
    browser();
    const url = new URL(mainDomainOAuthUrl('/twitch/auth/login?next=%2Ftwitch%2Fdashboard%3Fstreamer%3D42'));
    expect(url.origin).toBe(MAIN_DASHBOARD_ORIGIN);
    expect(url.searchParams.get('lang')).toBe('en');
    expect(url.searchParams.get('next')).toBe('/twitch/dashboard?streamer=42&lang=en');
  });

  it('carries English and the account fragment through Discord next metadata', () => {
    browser();
    const url = new URL(mainDomainOAuthUrl('/twitch/auth/discord/link?next=%2Ftwitch%2Fverwaltung%23konto'));
    expect(url.origin).toBe(MAIN_DASHBOARD_ORIGIN);
    expect(url.searchParams.get('next')).toBe('/twitch/verwaltung?lang=en#konto');
    expect(new URL(mainDomainOAuthUrl('/twitch/auth/discord/link')).searchParams.get('next')).toBe('/twitch/verwaltung?lang=en');
  });

  it.each(['/twitch/raid/auth?scope_profile=title', '/twitch/social-media/oauth/start/youtube', '/link/steam/login?uid=synthetic'])('rebases connect target %s', (path) => {
    browser('www.dead-lock.de');
    const url = new URL(mainDomainOAuthUrl(path));
    expect(url.origin).toBe(MAIN_DASHBOARD_ORIGIN);
    expect(url.pathname).toBe(path.split('?')[0]);
    expect(url.searchParams.get('lang')).toBe('en');
  });

  it('retains explicit hints and a user-selected German choice', () => {
    browser();
    expect(new URL(mainDomainOAuthUrl('/twitch/raid/auth?lang=de')).searchParams.get('lang')).toBe('de');
    expect(new URL(mainDomainOAuthUrl('/twitch/auth/login', 'de')).searchParams.get('next')).toBe('/twitch/dashboard?lang=de');
  });

  it.each(['//outside.example/path', 'https://outside.example/path', '/\\outside.example/path'])('does not allow an external login next target %s', (next) => {
    browser();
    const url = new URL(mainDomainOAuthUrl(`/twitch/auth/login?next=${encodeURIComponent(next)}`));
    expect(url.searchParams.get('next')).toBe('/twitch/dashboard?lang=en');
  });

  it('keeps provider URLs and main-domain links unchanged', () => {
    browser();
    const provider = 'https://id.twitch.tv/oauth2/authorize?redirect_uri=unchanged';
    expect(mainDomainOAuthUrl(provider)).toBe(provider);
    browser('deutsche-deadlock-community.de');
    const login = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard';
    expect(mainDomainOAuthUrl(login)).toBe(login);
  });
});

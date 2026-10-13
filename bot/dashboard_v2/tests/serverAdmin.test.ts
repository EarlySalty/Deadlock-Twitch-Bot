/// <reference types="node" />
import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  ServerAdminLoginRequired,
  errorText,
  isSnowflakeOrEmpty,
  patchGuildSettings,
  setStreamerActive,
} from '../src/api/serverAdmin';
import { translate } from '../src/i18n/dictionary';
import { ApiHttpError } from '../src/api/httpError';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');
const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
const PAGE = read('pages/ServerAdmin.tsx');
const SIDEBAR = read('components/layout/ServerAdminSidebar.tsx');
const API = read('api/serverAdmin.ts');
const APP = read('App.tsx');
const SHELL = read('components/layout/DashboardShell.tsx');

const originalFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = originalFetch;
});

function mockFetch(status: number, body: unknown) {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return { ok: status >= 200 && status < 300, status, json: async () => body };
  }) as unknown as typeof fetch;
  return calls;
}

test('Server-Admin-Panel kennt den Admin-Modus und die Partner-Sidebar nicht', () => {
  for (const [name, src] of [
    ['ServerAdmin.tsx', code(PAGE)],
    ['ServerAdminSidebar.tsx', code(SIDEBAR)],
    ['serverAdmin.ts', code(API)],
  ] as const) {
    assert.doesNotMatch(src, /adminMode|Admin-Modus|useDashboardProfile|isAdmin\b/, `${name} darf den Admin-Modus nicht anfassen`);
    assert.doesNotMatch(src, /from '@\/components\/layout\/DashboardSidebar'/, `${name} darf die Partner-Sidebar nicht importieren`);
    assert.doesNotMatch(src, /from '@\/api\/(admin|auth|core|client)'/, `${name} darf keine Partner-/Admin-APIs nutzen`);
    assert.doesNotMatch(src, /\/twitch\/api\/v2\/|\/twitch\/admin/, `${name} darf keine Partner-/Admin-Endpunkte aufrufen`);
  }
  assert.match(API, /const API = '\/twitch\/api\/server-admin'/);
});

test('Route laeuft ausserhalb von Onboarding und Assistent', () => {
  assert.match(APP, /const isServerAdminRoute = path === SERVER_ADMIN_ROUTE;/);
  assert.match(APP, /\{isServerAdminRoute \? \(\s*<ServerAdminPage \/>\s*\) : \(\s*<OnboardingProvider>/);
  assert.match(APP, /const zeigeAssistent =\s*!isServerAdminRoute &&/);
});

test('Optik kommt aus der Partner-Dashboard-Shell, nicht aus einer Kopie', () => {
  assert.match(PAGE, /import \{ DashboardShell \} from '@\/components\/layout\/DashboardShell'/);
  assert.match(PAGE, /import \{ KpiCard \} from '@\/components\/cards\/KpiCard'/);
  assert.match(PAGE, /<DashboardShell\s+sidebar=\{/);
  assert.match(PAGE, /panel-card card-glow card-glow-accent hero-aura/);
  assert.match(PAGE, /gradient-accent/);
  assert.match(SIDEBAR, /import \{ SidebarLink, type SidebarNavItem \} from '@\/components\/layout\/SidebarLink'/);
  assert.match(SIDEBAR, /panel-card card-glow/);
  assert.match(SHELL, /activeRoute \? <DashboardSidebar activeRoute=\{activeRoute\} \/> : sidebar/);
  assert.doesNotMatch(PAGE + SIDEBAR, /<style|\.css'/, 'kein paralleles Stylesheet');
});

test('Einstellungen schicken nur erlaubte Felder mit CSRF-Token', async () => {
  const calls = mockFetch(200, { guildId: '1' });
  const smuggled = { language: 'de', aiFeatures: true, tags: ['x'], billing: 'pro' } as unknown as Parameters<
    typeof patchGuildSettings
  >[1];
  await patchGuildSettings('123456789012345678', smuggled, 'tok');
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, '/twitch/api/server-admin/guilds/123456789012345678/settings');
  assert.equal(calls[0].init.method, 'PATCH');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), { language: 'de' });
  assert.equal((calls[0].init.headers as Record<string, string>)['X-CSRF-Token'], 'tok');
});

test('Streamer-Aktivierung zielt nur auf die gewaehlte Guild', async () => {
  const calls = mockFetch(200, { ok: true, active: false });
  await setStreamerActive('42', '303', false, 'tok');
  assert.equal(calls[0].url, '/twitch/api/server-admin/guilds/42/streamers/303');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), { active: false });
});

test('401 fuehrt zum Login, 403 zu klaren Texten', async () => {
  mockFetch(401, { error: 'server_admin_login_required' });
  await assert.rejects(setStreamerActive('42', '303', true, 'tok'), ServerAdminLoginRequired);
  mockFetch(403, { error: 'guild_forbidden' });
  await assert.rejects(setStreamerActive('42', '303', true, 'tok'), (error: unknown) => {
    assert.ok(error instanceof ApiHttpError);
    assert.equal(error.status, 403);
    assert.match(errorText(error), /keine Verwaltungsrechte/);
    return true;
  });
  assert.match(errorText(new ApiHttpError('area_forbidden', 403)), /zentral verwaltet/);
});

test('Fehlercodes und HTTP-Fallbacks sind in beiden Anzeigesprachen übersetzt', () => {
  const cases: Array<[string, number, string]> = [
    ['guild_forbidden', 403, 'Für diesen Server hast du keine Verwaltungsrechte.'],
    ['unknown_field', 400, 'Dieser Bereich wird zentral verwaltet und ist hier gesperrt.'],
    ['invalid_field', 400, 'Ein Wert ist ungültig. Bitte prüfe IDs und Einladungslink.'],
    ['invalid_csrf', 403, 'Sitzung abgelaufen. Bitte lade die Seite neu.'],
    ['opaque_failure', 401, 'Bitte melde dich mit Discord an.'],
    ['opaque_failure', 404, 'Dieser Streamer oder Server ist nicht mehr verfügbar. Bitte lade die Seite neu.'],
    ['opaque_failure', 429, 'Zu viele Anfragen. Bitte versuche es gleich erneut.'],
    ['opaque_failure', 503, 'Der Server ist gerade nicht erreichbar. Bitte versuche es erneut.'],
    ['opaque_failure', 400, 'Die Anfrage konnte nicht verarbeitet werden. Bitte versuche es erneut.'],
  ];
  for (const language of ['de', 'en'] as const) {
    const t = (text: string) => translate(language, text);
    for (const [code, status, key] of cases) {
      assert.equal(errorText(new ApiHttpError(code, status), t), t(key));
    }
    assert.equal(errorText(new TypeError('private transport detail'), t), t('Der Server ist gerade nicht erreichbar. Bitte versuche es erneut.'));
    assert.equal(errorText(new Error('private detail'), t), t('Unbekannter Fehler.'));
  }
});

test('Snowflake-Pruefung', () => {
  assert.equal(isSnowflakeOrEmpty(''), true);
  assert.equal(isSnowflakeOrEmpty('1289721245281292288'), true);
  assert.equal(isSnowflakeOrEmpty('0123'), false);
  assert.equal(isSnowflakeOrEmpty('abc'), false);
});

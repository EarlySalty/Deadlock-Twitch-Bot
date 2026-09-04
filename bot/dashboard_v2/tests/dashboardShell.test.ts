/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');

const APP = read('App.tsx');
const SHELL = read('components/layout/DashboardShell.tsx');
const HOOK = read('hooks/useDashboardProfile.ts');

const PAGES = [
  'pages/InternalHomeLanding.tsx',
  'pages/Uplink.tsx',
  'pages/Verwaltung.tsx',
  'pages/OverlayBuilder.tsx',
  'pages/SocialMediaAdmin.tsx',
  'pages/Pricing.tsx',
] as const;

const ROUTES = ['home', 'analyse', 'social', 'uplink', 'verwaltung', 'overlay', 'pricing'] as const;

const ALLOWED_CONTENT_WIDTHS = new Set(['max-w-sm', 'max-w-xl', 'max-w-2xl']);
const MAX_W_CLASS = /max-w-(?:\[[^\]]*\]|[\w-]+)/g;

const forbiddenMaxWidths = (src: string): string[] =>
  (src.match(MAX_W_CLASS) ?? []).filter((cls) => !ALLOWED_CONTENT_WIDTHS.has(cls));

test('App.tsx importiert die Shell und wickelt jede der sieben Routen darin ein', () => {
  assert.match(APP, /import \{ DashboardShell \} from '@\/components\/layout\/DashboardShell'/);
  for (const route of ROUTES) {
    assert.match(
      APP,
      new RegExp(`activeRoute="${route}"`),
      `App.tsx muss die Route ${route} in DashboardShell wickeln`,
    );
  }
});

test('keine Seite setzt einen eigenen Gesamtrahmen mehr', () => {
  for (const page of PAGES) {
    const src = read(page);
    assert.doesNotMatch(src, /internal-home-vibe/, `${page} darf den Shell-Hintergrund nicht selbst setzen`);
    const offenders = forbiddenMaxWidths(src);
    assert.deepEqual(
      offenders,
      [],
      `${page} darf keine eigene Gesamt-Maximalbreite tragen: ${offenders.join(', ')}`,
    );
  }
});

test('die Shell trägt Hintergrund, Gesamtbreite, Sidebar-Spalte und den Main-Slot', () => {
  assert.match(SHELL, /internal-home-vibe/);
  assert.doesNotMatch(SHELL, /mx-auto/);
  assert.doesNotMatch(SHELL, /max-w-/);
  assert.match(SHELL, /lg:grid-cols-\[220px_minmax\(0,1fr\)\]/);
  assert.match(SHELL, /<DashboardSidebar activeRoute=\{activeRoute\} \/>/);
  assert.match(SHELL, /<main[^>]*>\{children\}<\/main>/);
});

test('der Shell-Profil-Hook gatet den Fetch gegen anonyme und Admin-Sitzungen ohne eigenes Konto', () => {
  assert.match(HOOK, /const isAuthenticated = authStatus\?\.authenticated === true;/);
  assert.match(HOOK, /const isLocalhostAdmin = Boolean\(authStatus\?\.isLocalhost\);/);
  assert.match(HOOK, /const isAdminWithoutOwnLogin = Boolean\(authStatus\?\.isAdmin\) && !ownLogin;/);
  assert.match(
    HOOK,
    /const canRequestInternalHome =\s*isAuthenticated && !loadingAuth && !isLocalhostAdmin && !isAdminWithoutOwnLogin;/,
  );
  assert.match(HOOK, /enabled:\s*canRequestInternalHome/);
});

test('App.tsx gatet die Pricing-Sidebar gegen den Auth-Status', () => {
  assert.match(APP, /function PricingRoute\(\)/, 'App.tsx braucht eine PricingRoute mit Auth-Gate');
  const start = APP.indexOf('function PricingRoute');
  const pricing = APP.slice(start, start + 500);
  assert.match(pricing, /useAuthStatus\(\)/, 'PricingRoute muss den Auth-Status laden');
  assert.match(
    pricing,
    /authStatus\?\.authenticated === true/,
    'PricingRoute muss auf authentifizierte Sitzungen pruefen',
  );
  assert.match(
    pricing,
    /showSidebar=\{authenticated\}/,
    'PricingRoute muss die Sidebar an die Auth-Entscheidung koppeln',
  );
  assert.match(APP, /<PricingRoute \/>/, 'Die Pricing-Route muss PricingRoute rendern');
});

test('der Analytics-Rahmen in App.tsx setzt keine eigene Gesamtbreite', () => {
  const analytics = APP.slice(
    APP.indexOf('function AnalyticsDashboard'),
    APP.indexOf('export default function App'),
  );
  assert.doesNotMatch(
    analytics,
    /internal-home-vibe/,
    'AnalyticsDashboard darf den Shell-Hintergrund nicht selbst setzen',
  );
  const offenders = forbiddenMaxWidths(analytics);
  assert.deepEqual(
    offenders,
    [],
    `AnalyticsDashboard darf keine eigene Gesamt-Maximalbreite tragen: ${offenders.join(', ')}`,
  );
});

test('App.tsx reicht die Demo-Entscheidung an die Shell durch', () => {
  assert.match(
    APP,
    /demoMode=\{isDemoMode\}/,
    'App.tsx muss den Demo-Zustand an DashboardShell durchreichen',
  );
});

test('DashboardShell laesst ohne Sidebar-Freigabe Sidebar und Profil-Hook aus', () => {
  assert.match(SHELL, /demoMode\?:\s*boolean/, 'Shell braucht eine demoMode-Prop');
  assert.match(SHELL, /showSidebar\?:\s*boolean/, 'Shell braucht eine showSidebar-Prop');
  assert.match(
    SHELL,
    /const withSidebar = !demoMode && showSidebar;/,
    'Shell muss Sidebar an demoMode und showSidebar koppeln',
  );
  const gateStart = SHELL.indexOf('withSidebar ?');
  const branchSplit = SHELL.indexOf(') : (', gateStart);
  assert.ok(gateStart >= 0 && branchSplit > gateStart, 'Shell muss withSidebar als Zweig behandeln');
  const sidebarBranch = SHELL.slice(gateStart, branchSplit);
  assert.match(
    sidebarBranch,
    /<DashboardSidebar activeRoute=\{activeRoute\} \/>/,
    'Der Sidebar-Zweig muss die Sidebar mounten',
  );
  const plainBranch = SHELL.slice(branchSplit);
  assert.doesNotMatch(
    plainBranch,
    /DashboardSidebar/,
    'Der Zweig ohne Sidebar-Freigabe darf DashboardSidebar nicht mounten',
  );
});

test('App.tsx traegt keine AuthBadge-Zeile mehr ueber dem Analyse-Kopf', () => {
  assert.doesNotMatch(APP, /AuthBadge/);
});

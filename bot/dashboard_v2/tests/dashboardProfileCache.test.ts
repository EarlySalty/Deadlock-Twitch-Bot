/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

(globalThis as { window?: unknown }).window = {
  __TWITCH_DASHBOARD_RUNTIME__: {},
  location: { origin: 'http://localhost' },
};

const { readCachedDashboardProfile, writeCachedDashboardProfile } = await import(
  '@/hooks/useDashboardProfile'
);

const CACHE_KEY = 'ddc.dashboard.profile';

function fakeStorage(initial: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(initial));
  return {
    getItem: (key: string): string | null => (store.has(key) ? store.get(key)! : null),
    setItem: (key: string, value: string): void => {
      store.set(key, value);
    },
  };
}

test('passende Identitaet liefert das gecachte Profil', () => {
  const storage = fakeStorage({
    [CACHE_KEY]: JSON.stringify({
      identityKey: 'nani',
      displayName: 'Nani',
      avatarUrl: 'https://cdn/x.png',
      planName: 'Pro',
      twitchLogin: 'nani',
    }),
  });
  assert.deepEqual(readCachedDashboardProfile('nani', storage), {
    identityKey: 'nani',
    displayName: 'Nani',
    avatarUrl: 'https://cdn/x.png',
    planName: 'Pro',
    twitchLogin: 'nani',
  });
});

test('fremde Identitaet liefert null', () => {
  const storage = fakeStorage({
    [CACHE_KEY]: JSON.stringify({ identityKey: 'nani', displayName: 'Nani' }),
  });
  assert.equal(readCachedDashboardProfile('jemand-anderes', storage), null);
});

test('kaputtes JSON liefert null statt zu werfen', () => {
  const storage = fakeStorage({ [CACHE_KEY]: '{nicht valide' });
  assert.equal(readCachedDashboardProfile('nani', storage), null);
});

test('fehlende Identitaet oder fehlender Speicher liefert null', () => {
  const storage = fakeStorage();
  assert.equal(readCachedDashboardProfile(null, storage), null);
  assert.equal(readCachedDashboardProfile('nani', null), null);
});

test('write speichert nur Anzeigefelder und wird von read wieder gelesen', () => {
  const storage = fakeStorage();
  writeCachedDashboardProfile(
    {
      identityKey: 'nani',
      displayName: 'Nani',
      avatarUrl: 'https://cdn/x.png',
      planName: 'Pro',
      twitchLogin: 'nani',
    },
    storage,
  );
  const raw = storage.getItem(CACHE_KEY);
  assert.ok(raw);
  const parsed = JSON.parse(raw);
  assert.deepEqual(Object.keys(parsed).sort(), [
    'avatarUrl',
    'displayName',
    'identityKey',
    'planName',
    'twitchLogin',
  ]);
  assert.deepEqual(readCachedDashboardProfile('nani', storage), {
    identityKey: 'nani',
    displayName: 'Nani',
    avatarUrl: 'https://cdn/x.png',
    planName: 'Pro',
    twitchLogin: 'nani',
  });
});

test('write ohne Identitaet schreibt nichts', () => {
  const storage = fakeStorage();
  writeCachedDashboardProfile(
    { identityKey: '', displayName: 'X', avatarUrl: null, planName: null, twitchLogin: null },
    storage,
  );
  assert.equal(storage.getItem(CACHE_KEY), null);
});

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');
const HOOK = read('hooks/useDashboardProfile.ts');
const SIDEBAR = read('components/layout/DashboardSidebar.tsx');

test('der Hook liest den Cache und verdrahtet ihn als placeholderData', () => {
  assert.match(HOOK, /readCachedDashboardProfile\(/);
  assert.match(HOOK, /writeCachedDashboardProfile\(/);
  assert.match(HOOK, /sessionStorage/);
  assert.match(HOOK, /placeholderData:\s*placeholderProfile/);
});

test('die Sidebar rendert ohne Profildaten einen neutralen Skeleton statt gradient-accent', () => {
  assert.match(SIDEBAR, /sidebar-avatar-skeleton/);
  assert.match(SIDEBAR, /animate-pulse/);
  const readyIdx = SIDEBAR.indexOf('profileReady ?');
  const gradientIdx = SIDEBAR.indexOf('gradient-accent');
  const skeletonIdx = SIDEBAR.indexOf('sidebar-avatar-skeleton');
  assert.ok(readyIdx >= 0, 'der gradient-accent-Zweig muss hinter profileReady gaten');
  assert.ok(readyIdx < gradientIdx, 'gradient-accent darf nur im profileReady-Zweig stehen');
  assert.ok(gradientIdx < skeletonIdx, 'der Skeleton-Zweig kommt nach dem gradient-accent-Zweig');
  assert.doesNotMatch(SIDEBAR.slice(skeletonIdx, skeletonIdx + 400), /gradient-accent/);
});

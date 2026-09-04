/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import {
  DASHBOARD_PROFILE_CACHE_KEY,
  clearCachedDashboardProfile,
  profilBereit,
  readCachedDashboardProfile,
  writeCachedDashboardProfile,
} from '../src/hooks/dashboardProfileCache';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');

const HOOK = read('hooks/useDashboardProfile.ts');
const CACHE = read('hooks/dashboardProfileCache.ts');
const SIDEBAR = read('components/layout/DashboardSidebar.tsx');

function fakeStorage() {
  const map = new Map<string, string>();
  return {
    map,
    getItem: (key: string) => (map.has(key) ? map.get(key)! : null),
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  };
}

test('read liefert das Profil bei passender Identitaet', () => {
  const storage = fakeStorage();
  storage.map.set(
    DASHBOARD_PROFILE_CACHE_KEY,
    JSON.stringify({
      identityKey: 'nani',
      displayName: 'Nani',
      avatarUrl: 'https://cdn/x.png',
      twitchLogin: 'nani',
    }),
  );
  const result = readCachedDashboardProfile('nani', storage);
  assert.deepEqual(result, {
    identityKey: 'nani',
    displayName: 'Nani',
    avatarUrl: 'https://cdn/x.png',
    twitchLogin: 'nani',
  });
});

test('read liefert null bei fremder Identitaet', () => {
  const storage = fakeStorage();
  storage.map.set(
    DASHBOARD_PROFILE_CACHE_KEY,
    JSON.stringify({ identityKey: 'leo', displayName: 'Leo' }),
  );
  assert.equal(readCachedDashboardProfile('nani', storage), null);
});

test('read liefert null bei kaputtem JSON', () => {
  const storage = fakeStorage();
  storage.map.set(DASHBOARD_PROFILE_CACHE_KEY, '{nicht valide');
  assert.equal(readCachedDashboardProfile('nani', storage), null);
});

test('read liefert null bei fehlendem Eintrag', () => {
  const storage = fakeStorage();
  assert.equal(readCachedDashboardProfile('nani', storage), null);
});

test('read liefert null ohne Identitaet oder Storage', () => {
  assert.equal(readCachedDashboardProfile(null, fakeStorage()), null);
  assert.equal(readCachedDashboardProfile('nani', null), null);
});

test('write schreibt nur Anzeigefelder und Identitaetsschluessel', () => {
  const storage = fakeStorage();
  writeCachedDashboardProfile(
    {
      identityKey: 'nani',
      displayName: 'Nani',
      avatarUrl: 'https://cdn/x.png',
      twitchLogin: 'nani',
    },
    storage,
  );
  const raw = storage.map.get(DASHBOARD_PROFILE_CACHE_KEY);
  assert.ok(raw);
  const parsed = JSON.parse(raw!);
  assert.deepEqual(Object.keys(parsed).sort(), [
    'avatarUrl',
    'displayName',
    'identityKey',
    'twitchLogin',
  ]);
  assert.equal(parsed.identityKey, 'nani');
});

test('write ohne Identitaet schreibt nichts', () => {
  const storage = fakeStorage();
  writeCachedDashboardProfile(
    { identityKey: '', displayName: 'Nani', avatarUrl: null, twitchLogin: null },
    storage,
  );
  assert.equal(storage.map.has(DASHBOARD_PROFILE_CACHE_KEY), false);
});

test('clear entfernt den Cache-Eintrag', () => {
  const storage = fakeStorage();
  storage.map.set(DASHBOARD_PROFILE_CACHE_KEY, JSON.stringify({ identityKey: 'nani' }));
  clearCachedDashboardProfile(storage);
  assert.equal(storage.map.has(DASHBOARD_PROFILE_CACHE_KEY), false);
});

test('profilBereit ist true nach fehlgeschlagenem Fetch ohne Cache', () => {
  assert.equal(
    profilBereit({
      loadingAuth: false,
      loadingProfile: false,
      hasProfile: false,
      hasCache: false,
      canRequest: true,
    }),
    true,
  );
});

test('profilBereit ist false waehrend der Fetch laeuft ohne Cache', () => {
  assert.equal(
    profilBereit({
      loadingAuth: false,
      loadingProfile: true,
      hasProfile: false,
      hasCache: false,
      canRequest: true,
    }),
    false,
  );
});

test('profilBereit ist false solange Auth laedt', () => {
  assert.equal(
    profilBereit({
      loadingAuth: true,
      loadingProfile: false,
      hasProfile: true,
      hasCache: true,
      canRequest: false,
    }),
    false,
  );
});

test('profilBereit ist true mit Cache trotz laufendem Fetch', () => {
  assert.equal(
    profilBereit({
      loadingAuth: false,
      loadingProfile: true,
      hasProfile: false,
      hasCache: true,
      canRequest: true,
    }),
    true,
  );
});

test('profilBereit ist true fuer Admin ohne internal-home-Anfrage', () => {
  assert.equal(
    profilBereit({
      loadingAuth: false,
      loadingProfile: false,
      hasProfile: false,
      hasCache: false,
      canRequest: false,
    }),
    true,
  );
});

test('Verdrahtung: der Hook nutzt die reinen Cache-Funktionen und loescht bei Logout', () => {
  assert.match(HOOK, /from '@\/hooks\/dashboardProfileCache'/);
  assert.match(HOOK, /readCachedDashboardProfile\(identityKey, dashboardProfileStorage\(\)\)/);
  assert.match(HOOK, /profilBereit\(\{/);
  assert.match(HOOK, /clearCachedDashboardProfile\(dashboardProfileStorage\(\)\)/);
  assert.match(HOOK, /authStatus\?\.authenticated === false/);
  assert.doesNotMatch(HOOK, /cachedProfile\?\.planName/);
});

test('Verdrahtung: das Cache-Modul ist seiteneffektfrei und schuetzt JSON und Identitaet', () => {
  assert.doesNotMatch(CACHE, /\bwindow\b/);
  assert.doesNotMatch(CACHE, /\bimport\b/);
  assert.match(CACHE, /parsed = JSON\.parse\(raw\);/);
  assert.match(CACHE, /if \(record\.identityKey !== identityKey\) return null;/);
});

test('die Sidebar rendert ohne Profildaten einen neutralen Skeleton statt gradient-accent', () => {
  const avatarStart = SIDEBAR.indexOf('{shownAvatar ?');
  assert.ok(avatarStart >= 0, 'der Avatar-Block muss ueber shownAvatar gaten');
  const block = SIDEBAR.slice(avatarStart, avatarStart + 800);
  assert.match(block, /sidebar-avatar-skeleton/);
  assert.match(block, /animate-pulse/);
  const readyIdx = block.indexOf('profileReady ?');
  const gradientIdx = block.indexOf('gradient-accent');
  const skeletonIdx = block.indexOf('sidebar-avatar-skeleton');
  assert.ok(readyIdx >= 0, 'der gradient-accent-Zweig muss hinter profileReady gaten');
  assert.ok(readyIdx < gradientIdx, 'gradient-accent darf nur im profileReady-Zweig stehen');
  assert.ok(gradientIdx < skeletonIdx, 'der Skeleton-Zweig kommt nach dem gradient-accent-Zweig');
  assert.doesNotMatch(block.slice(skeletonIdx), /gradient-accent/);
});

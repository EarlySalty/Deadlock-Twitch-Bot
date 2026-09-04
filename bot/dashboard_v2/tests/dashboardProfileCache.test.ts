/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const SRC = join(import.meta.dirname, '..', 'src');
const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8');

const HOOK = read('hooks/useDashboardProfile.ts');
const SIDEBAR = read('components/layout/DashboardSidebar.tsx');

test('der Hook definiert reine Cache-Funktionen mit Identitaets- und JSON-Schutz', () => {
  assert.match(HOOK, /export function readCachedDashboardProfile\(/);
  assert.match(HOOK, /export function writeCachedDashboardProfile\(/);
  assert.match(HOOK, /if \(!identityKey \|\| !storage\) return null;/);
  assert.match(HOOK, /if \(!raw\) return null;/);
  assert.match(HOOK, /parsed = JSON\.parse\(raw\);/);
  assert.match(HOOK, /if \(record\.identityKey !== identityKey\) return null;/);
  assert.match(HOOK, /if \(!value\.identityKey \|\| !storage\) return;/);
});

test('read faengt kaputtes JSON und leeren Cache ab, statt zu werfen', () => {
  const start = HOOK.indexOf('export function readCachedDashboardProfile');
  const end = HOOK.indexOf('export function writeCachedDashboardProfile');
  assert.ok(start >= 0 && end > start, 'read-Funktion muss vor write stehen');
  const body = HOOK.slice(start, end);
  const tryCount = (body.match(/\btry\s*\{/g) ?? []).length;
  const nullReturns = (body.match(/return null;/g) ?? []).length;
  assert.ok(tryCount >= 2, 'read muss getItem und JSON.parse je in try/catch kapseln');
  assert.ok(nullReturns >= 5, 'read muss jede Fehlbedingung mit null quittieren');
});

test('der Hook cacht nur Anzeigefelder plus Identitaetsschluessel, keine Secrets', () => {
  const start = HOOK.indexOf('if (!identityKey || isPlaceholderData || !profile) return;');
  assert.ok(start >= 0, 'der Hook muss den Cache aktiv schreiben');
  const call = HOOK.slice(start, HOOK.indexOf('}, [identityKey, isPlaceholderData', start));
  for (const feld of ['identityKey', 'displayName', 'avatarUrl', 'planName', 'twitchLogin']) {
    assert.match(call, new RegExp(`${feld}[,:]`), `Cache-Schreibweg muss ${feld} setzen`);
  }
  assert.doesNotMatch(call, /token|csrf|secret|cookie/i, 'kein Secret in den Cache schreiben');
});

test('der Hook liest den Cache aus sessionStorage und verdrahtet ihn als placeholderData', () => {
  assert.match(HOOK, /readCachedDashboardProfile\(identityKey, dashboardProfileStorage\(\)\)/);
  assert.match(HOOK, /return window\.sessionStorage;/);
  assert.match(HOOK, /placeholderData:\s*placeholderProfile/);
  assert.match(HOOK, /const identityKey = canRequestInternalHome && ownLogin \? ownLogin : null;/);
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

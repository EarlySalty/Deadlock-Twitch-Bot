import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveTabParam } from '../src/tabAliases';

test('alte Tab-ID wird auf neuen Tab + Sub gemappt', () => {
  assert.deepEqual(resolveTabParam('chat'), { tab: 'audience', sub: 'chat' });
  assert.deepEqual(resolveTabParam('viewers'), { tab: 'audience', sub: 'viewer' });
  assert.deepEqual(resolveTabParam('category'), { tab: 'growth', sub: 'markt' });
});

test('alte Ratgeber-Tabs landen im Hub mit passendem Modus', () => {
  assert.deepEqual(resolveTabParam('reports'), { tab: 'coaching', mode: 'session' });
  assert.deepEqual(resolveTabParam('ai'), { tab: 'coaching', mode: 'gesamt' });
});

test('aktuelle Tab-ID loest auf sich selbst auf', () => {
  assert.deepEqual(resolveTabParam('monetization'), { tab: 'monetization' });
  assert.deepEqual(resolveTabParam('overview'), { tab: 'overview' });
});

test('unbekannt oder leer liefert null', () => {
  assert.equal(resolveTabParam('bogus'), null);
  assert.equal(resolveTabParam(null), null);
});

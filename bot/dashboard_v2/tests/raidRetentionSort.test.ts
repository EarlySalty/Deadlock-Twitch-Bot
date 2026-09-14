/// <reference types="node" />
import { strict as assert } from 'node:assert';
import test from 'node:test';

import { filterAndSortRaids } from '../src/types/analytics';
import type { RaidRetentionEntry } from '../src/types/analytics';

function raid(over: Partial<RaidRetentionEntry>): RaidRetentionEntry {
  return {
    raidId: 0,
    toBroadcaster: 'kanal',
    viewersSent: 0,
    executedAt: '2026-09-01T00:00:00Z',
    chattersAt5m: 0,
    chattersAt15m: 0,
    chattersAt30m: 0,
    retention30mPct: 0,
    newChatters: 0,
    chatterConversionPct: 0,
    knownFromRaider: 0,
    ...over,
  };
}

const RAIDS: RaidRetentionEntry[] = [
  raid({ raidId: 1, toBroadcaster: 'Alpha', viewersSent: 30, newChatters: 5 }),
  raid({ raidId: 2, toBroadcaster: 'beta', viewersSent: 90, newChatters: null }),
  raid({ raidId: 3, toBroadcaster: 'Gamma', viewersSent: 60, newChatters: 12 }),
];

test('Standard ist Gesendet absteigend', () => {
  const out = filterAndSortRaids(RAIDS, '', 'viewersSent', 'desc');
  assert.deepEqual(out.map((r) => r.raidId), [2, 3, 1]);
});

test('aufsteigend dreht die Reihenfolge', () => {
  const out = filterAndSortRaids(RAIDS, '', 'viewersSent', 'asc');
  assert.deepEqual(out.map((r) => r.raidId), [1, 3, 2]);
});

test('Suche filtert nach Ziel-Streamer als Teilstring ohne Gross-/Kleinschreibung', () => {
  const out = filterAndSortRaids(RAIDS, 'AM', 'viewersSent', 'desc');
  assert.deepEqual(out.map((r) => r.raidId), [3]);
});

test('leere Suche behaelt alle Raids', () => {
  const out = filterAndSortRaids(RAIDS, '   ', 'viewersSent', 'desc');
  assert.equal(out.length, 3);
});

test('Null-Werte landen bei absteigender Sortierung unten', () => {
  const out = filterAndSortRaids(RAIDS, '', 'newChatters', 'desc');
  assert.deepEqual(out.map((r) => r.raidId), [3, 1, 2]);
});

test('nach Ziel-Streamer wird alphabetisch sortiert', () => {
  const out = filterAndSortRaids(RAIDS, '', 'toBroadcaster', 'asc');
  assert.deepEqual(out.map((r) => r.toBroadcaster), ['Alpha', 'beta', 'Gamma']);
});

test('die Eingabeliste wird nicht mutiert', () => {
  const kopie = RAIDS.map((r) => r.raidId);
  filterAndSortRaids(RAIDS, '', 'retention30mPct', 'asc');
  assert.deepEqual(RAIDS.map((r) => r.raidId), kopie);
});

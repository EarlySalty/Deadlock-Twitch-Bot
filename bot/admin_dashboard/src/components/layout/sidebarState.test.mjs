import test from 'node:test';
import assert from 'node:assert/strict';
import {
  readSidebarGroupState,
  sidebarStorageKey,
  writeSidebarGroupState,
} from './sidebarState.ts';

const labels = ['Übersicht', 'Partner'];

test('jede stabile Twitch-ID erhält einen eigenen Storage-Key und offene Defaults', () => {
  assert.notEqual(sidebarStorageKey('42'), sidebarStorageKey('99'));
  const storage = { getItem: () => null };
  assert.deepEqual(readSidebarGroupState(() => storage, sidebarStorageKey('42'), labels), {
    Übersicht: true,
    Partner: true,
  });
});

test('gesperrter Storage-Getter lässt die Navigation offen und bedienbar', () => {
  const blocked = () => { throw new DOMException('blocked', 'SecurityError'); };
  assert.deepEqual(readSidebarGroupState(blocked, sidebarStorageKey('42'), labels), {
    Übersicht: true,
    Partner: true,
  });
  assert.doesNotThrow(() => writeSidebarGroupState(blocked, sidebarStorageKey('42'), { Übersicht: false }));
});

test('ohne bestätigte Nutzer-ID wird kein Browser-Storage gelesen oder beschrieben', () => {
  const blocked = () => { throw new Error('should not access storage'); };
  assert.deepEqual(readSidebarGroupState(blocked, undefined, labels), {
    Übersicht: true,
    Partner: true,
  });
  assert.doesNotThrow(() => writeSidebarGroupState(blocked, undefined, { Übersicht: false }));
});

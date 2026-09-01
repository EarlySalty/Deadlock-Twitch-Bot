import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  VERWALTUNG_TAB_IDS,
  resolveVerwaltungTab,
} from '../src/pages/verwaltungTabs';

test('nimmt den Tab aus dem Hash, damit Deeplinks und Reload halten', () => {
  assert.equal(resolveVerwaltungTab('#chat'), 'chat');
  assert.equal(resolveVerwaltungTab('#bot'), 'bot');
  assert.equal(resolveVerwaltungTab('#werbung'), 'werbung');
});

test('kommt auch ohne führendes # klar', () => {
  assert.equal(resolveVerwaltungTab('overlay'), 'overlay');
});

test('ignoriert Groß-/Kleinschreibung und Leerzeichen', () => {
  assert.equal(resolveVerwaltungTab('  #Chat '), 'chat');
});

test('fällt bei unbekanntem oder leerem Hash auf den ersten Tab zurück', () => {
  assert.equal(resolveVerwaltungTab('#gibtsnicht'), 'konto');
  assert.equal(resolveVerwaltungTab(''), 'konto');
  assert.equal(resolveVerwaltungTab('#'), 'konto');
  assert.equal(resolveVerwaltungTab(undefined), 'konto');
});

test('jede Sektion liegt in genau einem Tab', () => {
  assert.deepEqual(VERWALTUNG_TAB_IDS, ['konto', 'chat', 'bot', 'overlay', 'werbung']);
  assert.equal(new Set(VERWALTUNG_TAB_IDS).size, VERWALTUNG_TAB_IDS.length);
});

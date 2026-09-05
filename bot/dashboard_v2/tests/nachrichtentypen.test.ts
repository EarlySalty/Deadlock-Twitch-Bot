import { test } from 'node:test';
import assert from 'node:assert/strict';

import { MESSAGE_TYPE_LABELS } from '../src/i18n/dictionary';

const API_KEYS = [
  'Command',
  'Hype',
  'Greeting',
  'Question',
  'Feedback',
  'Technical',
  'Social',
  'Reaction',
  'Game-Related',
  'Statement',
  'Other',
];

test('jeder API-Schluessel hat eine nicht leere deutsche Bezeichnung', () => {
  for (const key of API_KEYS) {
    const label = MESSAGE_TYPE_LABELS[key];
    assert.ok(label, `keine Bezeichnung fuer ${key}`);
    assert.ok(label.trim().length > 0, `leere Bezeichnung fuer ${key}`);
  }
});

test('erwartete deutsche Bezeichnungen stimmen', () => {
  assert.equal(MESSAGE_TYPE_LABELS.Command, 'Befehl');
  assert.equal(MESSAGE_TYPE_LABELS.Greeting, 'Begrüßung');
  assert.equal(MESSAGE_TYPE_LABELS.Question, 'Frage');
  assert.equal(MESSAGE_TYPE_LABELS.Technical, 'Technik');
  assert.equal(MESSAGE_TYPE_LABELS.Social, 'Sozial');
  assert.equal(MESSAGE_TYPE_LABELS['Game-Related'], 'Spielbezug');
  assert.equal(MESSAGE_TYPE_LABELS.Statement, 'Aussage');
  assert.equal(MESSAGE_TYPE_LABELS.Other, 'Sonstiges');
});

test('System bleibt ohne Karten-Bezeichnung, weil es nie angezeigt wird', () => {
  assert.equal(MESSAGE_TYPE_LABELS.System, undefined);
});

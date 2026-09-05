import { test } from 'node:test';
import assert from 'node:assert/strict';

import { kalenderFenster } from '../src/utils/zeitraum';

test('kalenderFenster haelt die Untergrenze von 30 Tagen', () => {
  assert.equal(kalenderFenster(7), 30);
  assert.equal(kalenderFenster(14), 30);
  assert.equal(kalenderFenster(30), 30);
});

test('kalenderFenster folgt groesseren Zeitraeumen', () => {
  assert.equal(kalenderFenster(90), 90);
  assert.equal(kalenderFenster(365), 365);
});

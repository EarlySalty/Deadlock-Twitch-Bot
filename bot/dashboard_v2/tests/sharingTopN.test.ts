import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  SHARING_TOPN_DEFAULT,
  SHARING_TOPN_OPTIONS,
  sanitizeSharingTopN,
  readSharingTopN,
} from '../src/utils/sharingTopN';

test('gueltige Werte bleiben erhalten', () => {
  for (const option of SHARING_TOPN_OPTIONS) {
    assert.equal(sanitizeSharingTopN(option), option);
    assert.equal(sanitizeSharingTopN(String(option)), option);
  }
});

test('ungueltige Werte fallen auf den Default', () => {
  for (const bad of [null, undefined, '', 'abc', 0, 4, 7, 100, NaN, {}, []]) {
    assert.equal(sanitizeSharingTopN(bad), SHARING_TOPN_DEFAULT);
  }
});

test('Default ist 3 und in den Optionen enthalten', () => {
  assert.equal(SHARING_TOPN_DEFAULT, 3);
  assert.ok(SHARING_TOPN_OPTIONS.includes(SHARING_TOPN_DEFAULT));
});

test('readSharingTopN liest gespeicherte Wahl aus dem Storage', () => {
  const store = new Map<string, string>([['ddc.sharingTimelineTopN', '10']]);
  const fake = { getItem: (key: string) => store.get(key) ?? null };
  assert.equal(readSharingTopN(fake), 10);
});

test('readSharingTopN nimmt den Default bei kaputtem Storage-Wert', () => {
  const fake = { getItem: () => 'weird' };
  assert.equal(readSharingTopN(fake), SHARING_TOPN_DEFAULT);
});

test('readSharingTopN faengt Storage-Fehler ab', () => {
  const fake = {
    getItem: () => {
      throw new Error('blocked');
    },
  };
  assert.equal(readSharingTopN(fake), SHARING_TOPN_DEFAULT);
});

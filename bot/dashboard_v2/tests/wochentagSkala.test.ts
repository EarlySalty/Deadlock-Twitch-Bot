/// <reference types="node" />
import { strict as assert } from 'node:assert';
import test from 'node:test';

import { wochentagBalkenHoehe } from '../src/types/analytics';

test('der kleinste Wert bekommt 15 Prozent Hoehe', () => {
  assert.equal(wochentagBalkenHoehe(2.6, 2.6, 4.1), 15);
});

test('der groesste Wert bekommt 100 Prozent Hoehe', () => {
  assert.equal(wochentagBalkenHoehe(4.1, 2.6, 4.1), 100);
});

test('die Mitte liegt bei 57,5 Prozent', () => {
  assert.equal(wochentagBalkenHoehe(3, 2, 4), 57.5);
});

test('bei gleichem Min und Max sind es 100 Prozent', () => {
  assert.equal(wochentagBalkenHoehe(3, 3, 3), 100);
});

test('Werte ausserhalb der Spanne werden geklemmt', () => {
  assert.equal(wochentagBalkenHoehe(1, 2, 4), 15);
  assert.equal(wochentagBalkenHoehe(9, 2, 4), 100);
});

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { clampDays, parseDaysParam, streamerAusUrlErlaubt } from '../src/utils/zeitraum';

test('clampDays deckelt auf 7 bis 365', () => {
  assert.equal(clampDays(7), 7);
  assert.equal(clampDays(365), 365);
  assert.equal(clampDays(30), 30);
  assert.equal(clampDays(3), 7);
  assert.equal(clampDays(0), 7);
  assert.equal(clampDays(400), 365);
  assert.equal(clampDays(-10), 7);
});

test('clampDays schneidet Nachkommastellen ab', () => {
  assert.equal(clampDays(30.9), 30);
  assert.equal(clampDays(7.4), 7);
});

test('clampDays faellt bei ungueltiger Zahl auf 30', () => {
  assert.equal(clampDays(Number.NaN), 30);
  assert.equal(clampDays(Number.POSITIVE_INFINITY), 30);
});

test('parseDaysParam akzeptiert ganze Zahlen 7 bis 365', () => {
  assert.equal(parseDaysParam('7'), 7);
  assert.equal(parseDaysParam('14'), 14);
  assert.equal(parseDaysParam('90'), 90);
  assert.equal(parseDaysParam('365'), 365);
});

test('parseDaysParam deckelt Werte ausserhalb des Bereichs', () => {
  assert.equal(parseDaysParam('1'), 7);
  assert.equal(parseDaysParam('5000'), 365);
});

test('parseDaysParam faellt bei Unsinn auf 30', () => {
  assert.equal(parseDaysParam(null), 30);
  assert.equal(parseDaysParam(''), 30);
  assert.equal(parseDaysParam('abc'), 30);
  assert.equal(parseDaysParam('30.5'), 30);
  assert.equal(parseDaysParam('12px'), 30);
});

test('parseDaysParam ignoriert umgebende Leerzeichen', () => {
  assert.equal(parseDaysParam('  45  '), 45);
});

test('streamerAusUrlErlaubt lässt ausserhalb der Demo-Shell jeden Streamer zu', () => {
  assert.equal(streamerAusUrlErlaubt('foo', false, []), true);
  assert.equal(streamerAusUrlErlaubt('foo', false, ['bar']), true);
});

test('streamerAusUrlErlaubt lässt in der Demo-Shell ohne Whitelist jeden zu', () => {
  assert.equal(streamerAusUrlErlaubt('foo', true, []), true);
});

test('streamerAusUrlErlaubt prüft in der Demo-Shell gegen die Whitelist', () => {
  assert.equal(streamerAusUrlErlaubt('foo', true, ['foo', 'bar']), true);
  assert.equal(streamerAusUrlErlaubt('baz', true, ['foo', 'bar']), false);
});

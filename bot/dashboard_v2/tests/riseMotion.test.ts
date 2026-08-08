import { test } from 'node:test';
import assert from 'node:assert/strict';
import { riseDelayMs, riseStyle, RISE_MAX_DELAY_MS, RISE_STEP_MS } from '../src/motion/rise';

test('Stufen staffeln in kurzen Schritten statt in Zehntelsekunden', () => {
  assert.equal(riseDelayMs(0), 0);
  assert.equal(riseDelayMs(1), RISE_STEP_MS);
  assert.equal(riseDelayMs(3), 3 * RISE_STEP_MS);
  assert.ok(RISE_STEP_MS <= 80, 'Schritt bleibt im Emil-Fenster von 30-80ms');
});

test('lange Ketten laufen gegen eine Obergrenze, nicht ins Endlose', () => {
  // Coaching.tsx staffelte bis delay 0.65 — der letzte Block kam nach 650ms.
  assert.equal(riseDelayMs(99), RISE_MAX_DELAY_MS);
  assert.ok(RISE_MAX_DELAY_MS <= 240, 'kein Block wartet laenger als 240ms');
});

test('Sekundenwerte aus den alten framer-motion-Props werden uebernommen', () => {
  // transition={{ delay: 0.15 }} -> 150ms, aber gekappt und gerastert.
  assert.equal(riseDelayMs({ seconds: 0.15 }), 150);
  assert.equal(riseDelayMs({ seconds: 0.65 }), RISE_MAX_DELAY_MS);
  assert.equal(riseDelayMs({ seconds: 0 }), 0);
});

test('negative und unsinnige Eingaben fallen auf 0 zurueck', () => {
  assert.equal(riseDelayMs(-3), 0);
  assert.equal(riseDelayMs({ seconds: -1 }), 0);
  assert.equal(riseDelayMs(Number.NaN), 0);
});

test('riseStyle liefert die Verzoegerung als CSS-Variable, nicht als Inline-Animation', () => {
  const style = riseStyle(2);
  assert.deepEqual(style, { '--rise-delay': `${2 * RISE_STEP_MS}ms` });
  assert.equal(riseStyle(0), undefined, 'ohne Verzoegerung kein ueberfluessiges style-Attribut');
});

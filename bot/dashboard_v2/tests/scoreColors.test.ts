import { test } from 'node:test';
import assert from 'node:assert/strict';

import { getScoreColor, getRetentionColor } from '../src/utils/formatters';

// Die Skala hat vier Stufen. Ein Rebrand darf sie umfärben, aber nicht zusammenlegen:
// zwei Stufen mit derselben Farbe löschen stillschweigend eine Bedeutungsstufe.
test('getScoreColor liefert vier unterscheidbare Stufen', () => {
  const stufen = [95, 70, 45, 10].map(getScoreColor);
  assert.equal(new Set(stufen).size, 4, `Stufen kollabiert: ${stufen.join(', ')}`);
});

test('getRetentionColor liefert vier unterscheidbare Stufen', () => {
  const stufen = [85, 60, 40, 10].map(getRetentionColor);
  assert.equal(new Set(stufen).size, 4, `Stufen kollabiert: ${stufen.join(', ')}`);
});

test('Score-Schwellen liegen bei 80/60/40', () => {
  assert.equal(getScoreColor(80), getScoreColor(95));
  assert.notEqual(getScoreColor(79), getScoreColor(80));
  assert.notEqual(getScoreColor(59), getScoreColor(60));
  assert.notEqual(getScoreColor(39), getScoreColor(40));
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { scoresForBuild } from './brainLab.mjs';

test('each family uses only its own scores', () => {
  const gun = { family: { id: 'gun' } };
  const spirit = { family: { id: 'spirit' } };
  const report = { build: gun, scored: ['legacy'], variant_scores: { gun: ['weapon'], spirit: ['spirit'] } };
  assert.deepEqual(scoresForBuild(report, gun), ['weapon']);
  assert.deepEqual(scoresForBuild(report, spirit), ['spirit']);
});
test('missing family scores do not fall back to a different playstyle', () => {
  const build = { family: { id: 'missing' } };
  const report = { build, scored: ['wrong'], variant_scores: {} };
  assert.deepEqual(scoresForBuild(report, build), []);
});
test('only an unconditioned root may use the base scores', () => {
  const build = { family: null };
  const report = { build, scored: ['base'], variant_scores: {} };
  assert.deepEqual(scoresForBuild(report, build), ['base']);
  assert.deepEqual(scoresForBuild(report, { family: null }), []);
});

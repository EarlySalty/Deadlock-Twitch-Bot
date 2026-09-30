import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { neverWordList } from '../src/utils/titlePreferences';

test('Verbotsliste normalisiert, zählt Unicode-Zeichen und begrenzt 40 Einträge', () => {
  assert.deepEqual(neverWordList('  cringe  \nCRINGE\n\nmehr   Wörter'), ['cringe', 'mehr Wörter']);
  const entries = neverWordList(Array.from({ length: 50 }, (_, n) => `${n} ${'😀'.repeat(70)}`).join('\n'));
  assert.equal(entries.length, 40);
  assert.ok(entries.every((entry) => [...entry].length === 60));
  assert.ok(entries.every((entry) => !/[\uD800-\uDBFF]$/.test(entry)));
});

test('So nicht bleibt freiwillig und bei voller Liste wird kein wirkungsloser Knopf angeboten', () => {
  const page = readFileSync(new URL('../src/pages/TitleGenerator.tsx', import.meta.url), 'utf8');
  assert.ok(page.includes("feedbackState === 'disliked' && neverWordList(neverWords).length < 40"));
  assert.ok(page.includes("feedbackState === 'disliked' && neverWordList(neverWords).length >= 40"));
  assert.ok(page.includes('includeLive && result?.co_streamers'));
  assert.ok(!page.includes('Live-Hero'));
  assert.ok(!page.includes('Wird sicher weggelassen'));
});

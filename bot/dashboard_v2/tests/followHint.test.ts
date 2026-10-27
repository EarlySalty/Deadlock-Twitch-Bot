import test from 'node:test';
import assert from 'node:assert/strict';
import { countChars, followHintTooLong, normalizeFollowHint } from '../src/api/followHint';

test('emoji count as one character like the backend', () => {
  const emoji = '💜'.repeat(290);
  assert.equal(countChars(emoji), 290);
  assert.equal(followHintTooLong(emoji, 300), false);
});

test('placeholder expansion beyond the Twitch limit is rejected', () => {
  assert.equal(followHintTooLong(Array(25).fill('@{user}').join(' '), 300), true);
  assert.equal(followHintTooLong(`${'y'.repeat(270)} {user}`, 300), false);
});

test('whitespace collapses like the backend', () => {
  assert.equal(normalizeFollowHint('  a\n\n  b  '), 'a b');
  assert.equal(followHintTooLong('   ', 300), false);
});

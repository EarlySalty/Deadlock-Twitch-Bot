import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { parsePublicProfiles } from '../src/lib/publicProfiles.ts';

test('published directory accepts only safe profile paths', () => {
  assert.deepEqual(parsePublicProfiles({ profiles: [
    { login: 'alice_1', headline: 'Deadlock mit euch' },
    { login: '../admin', headline: 'Bad' },
    { login: 'javascript:alert(1)', headline: 'Bad' },
    { login: 'bob', headline: null },
    null,
  ] }), [{ login: 'alice_1', headline: 'Deadlock mit euch' }]);
  assert.deepEqual(parsePublicProfiles(null), []);
  assert.deepEqual(parsePublicProfiles({ profiles: 'bad' }), []);
});
test('both landing variants expose the shared profile directory', () => {
  const source = readFileSync(new URL('../src/components/partner-clean/PartnerNetwork.tsx', import.meta.url), 'utf8');
  assert.match(source, /<PublicProfiles\s*\/>/);
  for (const name of ['StreamerNetworkPage', 'StreamerNetworkV3Page']) {
    assert.match(readFileSync(new URL(`../src/pages/${name}.tsx`, import.meta.url), 'utf8'), /components\/partner-clean\/PartnerNetwork/);
  }
});
test('directory fails closed and refreshes without cached credentials', () => {
  const source = readFileSync(new URL('../src/components/partner-clean/PublicProfiles.tsx', import.meta.url), 'utf8');
  assert.match(source, /cache: "no-store"/);
  assert.match(source, /credentials: "omit"/);
  assert.match(source, /setProfiles\(\[\]\)/);
  assert.match(source, /visibilitychange/);
  assert.match(source, /current === generation/);
  assert.match(source, /\/streamer\/\$\{profile.login\}/);
});

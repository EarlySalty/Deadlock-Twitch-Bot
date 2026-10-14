import assert from 'node:assert/strict';
import { test } from 'node:test';
import { draftFromSaved, isDraftChanged, parseDraft, uniqueFields, withoutSubmitted } from './settingsCatalog.ts';
import { settingsCatalogFixture } from './settingsCatalogFixture.ts';

const field = path => settingsCatalogFixture.fields.find(entry => entry.path === path);
const guilds = field('discord.raid_oauth.allowed_guild_ids');
const port = field('bot.legacy_internal_api_port');

test('integer ids above 2^53 stay exact strings on the wire', () => {
  assert.deepEqual(parseDraft(guilds, ['9223372036854775807', '1289721245281292288']), { ok: true, value: ['9223372036854775807', '1289721245281292288'] });
  assert.equal(parseDraft(guilds, ['9223372036854775808']).ok, false);
  assert.equal(parseDraft(guilds, ['0012']).ok, false);
  assert.equal(parseDraft(port, '-0').ok, false);
});

test('null and empty list stay distinct', () => {
  assert.deepEqual(parseDraft(guilds, null), { ok: true, value: null });
  assert.deepEqual(parseDraft(guilds, []), { ok: true, value: [] });
  assert.equal(isDraftChanged(guilds, [], null), true);
  assert.equal(isDraftChanged(guilds, null, null), false);
});

test('decimal input with comma matches saved number', () => {
  const window = field('bot.auto_unraid_window_seconds');
  assert.equal(draftFromSaved(window, 90), '90');
  assert.equal(isDraftChanged(window, '90,0', 90), false);
  assert.deepEqual(parseDraft(window, '1,5'), { ok: true, value: 1.5 });
});

test('duplicate catalog paths render once', () => {
  assert.equal(uniqueFields(settingsCatalogFixture.fields).length, settingsCatalogFixture.fields.length - 1);
});

test('save completion keeps drafts edited after submission', () => {
  const submittedList = ['1'];
  const newerList = ['1', '2'];
  const drafts = { a: submittedList, b: '5', c: true };
  const remaining = withoutSubmitted({ ...drafts, a: newerList }, { a: submittedList, b: '5' });
  assert.deepEqual(remaining, { a: newerList, c: true });
});

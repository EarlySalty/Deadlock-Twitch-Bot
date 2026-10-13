import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { afterEach, test } from 'node:test';
import { fetchClipCommandSettings, fetchPrivateVodClips, setPrivateVodCommand } from '../src/api/clipCommand';
import { dictionaryFor, setActiveLanguage, translate } from '../src/i18n/dictionary';
import { checkFile } from '../tools/i18nCoverage';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
  setActiveLanguage('de');
});

test('clip and private VOD surfaces have complete translation and locale coverage', () => {
  for (const file of ['ClipCommandSection', 'PrivateVodSection']) {
    assert.deepEqual(checkFile(`src/components/verwaltung/${file}.tsx`), []);
  }
});

const handlers = new URL('../../../rust/crates/tb-dashboard-api/src/handlers/', import.meta.url);
const clipSource = readFileSync(new URL('clip_command_settings.rs', handlers), 'utf8');
const privateSource = readFileSync(new URL('private_vod.rs', handlers), 'utf8').split('pub async fn download_handler')[0];
const clipErrors = [...clipSource.matchAll(/"error":\s*"([^"]+)"/g)].map(match => match[1]).filter(key => key !== 'db');
const privateErrors = [...privateSource.matchAll(/"([^"]+)"/g)].map(match => match[1]).filter(key => /^(Bitte melde|Für diesen Download|Die VOD-Ausschnitte|Die Einstellung konnte)/.test(key));

for (const [surface, keys, request] of [
  ['clip', clipErrors, fetchClipCommandSettings],
  ['private-list', privateErrors, fetchPrivateVodClips],
  ['private-settings', privateErrors, () => setPrivateVodCommand(true)],
] as const) {
  test(`${surface} producer errors use the shared bilingual API dictionary`, async () => {
    assert.ok(keys.length > 0);
    for (const key of new Set(keys)) {
      const english = dictionaryFor('en')[key];
      assert.ok(english && english !== key, key);
      for (const language of ['de', 'en'] as const) {
        setActiveLanguage(language);
        globalThis.fetch = async () => new Response(JSON.stringify({ error: key }), { status: 403 });
        await assert.rejects(request, error => error instanceof Error && error.message === translate(language, key));
      }
    }
  });
}

test('private VOD range, expiry and size templates retain their parameters in both languages', () => {
  for (const key of ['{start} bis {end}', 'MP4 herunterladen: {start} bis {end}', 'Verfügbar bis {date}', ' · {size} MiB']) {
    assert.ok(dictionaryFor('en')[key]);
    const placeholders = [...key.matchAll(/\{(\w+)\}/g)].map(match => match[1]);
    for (const language of ['de', 'en'] as const) {
      const params = Object.fromEntries(placeholders.map(name => [name, `fixture-${name}`]));
      const output = translate(language, key, params);
      for (const value of Object.values(params)) assert.ok(output.includes(value));
      assert.doesNotMatch(output, /\{\w+\}/);
    }
  }
});

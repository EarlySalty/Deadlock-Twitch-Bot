import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { LanguageProvider } from '../src/context/LanguageContext';
import { dictionaryFor, LANGUAGE_STORAGE_KEY } from '../src/i18n/dictionary';

Object.assign(globalThis, { React });
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  writable: true,
  value: { location: new URL('https://fixture.invalid/twitch/social-media') },
});
const { ImportProgress } = await import('../src/components/socialmedia/autopilot/import-pool/ImportProgress');
const { createImportPoolFixtureJob } = await import('../src/components/socialmedia/autopilot/import-pool/fixtures');
const producer = readFileSync(new URL('../../../rust/crates/tb-social-media/src/clip_pool/worker.rs', import.meta.url), 'utf8');
const incompleteError = producer.match(/THEN '([^']+)' END/)?.[1];
const retryErrors = [...producer.matchAll(/"((?:Twitch |Die Twitch-|Der Twitch-|Die Clipseite )[^"\n]+)"/g)].map((match) => match[1]);
assert.ok(incompleteError);
assert.equal(retryErrors.length, 5);

for (const language of ['de', 'en'] as const) {
  for (const [index, last_error] of [incompleteError, ...retryErrors].entries()) {
    test(`ImportProgress renders production error ${index + 1} through the ${language} language provider`, () => {
      const original = window;
      Object.assign(globalThis, {
        window: {
          location: original.location,
          localStorage: { getItem: (key: string) => key === LANGUAGE_STORAGE_KEY ? language : null },
        },
      });
      try {
        const en = dictionaryFor('en');
        assert.ok(Object.hasOwn(en, last_error), `Missing production error: ${last_error}`);
        assert.notEqual(en[last_error], last_error);
        const job = createImportPoolFixtureJob({ status: index === 0 ? 'incomplete' : 'error', last_error });
        const html = renderToStaticMarkup(<LanguageProvider><ImportProgress job={job} /></LanguageProvider>);
        const expected = renderToStaticMarkup(<p className="import-pool-warning">{language === 'de' ? last_error : en[last_error]}</p>);
        assert.ok(html.includes(expected));
      } finally {
        Object.assign(globalThis, { window: original });
      }
    });
  }
}

import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  DEFAULT_LANGUAGE,
  LANGUAGE_STORAGE_KEY,
  browserLanguage,
  dictionaryFor,
  isLanguage,
  readInitialLanguage,
  readStoredLanguage,
  storeLanguage,
  translate,
} from '../src/i18n/dictionary';

test('Deutsch ist die Standardsprache und laesst Texte unveraendert', () => {
  assert.equal(DEFAULT_LANGUAGE, 'de');
  assert.equal(translate('de', 'Einstellungen'), 'Einstellungen');
  assert.equal(dictionaryFor('de').Einstellungen, undefined);
});

test('Englisch uebersetzt bekannte Texte', () => {
  assert.equal(translate('en', 'Einstellungen'), 'Settings');
  assert.equal(translate('en', 'Verbindungen'), 'Connections');
  assert.equal(translate('en', 'VOD-Archiv'), 'VOD archive');
});

test('Fehlende Uebersetzung faellt auf den deutschen Text zurueck', () => {
  const unknown = 'Ein Satz, der nie uebersetzt wurde.';
  assert.equal(translate('en', unknown), unknown);
  assert.notEqual(translate('en', unknown), '');
});

test('Platzhalter werden in beiden Sprachen ersetzt', () => {
  assert.equal(translate('de', '{count} Treffer', { count: 3 }), '3 Treffer');
  assert.equal(translate('en', '{count} Treffer', { count: 3 }), '3 results');
  assert.equal(translate('en', 'Status: {state}', {}), 'Status: {state}');
});

test('Kein englischer Eintrag ist leer', () => {
  for (const [key, value] of Object.entries(dictionaryFor('en'))) {
    assert.ok(value.trim().length > 0, `leerer Eintrag fuer ${key}`);
  }
});

test('Platzhalter stimmen zwischen Deutsch und Englisch ueberein', () => {
  const names = (text: string) =>
    [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  for (const [key, value] of Object.entries(dictionaryFor('en'))) {
    assert.deepEqual(names(value), names(key), `Platzhalter weichen ab: ${key}`);
  }
});

test('Sprachwahl ueberlebt einen Neuladen und gilt bei Muell als nicht gemerkt', () => {
  const store = new Map<string, string>();
  const original = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = {
    localStorage: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    },
  };
  try {
    assert.equal(readStoredLanguage(), null);
    storeLanguage('en');
    assert.equal(store.get(LANGUAGE_STORAGE_KEY), 'en');
    assert.equal(readStoredLanguage(), 'en');
    store.set(LANGUAGE_STORAGE_KEY, 'klingonisch');
    assert.equal(readStoredLanguage(), null);
  } finally {
    (globalThis as { window?: unknown }).window = original;
  }
});

test('Gesperrter Speicher kippt die Oberflaeche nicht', () => {
  const original = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = {
    localStorage: {
      getItem() {
        throw new Error('SecurityError');
      },
      setItem() {
        throw new Error('SecurityError');
      },
    },
  };
  try {
    assert.equal(readStoredLanguage(), null);
    assert.doesNotThrow(() => storeLanguage('en'));
  } finally {
    (globalThis as { window?: unknown }).window = original;
  }
});

test('isLanguage erkennt nur gepflegte Sprachen', () => {
  assert.ok(isLanguage('de'));
  assert.ok(isLanguage('en'));
  assert.ok(!isLanguage('fr'));
  assert.ok(!isLanguage(null));
  assert.equal(LANGUAGE_STORAGE_KEY, 'dashboard.language');
});

function withBrowser(
  navigator: { languages?: string[]; language?: string } | undefined,
  stored: string | null,
  run: () => void,
) {
  const original = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = {
    navigator,
    localStorage: {
      getItem: (key: string) => (key === LANGUAGE_STORAGE_KEY ? stored : null),
      setItem: () => {},
    },
  };
  try {
    run();
  } finally {
    (globalThis as { window?: unknown }).window = original;
  }
}

test('Browsersprache wird erkannt, während der unvollständige Release Deutsch beibehält', () => {
  withBrowser({ languages: ['de-AT', 'en'] }, null, () => assert.equal(browserLanguage(), 'de'));
  withBrowser({ languages: ['en-US', 'de-DE'] }, null, () => {
    assert.equal(browserLanguage(), 'en');
    assert.equal(readInitialLanguage(), DEFAULT_LANGUAGE);
  });
  withBrowser({ languages: [], language: 'de' }, null, () => assert.equal(browserLanguage(), 'de'));
  withBrowser({ language: 'fr-FR' }, null, () => assert.equal(browserLanguage(), 'en'));
  withBrowser({}, null, () => assert.equal(browserLanguage(), 'en'));
});

test('Manuell gewaehlte Sprache schlaegt die Browsersprache', () => {
  withBrowser({ languages: ['en-US'] }, 'de', () => assert.equal(readInitialLanguage(), 'de'));
  withBrowser({ languages: ['de-DE'] }, 'en', () => assert.equal(readInitialLanguage(), 'en'));
  withBrowser({ languages: ['de-DE'] }, 'klingonisch', () => assert.equal(readInitialLanguage(), 'de'));
});

test('Ohne Browser bleibt Deutsch', () => {
  withBrowser(undefined, null, () => assert.equal(readInitialLanguage(), DEFAULT_LANGUAGE));
});

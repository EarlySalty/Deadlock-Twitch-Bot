import { test } from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import { LanguageProvider, useT } from '../src/context/LanguageContext';
import { LANGUAGE_STORAGE_KEY, translate } from '../src/i18n/dictionary';

function Probe() {
  const t = useT();
  return (
    <ul>
      <li>{t('Einstellungen')}</li>
      <li>{t('VOD-Archiv')}</li>
      <li>{t('Ein Satz ohne Uebersetzung')}</li>
      <li>{t('{count} Treffer', { count: 2 })}</li>
    </ul>
  );
}

function withStoredLanguage(
  value: string | null,
  run: () => void,
  navigator?: { languages: string[] },
) {
  const original = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = {
    navigator,
    localStorage: {
      getItem: (key: string) => (key === LANGUAGE_STORAGE_KEY ? value : null),
      setItem: () => {},
    },
  };
  try {
    run();
  } finally {
    (globalThis as { window?: unknown }).window = original;
  }
}

test('Provider liefert ohne gespeicherte Wahl Deutsch', () => {
  withStoredLanguage(null, () => {
    const html = renderToStaticMarkup(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );
    assert.match(html, /<li>Einstellungen<\/li>/);
    assert.match(html, /<li>VOD-Archiv<\/li>/);
    assert.match(html, /<li>2 Treffer<\/li>/);
  });
});

test('Gespeichertes Englisch schlaegt bis in die Komponenten durch', () => {
  withStoredLanguage('en', () => {
    const html = renderToStaticMarkup(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );
    assert.match(html, /<li>Settings<\/li>/);
    assert.match(html, /<li>VOD archive<\/li>/);
    assert.match(html, /<li>2 results<\/li>/);
    assert.match(html, /<li>Ein Satz ohne Uebersetzung<\/li>/);
  });
});

test('Englische Browsersprache ohne gespeicherte Wahl bleibt während der Teilintegration Deutsch', () => {
  withStoredLanguage(
    null,
    () => {
      const html = renderToStaticMarkup(
        <LanguageProvider>
          <Probe />
        </LanguageProvider>,
      );
      for (const text of ['Einstellungen', 'VOD-Archiv']) {
        assert.ok(html.includes(`<li>${translate('de', text)}</li>`));
      }
      assert.ok(html.includes(`<li>${translate('de', '{count} Treffer', { count: 2 })}</li>`));
    },
    { languages: ['en-US'] },
  );
});

test('Gespeichertes Deutsch schlaegt englische Browsersprache', () => {
  withStoredLanguage(
    'de',
    () => {
      const html = renderToStaticMarkup(
        <LanguageProvider>
          <Probe />
        </LanguageProvider>,
      );
      assert.match(html, /<li>Einstellungen<\/li>/);
    },
    { languages: ['en-US'] },
  );
});

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
      <li>{t('Verwaltung')}</li>
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

for (const [stored, browser, expected] of [
  [null, 'en-US', 'en'],
  ['de', 'en-US', 'de'],
  ['en', 'de-DE', 'en'],
] as const) {
  test(`Provider resolves stored ${stored} and browser ${browser} as ${expected}`, () => {
    withStoredLanguage(
      stored,
      () => {
        const html = renderToStaticMarkup(
          <LanguageProvider>
            <Probe />
          </LanguageProvider>,
        );
        for (const text of ['Einstellungen', 'VOD-Archiv', 'Verwaltung']) {
          assert.ok(html.includes(`<li>${translate(expected, text)}</li>`));
        }
      },
      { languages: [browser] },
    );
  });
}

test('Scope checker renders its catalog and changelog in the selected language', async () => {
  const { AuthScopes } = await import('../src/pages/AuthScopes');
  const { ScopeProvider } = await import('../src/hooks/useScopes');
  for (const language of ['de', 'en'] as const) {
    withStoredLanguage(language, () => {
      Object.assign(window, { location: { search: '' } });
      const html = renderToStaticMarkup(<LanguageProvider><ScopeProvider><AuthScopes /></ScopeProvider></LanguageProvider>);
      assert.ok(html.includes(translate(language, 'Abdeckung')));
      assert.ok(html.includes(translate(language, 'Raids direkt aus Deadlock vorbereiten und auslösen.')));
      assert.ok(html.includes(translate(language, 'Basis-Auth entschlackt')));
      if (language === 'en') {
        assert.doesNotMatch(html, /Vergleicht die|Kritisch fehlen|Platzhalter-Einträge|Fehlt/);
        assert.match(html, />Missing</);
        assert.match(html, /Mar 18, 2026/);
      } else {
        assert.match(html, />Fehlt</);
      }
    });
  }
});

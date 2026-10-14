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
import { EN_AREAS } from '../src/i18n/en';

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
  const names = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  for (const [key, value] of Object.entries(dictionaryFor('en'))) {
    assert.deepEqual(names(value), names(key), `Platzhalter weichen ab: ${key}`);
  }
});

test('Sprachwahl ueberlebt einen Neuladen und faellt bei Muell auf Deutsch', () => {
  const store = new Map<string, string>();
  const original = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = {
    localStorage: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    },
  };
  try {
    assert.equal(readStoredLanguage(), 'de');
    storeLanguage('en');
    assert.equal(store.get(LANGUAGE_STORAGE_KEY), 'en');
    assert.equal(readStoredLanguage(), 'en');
    store.set(LANGUAGE_STORAGE_KEY, 'klingonisch');
    assert.equal(readStoredLanguage(), 'de');
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
    assert.equal(readStoredLanguage(), 'de');
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
  navigator: { languages?: readonly string[]; language?: string } | undefined,
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

test('Existing area dictionaries are available without changing German', () => {
  const dictionary = dictionaryFor('en');
  for (const area of Object.values(EN_AREAS)) {
    for (const key of Object.keys(area)) {
      assert.ok(Object.hasOwn(dictionary, key), `Missing area entry: ${key}`);
      assert.equal(translate('de', key), key);
    }
  }
  for (const key of ['Verwaltung', 'Kritik & Wünsche', 'Montag']) {
    assert.equal(translate('en', key), EN_AREAS.shell[key]);
  }
});

test('Browser default follows the primary language when no choice is stored', () => {
  for (const [navigator, expected] of [
    [{ languages: ['de-AT', 'en-US'] }, 'de'],
    [{ languages: ['en-US', 'de-DE'] }, 'en'],
    [{ languages: [], language: 'de' }, 'de'],
    [{ language: 'fr-FR' }, 'en'],
    [{}, 'en'],
    [undefined, DEFAULT_LANGUAGE],
  ] as const) {
    withBrowser(navigator, null, () => {
      assert.equal(browserLanguage(), expected);
      assert.equal(readInitialLanguage(), expected);
    });
  }
});

test('A persisted choice overrides the browser language', () => {
  withBrowser({ languages: ['en-US'] }, 'de', () => assert.equal(readInitialLanguage(), 'de'));
  withBrowser({ languages: ['de-DE'] }, 'en', () => assert.equal(readInitialLanguage(), 'en'));
  withBrowser({ languages: ['en-US'] }, 'invalid', () => assert.equal(readInitialLanguage(), 'en'));
});

test('Blocked storage still allows the browser default', () => {
  withBrowser({ languages: ['en-US'] }, null, () => {
    Object.defineProperty(window, 'localStorage', {
      get() {
        throw new Error('SecurityError');
      },
    });
    assert.equal(readInitialLanguage(), 'en');
    assert.doesNotThrow(() => storeLanguage('de'));
  });
});

test('Without a browser the initial language remains German', () => {
  const original = (globalThis as { window?: unknown }).window;
  try {
    delete (globalThis as { window?: unknown }).window;
    assert.equal(readInitialLanguage(), DEFAULT_LANGUAGE);
  } finally {
    (globalThis as { window?: unknown }).window = original;
  }
});

test('Priority page translations retain commands and cover feedback statuses', () => {
  for (const label of ['Eingegangen', 'In Prüfung', 'Geplant', 'In Arbeit', 'Umgesetzt', 'Derzeit nicht vorgesehen', 'Beantwortet']) {
    assert.notEqual(translate('en', label), label);
    assert.equal(translate('de', label), label);
  }
  const summary = 'Abo-Link für diesen Kanal. Freiwillige Erinnerung: !sub erinnerung an/aus/status.';
  assert.equal(translate('en', summary), 'Subscription link for this channel. Optional reminder: !sub erinnerung an/aus/status.');
  assert.equal(translate('de', summary), summary);
});

test('Ad-manager helper messages translate with locale-aware numbers and keep German defaults', async () => {
  const { budgetVorschau, statusSatz } = await import('../src/components/verwaltung/adManagerTexte');
  const t = translate.bind(null, 'en');
  assert.equal(budgetVorschau(3, null), '6 Blöcke à 30 Sek., etwa alle 10 Min.');
  assert.equal(budgetVorschau(3, null, t, 'en-US'), '6 blocks of 30 sec., about every 10 min.');
  const state = { enabled: true, isLive: true, currentReason: 'in_match', nextBlockLabel: 'in 5 min.', fit: null };
  assert.equal(statusSatz(state, t), "Running. Next block in 5 min. You're in a match. The bot uses available Twitch snoozes and won't start ads itself.");
});

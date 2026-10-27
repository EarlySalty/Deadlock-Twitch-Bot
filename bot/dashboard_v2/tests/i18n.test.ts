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
import { readFileSync } from 'node:fs';

test('Demo overview insights and actions translate the actual Rust snapshot copy', () => {
  const source = readFileSync(new URL('../../../rust/crates/tb-dashboard-api/src/handlers/demo.rs', import.meta.url), 'utf8');
  const fields = source.match(/"findings": (\[[\s\S]*?\]),\s*"actions": (\[[\s\S]*?\])/);
  assert.ok(fields, 'Demo overview findings and actions must be present');
  const findings = JSON.parse(fields[1]) as { title: string; text: string }[];
  const actions = JSON.parse(fields[2]) as { tag: string; text: string }[];
  assert.equal(findings.length, 2);
  assert.equal(actions.length, 1);
  for (const text of [
    ...findings.flatMap(finding => [finding.title, finding.text]),
    ...actions.flatMap(action => [action.tag, action.text]),
  ]) {
    assert.notEqual(translate('en', text), text, `Missing English demo copy: ${text}`);
    assert.equal(translate('de', text), text);
  }
  assert.equal(translate('en', findings[0].title), 'Strong retention');
  assert.equal(translate('en', actions[0].text), 'Put patch keywords near the start of the title.');
});

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
  assert.equal(LANGUAGE_STORAGE_KEY, 'dashboard.language.v2');
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

test('Browser default is German when any preferred language is German', () => {
  for (const [navigator, expected] of [
    [{ languages: ['de-AT', 'en-US'] }, 'de'],
    [{ languages: ['en-US', 'de-DE'] }, 'de'],
    [{ languages: ['en-US', 'fr-FR'] }, 'en'],
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
  const state = { enabled: true, isLive: true, currentReason: 'in_match', nextBlockLabel: t('in {value0} Min.', { value0: 5 }), fit: null };
  assert.equal(statusSatz(state, t), "Running. Next block in 5 min. You're in a match. The bot uses available Twitch snoozes and won't start ads itself.");
  assert.equal(statusSatz({ ...state, nextBlockLabel: 'now' }, t), "Running. Next block now. You're in a match. The bot uses available Twitch snoozes and won't start ads itself.");
});

test('Named raid history translates without changing channel names or German defaults', async () => {
  const { beschreibeEintrag } = await import('../src/components/verwaltung/adManagerTexte');
  const entry = { reason: 'recent_raid', detail: 'example_channel', decision: 'none' } as Parameters<typeof beschreibeEintrag>[0];
  assert.equal(beschreibeEintrag(entry), 'Verschoben, Raid von example_channel');
  assert.equal(beschreibeEintrag(entry, translate.bind(null, 'en')), 'Postponed, raid from example_channel');
});

test('Connected accounts and unread requests use English status labels', () => {
  for (const [source, english] of [['Verbunden', 'Connected'], ['Ungelesen', 'Unread'], ['Wunsch', 'Request']]) {
    assert.equal(translate('en', source), english);
    assert.equal(translate('de', source), source);
  }
});

test('D6 translates generated coaching and quest copy without changing German', async () => {
  const { translatePageText, translateGoalRoute } = await import('../src/i18n/pageText');
  const en = (text: string, params?: Record<string, string | number>) => translate('en', text, params);
  const de = (text: string, params?: Record<string, string | number>) => translate('de', text, params);
  const source = 'Deine Viewer-Hours pro Stream-Hour (1234.5) liegen unter dem 25. Perzentil. Der Kategorie-Durchschnitt liegt bei 2500.';
  assert.equal(translatePageText(en, source, 'en-US'), 'Your viewer hours per stream hour (1,234.5) are below the 25th percentile. The category average is 2,500.');
  assert.equal(translatePageText(de, source, 'de-DE'), source);
  assert.equal(translatePageText(en, 'Je laenger du streamst, desto weniger Viewer hast du (r=0.004). Das deutet auf Ermuedung hin.', 'en-US'), 'The longer you stream, the fewer viewers you have (r=0.004). This suggests fatigue.');
  assert.equal(translatePageText(en, 'Streame 45 Minuten länger als dein 4-Wochen-Mittel von 1 Minute'), 'Stream 45 minutes longer than your 4-week average of 1 minute');
  assert.equal(translateGoalRoute(en, 'Noch 1 geworbener Partner und 2 Clip-Einreichungen', 'en-US'), '1 referred partner and 2 clip submissions more');
  assert.equal(translateGoalRoute(de, 'Noch 1 geworbener Partner und 2 Clip-Einreichungen', 'de-DE'), 'Noch 1 geworbener Partner und 2 Clip-Einreichungen');
  assert.equal(translatePageText(en, 'Erfolgreiche Streamer nutzen Keywords, die du nicht verwendest: Etabliert.'), 'Successful streamers use keywords you are not using: Etabliert.');
  assert.equal(translatePageText(en, 'A user-written title: Grüße & Spaß'), 'A user-written title: Grüße & Spaß');
  for (const text of ['Etabliert', 'Beste Streaming-Zeiten', 'Stream-Aktivität', 'Scores von 0 bis 100 je Bereich']) {
    assert.notEqual(en(text), text);
    assert.equal(de(text), text);
  }
});

import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  DEFAULT_LANGUAGE,
  LANGUAGE_STORAGE_KEY,
  dictionaryFor,
  isLanguage,
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

// Der deutsche Text ist selbst der Schluessel: eine fehlende Uebersetzung
// faellt auf Deutsch zurueck, nie auf einen Schluesselnamen oder Leerstring.
test('Fehlende Uebersetzung faellt auf den deutschen Text zurueck', () => {
  const unknown = 'Ein Satz, der nie uebersetzt wurde.';
  assert.equal(translate('en', unknown), unknown);
  assert.notEqual(translate('en', unknown), '');
});

test('Platzhalter werden in beiden Sprachen ersetzt', () => {
  assert.equal(translate('de', '{count} Treffer', { count: 3 }), '3 Treffer');
  assert.equal(translate('en', '{count} Treffer', { count: 3 }), '3 results');
  // Unbekannter Platzhalter bleibt stehen, statt ein Loch zu hinterlassen.
  assert.equal(translate('en', 'Status: {state}', {}), 'Status: {state}');
});

test('Kein englischer Eintrag ist leer', () => {
  for (const [key, value] of Object.entries(dictionaryFor('en'))) {
    assert.ok(value.trim().length > 0, `leerer Eintrag fuer ${key}`);
  }
});

// Ein Platzhalter, der nur auf einer Seite steht, waere im Betrieb ein Loch
// oder eine tote Variable. Deshalb muessen beide Seiten dieselben tragen.
test('Platzhalter stimmen zwischen Deutsch und Englisch ueberein', () => {
  const names = (text: string) =>
    [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  for (const [key, value] of Object.entries(dictionaryFor('en'))) {
    assert.deepEqual(names(value), names(key), `Platzhalter weichen ab: ${key}`);
  }
});

test('isLanguage erkennt nur gepflegte Sprachen', () => {
  assert.ok(isLanguage('de'));
  assert.ok(isLanguage('en'));
  assert.ok(!isLanguage('fr'));
  assert.ok(!isLanguage(null));
  assert.equal(LANGUAGE_STORAGE_KEY, 'dashboard.language');
});

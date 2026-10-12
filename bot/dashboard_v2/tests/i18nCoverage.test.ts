import { test } from 'node:test';
import assert from 'node:assert/strict';

import { checkSource, coverageReport, pendingFiles, scopedFiles } from '../tools/i18nCoverage';
import { dictionaryFor } from '../src/i18n/dictionary';
import { EN_AREAS } from '../src/i18n/en';

test('Übersetzte Dateien enthalten keine sichtbaren deutschen Reste', () => {
  const pending = pendingFiles();
  const files = scopedFiles().filter((file) => !pending.has(file));
  const violations = [...coverageReport(files).values()].flat();
  assert.deepEqual(
    violations.map((v) => `${v.file}:${v.line} [${v.rule}] ${v.text}`),
    [],
  );
});

test('Offene Dateien stehen nur auf der Liste, solange sie Reste haben', () => {
  const scoped = new Set(scopedFiles());
  const pending = [...pendingFiles()];
  const unknown = pending.filter((file) => !scoped.has(file));
  assert.deepEqual(unknown, [], 'Einträge ohne passende Datei');
  const report = coverageReport(pending.filter((file) => scoped.has(file)));
  const done = [...report].filter(([, violations]) => violations.length === 0).map(([file]) => file);
  assert.deepEqual(done, [], 'Diese Dateien sind fertig und gehören von der Liste');
});

test('Bereichswörterbücher widersprechen sich nicht', () => {
  const merged = dictionaryFor('en');
  const conflicts: string[] = [];
  for (const [area, entries] of Object.entries(EN_AREAS)) {
    for (const [key, value] of Object.entries(entries)) {
      if (merged[key] !== value) conflicts.push(`${area}: ${key} → ${value} / ${merged[key]}`);
    }
  }
  assert.deepEqual(conflicts, []);
});

test('Prüfer erkennt sichtbare deutsche Texte und fehlende Übersetzungen', () => {
  const source = [
    "const tabs = [{ id: 'a', label: 'Zuschauer' }];",
    "export function A({ t }: { t: (s: string) => string }) {",
    "  return (",
    "    <div title=\"Mehr Infos\" className=\"flex gap-2\">",
    "      Hallo Welt",
    "      {t('Gibt es nicht im Wörterbuch')}",
    "      {ok ? 'Ja' : 'Nein'}",
    "      {new Date().toLocaleDateString('de-DE')}",
    "      {(1).toLocaleString()}",
    "    </div>",
    "  );",
    "}",
  ].join('\n');
  const rules = checkSource('src/pages/Probe.tsx', source, {}).map((v) => `${v.rule}:${v.text}`);
  assert.deepEqual(rules.sort(), [
    'browser-locale:(1).toLocaleString()',
    'fixed-locale:de-DE',
    'jsx-attribute:Mehr Infos',
    'jsx-expression:Ja',
    'jsx-expression:Nein',
    'jsx-text:Hallo Welt',
    'missing-translation:Gibt es nicht im Wörterbuch',
    'visible-property:Zuschauer',
  ]);
});

test('Prüfer lässt übersetzte und unsichtbare Texte in Ruhe', () => {
  const source = [
    "import x from './für-später';",
    "type Modus = 'tägliche Ansicht';",
    "export function A({ t, mode }: { t: (s: string) => string; mode: string }) {",
    "  if (mode === 'läuft') return null;",
    "  return <div className=\"Große Box\" data-info=\"Über uns\">{t('Einstellungen')} Twitch</div>;",
    "}",
  ].join('\n');
  assert.deepEqual(checkSource('src/pages/Probe.tsx', source, dictionaryFor('en')), []);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { buildCategoryTrend, categoryArchiveLabel } from '../src/pages/categoryCollectorViewModel';

test('Stundenwerte bleiben unverändert statt zu einem falschen Tagesmittel zu werden', () => {
  const points = [0, 0, 100].map((streams, index) => ({ at: `2026-09-18T0${index}:00:00Z`, streams, viewers: streams, polls: index + 1 }));
  const result = buildCategoryTrend(points);
  assert.deepEqual(result.map(row => row.streams), [0, 0, 100]);
  assert.deepEqual(result.map(row => row.polls), [1, 2, 3]);
});

test('Unsortierte Zeitpunkte, ungültige Daten und Lücken werden sicher behandelt', () => {
  const result = buildCategoryTrend([
    { at: '2026-09-18T03:00:00Z', streams: null, viewers: 50 },
    { at: 'kaputt', streams: 999, viewers: 999 },
    { at: '2026-09-18T00:00:00Z', streams: 0, viewers: 0 },
  ]);
  assert.equal(result.length, 3);
  assert.equal(result[0].streams, 0);
  assert.equal(result[1].streams, null);
  assert.equal(result[2].streams, null);
  assert.equal(result[2].timestamp - result[0].timestamp, 10_800_000);
});

test('Kein verstecktes 90-Tage-Versprechen oder ungeprüfter Archivstatus', () => {
  assert.match(categoryArchiveLabel(true), /Keine automatische Löschung/);
  assert.match(categoryArchiveLabel(undefined), /noch nicht bestätigt/);
  assert.match(categoryArchiveLabel(false), /noch nicht bestätigt/);
  const source = readFileSync(new URL('../src/pages/CategoryCollector.tsx', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /retention_days\s*\?\?\s*90|vorzeitig entfernt|Reguläre Aufbewahrung: maximal/);
  assert.match(source, /buildCategoryTrend/);
  assert.match(source, /categoryArchiveLabel/);
});

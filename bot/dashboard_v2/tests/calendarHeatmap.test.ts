import assert from 'node:assert/strict';
import test from 'node:test';
import type { CalendarHeatmapData } from '../src/types/analytics';
import { buildCalendarHeatmap } from '../src/utils/calendarHeatmap';

const NOW = new Date('2026-09-18T14:00:00Z');
function entry(date: string, streamCount = 1, hoursWatched = 10): CalendarHeatmapData {
  return { date, streamCount, hoursWatched, value: hoursWatched };
}

test('3650 Tage behalten den gesamten Zeitraum in höchstens 132 Monatsfeldern', () => {
  const model = buildCalendarHeatmap([], 3650, 'hoursWatched', NOW);
  assert.equal(model.resolution, 'month');
  assert.equal(model.startDate, '2016-09-21');
  assert.equal(model.endDate, '2026-09-18');
  assert.deepEqual(model.years.map(row => row.year), Array.from({ length: 11 }, (_, i) => 2026 - i));
  assert.equal(model.years.flatMap(row => row.months).filter(Boolean).length, 121);
  assert.equal(model.weeks.length, 0);
  assert.equal(model.years[0].months[9], null, 'Zukünftige Monate bleiben außerhalb des Rasters');
  assert.equal(model.years.at(-1)?.months[7], null, 'Monate vor dem Filter werden nicht als leerer Monat ausgegeben');
});

test('Monatswerte summieren alle Tage und behalten getrennte Jahre und passende Farbskalen', () => {
  const data = [entry('2025-09-01', 2, 40), entry('2026-09-01', 3, 20), entry('2026-09-18', 4, 50)];
  const original = structuredClone(data);
  const hours = buildCalendarHeatmap(data, 730, 'hoursWatched', NOW);
  assert.equal(hours.totalStreams, 9);
  assert.equal(hours.maxValue, 70);
  assert.equal(hours.years[0].months[8]?.hoursWatched, 70);
  assert.equal(hours.years[0].months[8]?.streamCount, 7);
  assert.equal(hours.years[1].months[8]?.hoursWatched, 40);
  assert.equal(buildCalendarHeatmap(data, 730, 'streamCount', NOW).maxValue, 7);
  assert.deepEqual(data, original);
});

test('Angebrochene Randmonate filtern Daten und zeigen ihre wirklichen Grenzen', () => {
  const model = buildCalendarHeatmap([
    entry('2016-09-20', 100), entry('2016-09-21', 2), entry('2026-09-18', 3), entry('2026-09-19', 100),
  ], 3650, 'streamCount', NOW);
  assert.equal(model.totalStreams, 5);
  assert.equal(model.maxValue, 3);
  assert.equal(model.years.at(-1)?.months[8]?.startDate, '2016-09-21');
  assert.equal(model.years.at(-1)?.months[8]?.endDate, '2016-09-30');
  assert.equal(model.years[0].months[8]?.endDate, '2026-09-18');
});

test('Kurze und einjährige Zeiträume bleiben täglich und polstern nur mit nicht auswählbaren Leerfeldern', () => {
  for (const days of [1, 7, 30, 90, 365, 366]) {
    const model = buildCalendarHeatmap([], days, 'hoursWatched', NOW);
    assert.equal(model.resolution, 'day');
    assert.equal(model.weeks.flat().filter(Boolean).length, days);
    assert.ok(model.weeks.length <= 54);
    assert.ok(model.weeks.every(week => week.length === 7));
    assert.ok(model.monthLabels.every(label => label.weekSpan >= 3));
  }
  assert.equal(buildCalendarHeatmap([], 367, 'hoursWatched', NOW).resolution, 'month');
});

test('Tageswerte und Footer zählen denselben Zeitraum; doppelte Datumseinträge werden addiert', () => {
  const model = buildCalendarHeatmap([
    entry('2026-09-11', 100), entry('2026-09-12', 2, 5), entry('2026-09-12', 3, 7),
    entry('2026-09-18', 4, 20), entry('2026-09-19', 100),
  ], 7, 'hoursWatched', NOW);
  const cells = model.weeks.flat().filter(cell => cell !== null);
  assert.equal(cells[0].key, '2026-09-12');
  assert.equal(cells[0].streamCount, 5);
  assert.equal(cells[0].hoursWatched, 12);
  assert.equal(model.totalStreams, 9);
  assert.equal(model.maxValue, 20);
  assert.equal(model.weeks[0][0], null);
});

test('UTC-Datumsschlüssel bleiben über Sommerzeitwechsel und den Schalttag exakt', () => {
  for (const [now, keys] of [
    ['2026-03-30T00:30:00Z', ['2026-03-28', '2026-03-29', '2026-03-30']],
    ['2024-03-01T00:30:00Z', ['2024-02-28', '2024-02-29', '2024-03-01']],
    ['2026-01-01T00:30:00Z', ['2025-12-30', '2025-12-31', '2026-01-01']],
  ] as const) {
    const model = buildCalendarHeatmap(keys.map(key => entry(key)), 3, 'streamCount', new Date(now));
    assert.deepEqual(model.weeks.flat().filter(cell => cell !== null).map(cell => cell.key), keys);
    assert.equal(model.totalStreams, 3);
  }
});

test('Ungültige und übergroße Zeiträume erzeugen keine unbeschränkten Raster', () => {
  assert.equal(buildCalendarHeatmap([], NaN, 'hoursWatched', NOW).days, 365);
  assert.equal(buildCalendarHeatmap([], Infinity, 'hoursWatched', NOW).days, 365);
  assert.equal(buildCalendarHeatmap([], -5, 'hoursWatched', NOW).days, 1);
  assert.equal(buildCalendarHeatmap([], 30.9, 'hoursWatched', NOW).days, 30);
  assert.equal(buildCalendarHeatmap([], 999999, 'hoursWatched', NOW).days, 3650);
});

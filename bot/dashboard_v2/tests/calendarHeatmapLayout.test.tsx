import assert from 'node:assert/strict';
import test from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { CalendarHeatmap } from '../src/components/heatmaps/CalendarHeatmap';

test('3650 Tage werden als Monatsraster mit Jahreszeilen statt 522 winzigen Wochenspalten dargestellt', () => {
  const html = renderToStaticMarkup(createElement(CalendarHeatmap, { data: [], days: 3650 }));
  assert.ok(html.includes('data-resolution="month"'), 'Mehrjährige Zeiträume brauchen eine lesbare Monatsansicht');
  assert.equal((html.match(/data-calendar-year=/g) ?? []).length, 11);
  assert.ok(!/repeat\(52\d,/.test(html), 'Zehn Jahre dürfen nicht in eine einzige Wochenzeile gepresst werden');
  assert.ok(html.includes('0 Streams in den letzten 3650 Tagen'));
});

test('365 Tage behalten Tagesauflösung mit Mindestbreite und eigenem Scrollbereich', () => {
  const html = renderToStaticMarkup(createElement(CalendarHeatmap, { data: [], days: 365 }));
  assert.ok(html.includes('data-resolution="day"'));
  assert.ok(html.includes('overflow-x-auto'));
  assert.ok(html.includes('minmax(12px, 1fr)'));
  assert.ok(!html.includes('opacity:0'), 'Die einzelnen Kalenderfelder dürfen nicht erst durch tausende Animationen sichtbar werden');
});

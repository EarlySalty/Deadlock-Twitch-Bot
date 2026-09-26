import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  HEALTH_SCORE_METRICS,
  healthScoreBand,
  healthScoreBandRange,
} from '../src/utils/healthScoreContext';

test('Health-Score-Gewichte ergeben zusammen 100 Prozent', () => {
  const total = HEALTH_SCORE_METRICS.reduce((sum, metric) => sum + metric.weight, 0);
  assert.equal(total, 100);
});

test('Health-Score erklärt die vier tatsächlichen Signale verständlich', () => {
  const byKey = Object.fromEntries(HEALTH_SCORE_METRICS.map((metric) => [metric.key, metric]));

  assert.equal(byKey.growth.label, 'Wachstum');
  assert.match(byKey.growth.summary, /7 Tagen davor/);
  assert.match(byKey.growth.detail, /50 Punkte/);

  assert.equal(byKey.retention.label, 'Konstanz');
  assert.match(byKey.retention.detail, /nicht, wie lange einzelne Viewer/);

  assert.equal(byKey.engagement.label, 'Chat-Aktivität');
  assert.match(byKey.engagement.summary, /Chat-Nachrichten/);

  assert.equal(byKey.community.label, 'Stammcommunity');
  assert.match(byKey.community.detail, /Chat-Bots/);
});

test('Health-Score-Einordnung folgt den sichtbaren Schwellen 40 und 70', () => {
  assert.equal(healthScoreBand(0), 'Ausbaufähig');
  assert.equal(healthScoreBand(39), 'Ausbaufähig');
  assert.equal(healthScoreBand(40), 'Solide');
  assert.equal(healthScoreBand(69), 'Solide');
  assert.equal(healthScoreBand(70), 'Stark');
  assert.equal(healthScoreBand(100), 'Stark');

  assert.equal(healthScoreBandRange(39), '0 bis 39');
  assert.equal(healthScoreBandRange(40), '40 bis 69');
  assert.equal(healthScoreBandRange(70), '70 bis 100');
});

test('Kanal-Gesundheit zeigt Einordnung und Berechnung direkt im Dashboard', () => {
  const home = readFileSync(
    join(import.meta.dirname, '..', 'src', 'pages', 'InternalHomeLanding.tsx'),
    'utf8',
  );

  assert.match(home, /Kein Twitch-Ranking und kein Vergleich mit anderen Kanälen/);
  assert.match(home, /Ø Viewer/);
  assert.match(home, /gegenüber den 7 Tagen davor/);
  assert.match(home, /Wie kommt der Score zustande\?/);
  assert.match(home, /0 bis 39: ausbaufähig/);
});

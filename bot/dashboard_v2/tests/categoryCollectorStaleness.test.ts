import { strict as assert } from 'node:assert';
import test from 'node:test';

import {
  CATEGORY_HEARTBEAT_MAX_AGE_MS,
  collectorHeartbeatStale,
} from '../src/pages/categoryCollectorStaleness';

test('Collector-Heartbeat wird gegen die aktuelle Zeit statt gegen den alten Response-Zeitpunkt geprüft', () => {
  const heartbeat = '2026-09-25T18:00:00.000Z';
  const heartbeatMs = Date.parse(heartbeat);

  assert.equal(
    collectorHeartbeatStale(heartbeat, heartbeatMs + CATEGORY_HEARTBEAT_MAX_AGE_MS),
    false,
  );
  assert.equal(
    collectorHeartbeatStale(heartbeat, heartbeatMs + CATEGORY_HEARTBEAT_MAX_AGE_MS + 1),
    true,
  );
});

test('fehlende oder ungültige Heartbeats sind stale', () => {
  const now = Date.parse('2026-09-25T18:05:00.000Z');
  assert.equal(collectorHeartbeatStale(null, now), true);
  assert.equal(collectorHeartbeatStale('kein-datum', now), true);
});

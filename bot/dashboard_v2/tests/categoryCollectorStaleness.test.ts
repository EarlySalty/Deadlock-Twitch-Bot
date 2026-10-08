import { strict as assert } from 'node:assert';
import test from 'node:test';

import {
  CATEGORY_HEARTBEAT_MAX_AGE_MS,
  COLLECTOR_STATE_TEXT,
  collectorCoverageState,
  collectorHeartbeatStale,
  type CollectorCoverage,
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

test('fehlende oder ungültige Heartbeats sind veraltet', () => {
  const now = Date.parse('2026-09-25T18:05:00.000Z');
  assert.equal(collectorHeartbeatStale(null, now), true);
  assert.equal(collectorHeartbeatStale('kein-datum', now), true);
  assert.equal(collectorHeartbeatStale('2026-09-25T18:05:00.000Z', NaN), true);
});

const now = Date.parse('2026-10-08T12:00:00.000Z');
const current = '2026-10-08T12:00:00.000Z';
const old = '2026-10-08T11:50:00.000Z';
const healthy: CollectorCoverage = {
  heartbeat_at: current,
  last_snapshot: current,
  collector_config: { enabled: true, poll_seconds: 60 },
  status: { discovery_state: 'ok', last_discovery: current },
};

test('Zeitstempel weit in der Zukunft gelten nicht als aktueller Nachweis', () => {
  assert.equal(collectorHeartbeatStale(current, now - CATEGORY_HEARTBEAT_MAX_AGE_MS), false);
  assert.equal(collectorHeartbeatStale(current, now - CATEGORY_HEARTBEAT_MAX_AGE_MS - 1), true);
});

test('Deaktivierte Sammlung ist auch ohne Statusmeldung kein Ausfall', () => {
  const coverage = collectorCoverageState({ collector_config: { enabled: false, poll_seconds: 60 } }, now);
  assert.equal(coverage.state, 'disabled');
  assert.equal(coverage.liveChannelsCurrent, false);
});

test('Plattenpause bleibt bei aktuellem Heartbeat eine Pause trotz alter Messungen', () => {
  const coverage = collectorCoverageState({ ...healthy, last_snapshot: old, status: { ...healthy.status, disk_paused: true, raw_paused: true } }, now);
  assert.equal(coverage.state, 'disk_paused');
  assert.equal(coverage.heartbeatStale, false);
  assert.equal(coverage.snapshotsMissing, true);
  assert.equal(coverage.liveChannelsCurrent, false);
});

test('Rohchatbudget pausiert Chat, nicht die vorhandenen aktuellen Kategoriemessungen', () => {
  const coverage = collectorCoverageState({ ...healthy, status: { ...healthy.status, raw_paused: true } }, now);
  assert.equal(coverage.state, 'raw_paused');
  assert.equal(coverage.snapshotsMissing, false);
});

test('Speicherpause verdeckt keinen veralteten Heartbeat', () => {
  for (const status of [{ disk_paused: true }, { raw_paused: true }]) {
    const coverage = collectorCoverageState({ ...healthy, heartbeat_at: old, status: { ...healthy.status, ...status } }, now);
    assert.equal(coverage.state, 'heartbeat_stale');
    assert.equal(coverage.liveChannelsCurrent, false);
  }
});

test('Fehlende aktuelle Snapshots werden trotz lebendem Sammler erkannt', () => {
  for (const last_snapshot of [null, old, 'ungültig', '2026-10-08T13:00:00.000Z']) {
    const coverage = collectorCoverageState({ ...healthy, last_snapshot }, now);
    assert.equal(coverage.state, 'snapshots_missing');
    assert.equal(coverage.heartbeatStale, false);
  }
});

test('Rohchatpause verdeckt keine fehlenden Kategoriemessungen', () => {
  assert.equal(collectorCoverageState({ ...healthy, last_snapshot: old, status: { ...healthy.status, raw_paused: true } }, now).state, 'snapshots_missing');
});

test('Live-Kanäle brauchen einen aktuellen erfolgreichen Kategorieabruf', () => {
  for (const last_discovery of [null, old]) {
    const coverage = collectorCoverageState({ ...healthy, status: { discovery_state: 'ok', last_discovery } }, now);
    assert.equal(coverage.state, 'discovery_pending');
    assert.equal(coverage.liveChannelsCurrent, false);
  }
  const failed = collectorCoverageState({ ...healthy, status: { last_discovery: current, discovery_state: 'error' } }, now);
  assert.equal(failed.state, 'discovery_pending');
  assert.equal(failed.liveChannelsCurrent, false);
  assert.equal(collectorCoverageState(healthy, now).state, 'active');
});

test('Aktualität berücksichtigt den konfigurierten Messtakt bei unabhängigem Heartbeat', () => {
  const report = { ...healthy, collector_config: { enabled: true, poll_seconds: 300 } };
  assert.equal(collectorCoverageState({ ...report, heartbeat_at: '2026-10-08T12:10:00.000Z' }, now + 600_000).state, 'active');
  assert.equal(collectorCoverageState({ ...report, heartbeat_at: '2026-10-08T12:10:00.000Z' }, now + 600_001).state, 'snapshots_missing');
  assert.equal(collectorCoverageState(report, now + 120_001).state, 'heartbeat_stale');
  assert.equal(collectorCoverageState(healthy, now + 120_001).state, 'heartbeat_stale');
});

test('Ältere Payloads und ungültiger Messtakt bekommen keine erfundene Deaktivierung', () => {
  assert.equal(collectorCoverageState({ ...healthy, collector_config: undefined }, now).state, 'active');
  for (const poll_seconds of [0, -1, NaN, Infinity]) {
    assert.equal(collectorCoverageState({ ...healthy, collector_config: { enabled: true, poll_seconds } }, now + 120_001).state, 'heartbeat_stale');
  }
});

test('Jeder Betriebszustand hat einen gemeinsamen Anzeigetext', () => {
  for (const state of ['disabled', 'heartbeat_stale', 'disk_paused', 'snapshots_missing', 'raw_paused', 'discovery_pending', 'active'] as const) {
    assert.ok(COLLECTOR_STATE_TEXT[state].title.length > 0);
    assert.ok(COLLECTOR_STATE_TEXT[state].detail.length > 0);
  }
});

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  CHAT_AUDIENCE_TOOLTIP,
  normalizeHourlyActivity,
  resolveChatPenetration,
  resolveMessagesPer100ViewerMinutes,
} from '../src/utils/engagementKpi';

test('shows chat penetration value when reliable is true', () => {
  const result = resolveChatPenetration({
    chatPenetrationPct: 42.5,
    chatPenetrationReliable: true,
    dataQuality: { chattersCoverage: 0.4, passiveViewerSamples: 12 },
  });
  assert.equal(result.value, 42.5);
  assert.equal(result.reliable, true);
});

test('returns unreliable when only active chatters are available', () => {
  const result = resolveChatPenetration({
    chatPenetrationPct: 100,
    chatPenetrationReliable: false,
    dataQuality: { chattersCoverage: 0.0, passiveViewerSamples: 0 },
  });
  assert.equal(result.reliable, false);
});

test('tooltip clearly states chat audience scope', () => {
  assert.match(CHAT_AUDIENCE_TOOLTIP, /Chatters-API/i);
  assert.match(CHAT_AUDIENCE_TOOLTIP, /Video-Viewer/i);
});

test('normalizes missing/invalid hourly arrays without NaN bars', () => {
  const normalized = normalizeHourlyActivity(undefined);
  assert.equal(normalized.length, 24);
  for (const row of normalized) {
    assert.equal(Number.isFinite(row.count), true);
  }
});

test('legacy fields still resolve penetration and viewer-minute bridge', () => {
  const penetration = resolveChatPenetration({
    interactionRateActivePerViewer: 55,
    interactionRateReliable: true,
    dataQuality: { chattersApiCoverage: 0.3, passiveViewerSamples: 4 },
  });
  const bridge = resolveMessagesPer100ViewerMinutes({
    totalMessages: 1200,
    viewerMinutes: 600,
  });

  assert.equal(penetration.value, 55);
  assert.equal(penetration.reliable, true);
  assert.equal(bridge, 200);
});

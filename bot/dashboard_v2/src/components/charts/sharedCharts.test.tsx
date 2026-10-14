import assert from 'node:assert/strict';
import test from 'node:test';
import { createElement, type ReactElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { LanguageProvider } from '../../context/LanguageContext';
import { SessionTable } from '../tables/SessionTable';
import { CalendarHeatmap } from '../heatmaps/CalendarHeatmap';
import { FollowerFunnel } from './FollowerFunnel';
import { LurkerAnalysis } from './LurkerAnalysis';
import { RaidRetention } from './RaidRetention';
import { AudienceSharing } from './AudienceSharing';
import { WatchTimeDistribution } from './WatchTimeDistribution';
import type { StreamSession } from '../../types/analytics';

function render(language: 'de' | 'en', element: ReactElement): string {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: Object.assign(new EventTarget(), { location: { search: `?lang=${language}`, hostname: 'localhost' } }),
  });
  try {
    return renderToStaticMarkup(createElement(LanguageProvider, null, element));
  } finally {
    if (previous) Object.defineProperty(globalThis, 'window', previous);
    else Reflect.deleteProperty(globalThis, 'window');
  }
}

const session: StreamSession = {
  id: 1, date: '2026-10-14', startTime: '18:00:00', duration: 7200,
  startViewers: 100, peakViewers: 2345, endViewers: 100, avgViewers: 1234,
  retention5m: 65, retention10m: 65, retention20m: 65, dropoffPct: 35,
  totalChatterSessions: 3456, uniqueChatters: 3456, firstTimeChatters: 1234,
  returningChatters: 2222, followersStart: 1000, followersEnd: 1100,
  title: 'Synthetic stream', holdPct: 65,
};

for (const language of ['de', 'en'] as const) {
  const grouped = language === 'de' ? '1.234' : '1,234';
  test(`session chat counters and labels use ${language}`, () => {
    const html = render(language, createElement(SessionTable, { sessions: [session] }));
    assert.ok(html.includes(`(${grouped} ${language === 'de' ? 'neu' : 'new'})`));
    assert.ok(html.includes(language === 'de' ? '3.456' : '3,456'));
    assert.ok(html.includes(language === 'de' ? 'Datum' : 'Date'));
  });

  test(`funnel stage and source counters use ${language}`, () => {
    const html = render(language, createElement(FollowerFunnel, { data: {
      uniqueViewers: 12345, returningViewers: 4321, newFollowers: 1234,
      netFollowerDelta: 1200, conversionRate: 9.99, avgTimeToFollow: 18,
      followersBySource: { organic: 1234, raids: 0, hosts: 0, other: 0 },
    } }));
    assert.ok(html.includes(`+${grouped}`));
    assert.ok(html.includes(language === 'de' ? '12.345' : '12,345'));
    assert.ok(html.includes(language === 'de' ? '9,99%' : '9.99%'));
    assert.ok(html.includes(language === 'de' ? 'Von Viewer zu Follower' : 'From viewer to follower'));
    assert.ok(html.includes('flex-col sm:flex-row'));
    assert.ok(html.includes('flex-wrap items-center gap-2 mt-2'));
  });

  test(`lurker and raid counters use ${language}`, () => {
    const lurkers = render(language, createElement(LurkerAnalysis, { data: {
      dataAvailable: true,
      lurkerStats: { ratio: 0.8, avgSessions: 2.5, totalLurkers: 1234, totalViewers: 2345 },
      conversionStats: { rate: 0.2, converted: 1234, eligible: 2345 },
      regularLurkers: [{ login: 'synthetic_lurker', lurkSessions: 1234, firstSeen: null, lastSeen: '2026-10-14' }],
    } }));
    assert.ok(lurkers.includes(`${grouped} ${language === 'de' ? 'von' : 'of'}`));
    assert.ok(lurkers.includes(language === 'de' ? 'Reguläre Lurker' : 'Regular lurkers'));
    const raids = render(language, createElement(RaidRetention, { data: {
      dataAvailable: true,
      summary: { avgRetentionPct: 65, avgConversionPct: 20, totalNewChatters: 1234, raidCount: 1234 },
      raids: [{ raidId: 1, toBroadcaster: 'synthetic_partner', executedAt: '2026-10-14', viewersSent: 1234,
        chattersAt5m: 1234, chattersAt15m: null, chattersAt30m: 1234, retention30mPct: 65,
        newChatters: 1234, chatterConversionPct: 20, knownFromRaider: null }],
    } }));
    assert.ok(raids.includes(`${language === 'de' ? 'über' : 'across'} ${grouped} Raids`) || raids.includes(`across ${grouped} raids`));
    assert.ok(raids.includes(language === 'de' ? 'Ziel-Streamer suchen' : 'Search target streamer'));
  });

  test(`sharing and sample counters use ${language}`, () => {
    const sharing = render(language, createElement(AudienceSharing, { data: {
      dataAvailable: true, totalUniqueViewers: 1234,
      current: [{ streamer: 'synthetic_partner', sharedViewers: 1234, inflow: 1234, outflow: 1234, jaccardSimilarity: 0.4 }],
      timeline: [], dataQuality: { months: 1, minSharedFilter: 1 },
    } }));
    assert.ok(sharing.includes(`Inflow: ${grouped}`));
    assert.ok(sharing.includes(`Outflow: ${grouped}`));
    const samples = render(language, createElement(WatchTimeDistribution, { data: {
      under5min: 0, min5to15: 0, min15to30: 0, min30to60: 0, over60min: 0,
      avgWatchTime: 0, medianWatchTime: 0,
      dataQuality: { method: 'low_coverage', coverage: 0.01, sample_count: 1234, required_min_samples: 2345 },
    } }));
    assert.ok(samples.includes(`(${grouped})`));
    assert.ok(samples.includes(language === 'de' ? '2.345 Samples' : '2,345 samples'));
  });

  test(`calendar day grid keeps all seven rows outside flex shrinking in ${language}`, () => {
    const html = render(language, createElement(CalendarHeatmap, { data: [], days: 365 }));
    assert.ok(html.includes('overflow-x-auto shrink-0'));
    assert.ok(html.includes('repeat(7, 1.75rem)'));
    assert.ok(html.includes(language === 'de' ? 'Tagesansicht' : 'Daily view'));
  });
}

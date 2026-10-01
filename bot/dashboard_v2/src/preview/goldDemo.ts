import type { DashboardOverview, StreamSession } from '@/types/analytics';

const sessions: StreamSession[] = Array.from({ length: 8 }, (_, index) => ({
  id: 7001 + index,
  date: `2026-09-${String(30 - index * 3).padStart(2, '0')}`,
  startTime: '18:00',
  duration: 10800 + index * 300,
  startViewers: 78 + index * 3,
  peakViewers: 221 - index * 8,
  endViewers: 112 - index * 3,
  avgViewers: 142 - index * 4,
  retention5m: 68,
  retention10m: 54,
  retention20m: 42,
  dropoffPct: 18,
  totalChatterSessions: 230 - index * 10,
  uniqueChatters: 120 - index * 5,
  firstTimeChatters: 24,
  returningChatters: 96 - index * 5,
  followersStart: 3100 - index * 34,
  followersEnd: 3134 - index * 34,
  title: 'Deadlock Ranked mit der Community',
}));

export function getGoldDemoFixture(
  endpoint: string,
  params: Record<string, string | number | boolean>,
): unknown | undefined {
  if (endpoint === '/feedback/counts') return { total: 0, unread: 0 };
  if (endpoint === '/streamers') {
    return ['smallcore_focus', 'midcore_live', 'largecore_peak'].map((login, index) => ({
      login, twitchUserId: `demo-${index}`, isPartner: true,
    }));
  }
  if (endpoint === '/overview') {
    return {
      streamer: String(params.streamer || 'midcore_live'),
      days: Number(params.days || 30),
      scores: { total: 78, reach: 82, retention: 74, engagement: 85, growth: 72, monetization: 64, network: 80 },
      summary: {
        avgViewers: 126, peakViewers: 221, totalHoursWatched: 7182, totalAirtime: 57,
        followersDelta: 214, followersPerHour: 3.75, retention10m: 54,
        totalChatterSessions: 1842, streamCount: 18, avgViewersTrend: 12.4,
        followersTrend: 18.2, retentionTrend: 5.1,
      },
      sessions,
      findings: [
        { type: 'pos', title: 'Deine Community wächst', text: 'Mehr wiederkehrende Zuschauer und ein stabiler Einstieg in den Stream.' },
        { type: 'info', title: 'Guter Zeitpunkt', text: 'Die Abendstreams erreichen im Vergleich die meisten Zuschauer.' },
      ],
      actions: [{ tag: 'Planung', text: 'Den nächsten Community-Abend um 18 Uhr starten.', priority: 'medium' }],
      correlations: { durationVsViewers: 0.62, chatVsRetention: 0.74 },
      network: { sent: 12, received: 9, sentViewers: 438 },
      categoryRank: 7, categoryTotal: 186,
    } satisfies DashboardOverview;
  }
  if (endpoint === '/hourly-heatmap') {
    return Array.from({ length: 7 * 24 }, (_, index) => ({
      weekday: Math.floor(index / 24), hour: index % 24,
      streamCount: index % 24 >= 17 && index % 24 <= 22 ? 3 : 0,
      avgViewers: index % 24 >= 17 && index % 24 <= 22 ? 90 + index % 57 : 0,
      avgPeak: index % 24 >= 17 && index % 24 <= 22 ? 160 + index % 61 : 0,
    }));
  }
  if (endpoint === '/calendar-heatmap') {
    return Array.from({ length: Number(params.days || 90) }, (_, index) => {
      const date = new Date('2026-10-01T12:00:00Z');
      date.setUTCDate(date.getUTCDate() - index);
      const streamed = index % 3 === 0;
      return { date: date.toISOString().slice(0, 10), value: streamed ? 380 + index % 140 : 0,
        streamCount: streamed ? 1 : 0, hoursWatched: streamed ? 380 + index % 140 : 0 };
    });
  }
  if (endpoint === '/viewer-timeline') return [];
  return undefined;
}

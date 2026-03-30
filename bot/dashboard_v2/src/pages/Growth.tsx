import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { TrendingUp, Calendar, Clock, AlertCircle, Loader2, Crown, Users, Play, ArrowDownLeft, ArrowUpRight, UserPlus, Star } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend, BarChart, Bar, Cell } from 'recharts';
import { fetchMonthlyStats, fetchWeekdayStats } from '@/api/analytics';
import { useTagAnalysisExtended, useTitlePerformance, useRaidRetention, useRaidAnalytics } from '@/hooks/useAnalytics';
import { TagPerformanceChart } from '@/components/charts/TagPerformance';
import { RaidRetention } from '@/components/charts/RaidRetention';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import { NoDataCard } from '@/components/cards/NoDataCard';
import { formatNumber, formatPercent, formatDate } from '@/utils/formatters';
import type { MonthlyStats, WeekdayStats, TimeRange, IncomingRaid } from '@/types/analytics';

interface GrowthProps {
  streamer: string;
  days: TimeRange;
}

export function Growth({ streamer, days }: GrowthProps) {
  const { data: monthlyData, isLoading: loadingMonthly } = useQuery<MonthlyStats[]>({
    queryKey: ['monthlyStats', streamer, 12],
    queryFn: () => fetchMonthlyStats(streamer, 12),
    enabled: true,
  });

  const { data: weeklyData, isLoading: loadingWeekly } = useQuery<WeekdayStats[]>({
    queryKey: ['weeklyStats', streamer, days],
    queryFn: () => fetchWeekdayStats(streamer, days),
    enabled: true,
  });

  const { data: tagResponse } = useTagAnalysisExtended(streamer, days);
  const { data: titleResponse } = useTitlePerformance(streamer, days);

  // Extract tags/titles and peer benchmarks from response wrappers
  const tagData = tagResponse?.tags ?? null;
  const titleData = titleResponse?.titles ?? null;
  const tagPeerBenchmark = tagResponse?.peerBenchmark ?? null;
  const titlePeerBenchmark = titleResponse?.peerBenchmark ?? null;
  const { data: raidRetentionData } = useRaidRetention(streamer, days);
  const { data: raidAnalyticsData } = useRaidAnalytics(streamer, days);

  const chartData = useMemo(() => {
    if (!monthlyData) return [];
    return [...monthlyData].reverse().map(m => ({
      name: `${m.monthLabel} ${m.year}`,
      hoursWatched: Math.round(m.totalHoursWatched),
      airtime: Math.round(m.totalAirtime),
      avgViewers: Math.round(m.avgViewers),
      followers: m.followerDelta,
      streams: m.streamCount,
    }));
  }, [monthlyData]);

  if (loadingMonthly || loadingWeekly) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!monthlyData || monthlyData.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Keine Wachstumsdaten verfügbar</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Monthly Overview Cards */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        {monthlyData.slice(0, 4).map((month, i) => (
          <motion.div
            key={`${month.year}-${month.month}`}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: i * 0.1 }}
            className={`panel-card rounded-2xl p-5 ${i === 0 ? 'ring-2 ring-primary/30' : ''}`}
          >
            <div className="flex items-center justify-between mb-3">
              <span className="text-sm text-text-secondary">{month.monthLabel} {month.year}</span>
              {i === 0 && <span className="text-xs bg-primary/20 text-primary px-2 py-0.5 rounded">Aktuell</span>}
            </div>
            <div className="space-y-2">
              <MetricRow
                label="Hours Watched"
                value={month.totalHoursWatched.toLocaleString('de-DE', { maximumFractionDigits: 0 })}
                unit="h"
              />
              <MetricRow
                label="Ø Viewer"
                value={month.avgViewers.toLocaleString('de-DE', { maximumFractionDigits: 0 })}
              />
              <MetricRow
                label="Follower"
                value={(month.followerDelta >= 0 ? '+' : '') + month.followerDelta.toLocaleString('de-DE')}
                isPositive={month.followerDelta >= 0}
              />
              <MetricRow
                label="Streams"
                value={month.streamCount.toString()}
              />
            </div>
          </motion.div>
        ))}
      </div>

      {/* Hours Watched Trend */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2 }}
        className="panel-card rounded-2xl p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <TrendingUp className="w-6 h-6 text-primary" />
          <h2 className="text-xl font-bold text-white">Wachstumstrend (12 Monate)</h2>
        </div>

        <div className="h-[300px]">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={chartData}>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(194, 221, 240, 0.2)" />
              <XAxis dataKey="name" stroke="#9ca3af" fontSize={12} />
              <YAxis stroke="#9ca3af" fontSize={12} />
              <Tooltip
                contentStyle={{
                  backgroundColor: '#1f2937',
                  border: '1px solid rgba(194, 221, 240, 0.25)',
                  borderRadius: '8px',
                }}
                labelStyle={{ color: '#fff' }}
              />
              <Legend />
              <Line
                type="monotone"
                dataKey="hoursWatched"
                name="Hours Watched"
                stroke="var(--color-primary)"
                strokeWidth={2}
                dot={{ fill: 'var(--color-primary)' }}
              />
              <Line
                type="monotone"
                dataKey="avgViewers"
                name="Ø Viewer"
                stroke="#10b981"
                strokeWidth={2}
                dot={{ fill: '#10b981' }}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </motion.div>

      {/* Weekday Analysis */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.3 }}
        className="panel-card rounded-2xl p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Calendar className="w-6 h-6 text-accent" />
          <h2 className="text-xl font-bold text-white">Wochentags-Analyse</h2>
        </div>

        {weeklyData && weeklyData.length > 0 ? (
          <WeekdayCards data={weeklyData} />
        ) : (
          <div className="text-center py-8 text-text-secondary">
            <Calendar className="w-12 h-12 mx-auto mb-3 opacity-50" />
            <p>Keine Wochentags-Daten verfügbar</p>
          </div>
        )}
      </motion.div>

      {/* Stream Schedule Insights */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.4 }}
        className="bg-gradient-to-r from-primary/10 to-accent/10 rounded-xl border border-primary/20 p-6"
      >
        <div className="flex items-center gap-3 mb-4">
          <Clock className="w-6 h-6 text-primary" />
          <h3 className="text-lg font-bold text-white">Schedule-Empfehlungen</h3>
        </div>

        {weeklyData && weeklyData.length > 0 && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {generateScheduleInsights(weeklyData).map((insight, i) => (
              <div key={i} className="flex items-start gap-3 p-3 bg-background/50 rounded-lg">
                <div className={`w-2 h-2 rounded-full mt-2 ${insight.priority === 'high' ? 'bg-success' : 'bg-primary'}`} />
                <div>
                  <div className="text-sm font-medium text-white">{insight.title}</div>
                  <div className="text-sm text-text-secondary">{insight.text}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </motion.div>

      {/* Tag & Title Performance */}
      <PlanGateCard featureId="title_performance" title="Titel-Performance">
        {(tagData || titleData) && (
          <TagPerformanceChart
            tagData={tagData || mockTagData}
            titleData={titleData}
            peerBenchmark={tagPeerBenchmark || titlePeerBenchmark}
          />
        )}

        {/* Fallback if no tag data from API */}
        {!tagData && !titleData && (
          <TagPerformanceChart
            tagData={mockTagData}
            titleData={mockTitleData}
          />
        )}
      </PlanGateCard>

      {/* Raid Retention */}
      <PlanGateCard featureId="raid_retention" title="Raid-Retention">
        <div>
          <h2 className="text-lg font-semibold text-white mb-4">Raid-Retention</h2>
          <RaidRetention data={raidRetentionData} />
        </div>
      </PlanGateCard>

      {/* ── Incoming Raids Section ── */}
      <IncomingRaidsSection raidAnalyticsData={raidAnalyticsData} />
    </div>
  );
}

// Mock data for when API endpoints aren't available yet
const mockTagData = [
  { tagName: 'Deadlock', usageCount: 15, avgViewers: 145, avgRetention10m: 58, avgFollowerGain: 12, trend: 'up' as const, trendValue: 15, bestTimeSlot: '18:00-22:00', avgStreamDuration: 14400, categoryRank: 5 },
  { tagName: 'German', usageCount: 15, avgViewers: 142, avgRetention10m: 55, avgFollowerGain: 10, trend: 'stable' as const, trendValue: 2, bestTimeSlot: '19:00-23:00', avgStreamDuration: 14000, categoryRank: 8 },
  { tagName: 'Competitive', usageCount: 8, avgViewers: 165, avgRetention10m: 62, avgFollowerGain: 15, trend: 'up' as const, trendValue: 22, bestTimeSlot: '20:00-24:00', avgStreamDuration: 12000, categoryRank: 3 },
  { tagName: 'Ranked', usageCount: 6, avgViewers: 158, avgRetention10m: 60, avgFollowerGain: 14, trend: 'down' as const, trendValue: -5, bestTimeSlot: '18:00-22:00', avgStreamDuration: 10800, categoryRank: 6 },
  { tagName: 'Chill', usageCount: 4, avgViewers: 95, avgRetention10m: 48, avgFollowerGain: 6, trend: 'stable' as const, trendValue: 0, bestTimeSlot: '14:00-18:00', avgStreamDuration: 18000, categoryRank: 15 },
];

const mockTitleData = [
  { title: 'Ranked Grind bis Phantom! !discord', usageCount: 5, avgViewers: 168, avgRetention10m: 62, avgFollowerGain: 18, peakViewers: 245, keywords: ['Ranked', 'Grind', 'Phantom'] },
  { title: 'Chill Deadlock mit Zuschauern', usageCount: 4, avgViewers: 125, avgRetention10m: 55, avgFollowerGain: 8, peakViewers: 180, keywords: ['Chill', 'Zuschauer'] },
  { title: 'Road to Top 500 | Tag 42', usageCount: 3, avgViewers: 195, avgRetention10m: 68, avgFollowerGain: 22, peakViewers: 312, keywords: ['Road', 'Top 500', 'Tag'] },
];

interface MetricRowProps {
  label: string;
  value: string;
  unit?: string;
  isPositive?: boolean;
}

function MetricRow({ label, value, unit, isPositive }: MetricRowProps) {
  return (
    <div className="flex items-center justify-between text-sm">
      <span className="text-text-secondary">{label}</span>
      <span className={`font-medium ${isPositive !== undefined ? (isPositive ? 'text-success' : 'text-error') : 'text-white'}`}>
        {value}{unit && <span className="text-text-secondary ml-0.5">{unit}</span>}
      </span>
    </div>
  );
}

function WeekdayCards({ data }: { data: WeekdayStats[] }) {
  const maxViewers = Math.max(...data.map(d => d.avgViewers), 1);
  const bestDay = data.reduce((a, b) => a.avgViewers > b.avgViewers ? a : b);

  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-3">
      {data.map((day, i) => {
        const viewerPct = (day.avgViewers / maxViewers) * 100;
        const isBest = day.weekdayLabel === bestDay.weekdayLabel;
        const hasStreams = day.streamCount > 0;

        return (
          <motion.div
            key={day.weekdayLabel}
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.05 * i }}
            className={`relative p-4 rounded-xl border transition-all ${
              isBest
                ? 'bg-gradient-to-b from-accent/20 to-card border-accent/40 ring-1 ring-accent/20'
                : hasStreams
                ? 'bg-background border-border hover:border-border-hover'
                : 'bg-background/50 border-border/50 opacity-60'
            }`}
          >
            {/* Crown for best day */}
            {isBest && (
              <div className="absolute -top-2.5 left-1/2 -translate-x-1/2">
                <div className="bg-accent/20 border border-accent/30 rounded-full p-1">
                  <Crown className="w-3 h-3 text-accent" />
                </div>
              </div>
            )}

            {/* Day name */}
            <div className={`text-center text-sm font-semibold mb-3 ${isBest ? 'text-accent' : 'text-text-secondary'}`}>
              {day.weekdayLabel}
            </div>

            {/* Viewer bar */}
            <div className="h-24 flex items-end justify-center mb-3">
              <motion.div
                initial={{ height: 0 }}
                animate={{ height: `${Math.max(hasStreams ? 8 : 0, viewerPct)}%` }}
                transition={{ delay: 0.2 + i * 0.05, duration: 0.5, ease: 'easeOut' }}
                className={`w-8 rounded-t-lg ${
                  isBest
                    ? 'bg-gradient-to-t from-accent/60 to-accent'
                    : hasStreams
                    ? 'bg-gradient-to-t from-primary/40 to-primary/70'
                    : 'bg-border/30'
                }`}
              />
            </div>

            {/* Viewer count */}
            <div className="text-center">
              <div className={`text-lg font-bold ${isBest ? 'text-white' : hasStreams ? 'text-white' : 'text-text-secondary'}`}>
                {hasStreams ? Math.round(day.avgViewers) : '-'}
              </div>
              <div className="text-[10px] text-text-secondary uppercase tracking-wider">
                <Users className="w-3 h-3 inline mr-0.5 -mt-0.5" />
                Ø Viewer
              </div>
            </div>

            {/* Meta info */}
            <div className="mt-3 pt-3 border-t border-border/50 space-y-1.5">
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1">
                  <Play className="w-3 h-3" />Streams
                </span>
                <span className="text-white font-medium">{day.streamCount}</span>
              </div>
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1">
                  <Clock className="w-3 h-3" />Ø Dauer
                </span>
                <span className="text-white font-medium">{day.avgHours > 0 ? `${day.avgHours.toFixed(1)}h` : '-'}</span>
              </div>
              <div className="flex items-center justify-between text-xs">
                <span className="text-text-secondary flex items-center gap-1">
                  <TrendingUp className="w-3 h-3" />Peak
                </span>
                <span className="text-white font-medium">{day.avgPeak > 0 ? Math.round(day.avgPeak) : '-'}</span>
              </div>
            </div>
          </motion.div>
        );
      })}
    </div>
  );
}

function generateScheduleInsights(data: WeekdayStats[]) {
  const insights = [];
  const sorted = [...data].sort((a, b) => b.avgViewers - a.avgViewers);
  const best = sorted[0];
  const underperforming = data.filter(d => d.streamCount > 0 && d.avgViewers < sorted[Math.floor(sorted.length / 2)]?.avgViewers);

  insights.push({
    priority: 'high',
    title: `Fokus auf ${best.weekdayLabel}`,
    text: `${best.weekdayLabel} zeigt die besten Viewer-Zahlen. Plane wichtige Content-Events an diesem Tag.`,
  });

  if (underperforming.length > 0) {
    insights.push({
      priority: 'medium',
      title: 'Optimierungspotential',
      text: `${underperforming.map(d => d.weekdayLabel).join(', ')} haben unterdurchschnittliche Performance. Experimentiere mit anderen Zeiten.`,
    });
  }

  const noStreams = data.filter(d => d.streamCount === 0);
  if (noStreams.length > 0) {
    insights.push({
      priority: 'medium',
      title: 'Ungenutzte Tage',
      text: `Keine Streams an ${noStreams.map(d => d.weekdayLabel).join(', ')}. Teste diese Slots!`,
    });
  }

  return insights;
}

// ── Incoming Raids Section ──

function boostColor(pct: number | null): string {
  if (pct === null) return 'text-text-secondary';
  if (pct > 0) return 'text-success';
  if (pct < 0) return 'text-error';
  return 'text-text-secondary';
}

function retentionColorIncoming(pct: number | null): string {
  if (pct === null) return 'text-text-secondary';
  if (pct >= 50) return 'text-success';
  if (pct >= 30) return 'text-warning';
  return 'text-error';
}

interface TopRaider {
  from_channel: string;
  raid_count: number;
  avg_viewers: number;
  avg_boost: number | null;
}

function computeTopRaiders(raids: IncomingRaid[]): TopRaider[] {
  const grouped: Record<string, { displayName: string; count: number; totalViewers: number; boosts: number[]; }> = {};
  for (const r of raids) {
    const key = r.from_channel.toLowerCase();
    if (!grouped[key]) {
      grouped[key] = { displayName: r.from_channel, count: 0, totalViewers: 0, boosts: [] };
    }
    grouped[key].count += 1;
    grouped[key].totalViewers += r.viewers_sent;
    if (r.impact.boost_pct !== null) {
      grouped[key].boosts.push(r.impact.boost_pct);
    }
  }
  return Object.values(grouped)
    .map((v) => ({
      from_channel: v.displayName,
      raid_count: v.count,
      avg_viewers: Math.round(v.totalViewers / v.count),
      avg_boost: v.boosts.length > 0 ? Math.round((v.boosts.reduce((a, b) => a + b, 0) / v.boosts.length) * 10) / 10 : null,
    }))
    .sort((a, b) => b.raid_count - a.raid_count || (b.avg_boost ?? 0) - (a.avg_boost ?? 0))
    .slice(0, 5);
}

interface IncomingRaidsSectionProps {
  raidAnalyticsData: import('@/types/analytics').RaidAnalytics | undefined;
}

function IncomingRaidsSection({ raidAnalyticsData }: IncomingRaidsSectionProps) {
  const incomingRaids = raidAnalyticsData?.incoming_raids ?? [];
  const summary = raidAnalyticsData?.incoming_summary;

  if (!raidAnalyticsData) {
    return null; // Still loading or no data
  }

  if (incomingRaids.length === 0) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
      >
        <NoDataCard
          message="Keine eingehenden Raids"
          submessage="Es wurden noch keine Raids zu deinem Kanal erkannt."
          icon={ArrowDownLeft}
        />
      </motion.div>
    );
  }

  const topRaiders = computeTopRaiders(incomingRaids);
  const balanceData = summary ? [
    { name: 'Gesendet', value: summary.raid_balance.sent, fill: 'var(--color-primary)' },
    { name: 'Empfangen', value: summary.raid_balance.received, fill: '#10b981' },
  ] : [];

  return (
    <>
      {/* 1. Raid-Bilanz */}
      {summary && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="bg-card rounded-xl border border-border p-5"
        >
          <div className="flex items-center gap-3 mb-4">
            <ArrowDownLeft className="w-5 h-5 text-success" />
            <h3 className="text-lg font-bold text-white">Raid-Bilanz</h3>
          </div>
          <div className="h-[200px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={balanceData} layout="vertical" margin={{ left: 20, right: 40 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="rgba(194, 221, 240, 0.15)" />
                <XAxis type="number" stroke="#9ca3af" fontSize={12} />
                <YAxis type="category" dataKey="name" stroke="#9ca3af" fontSize={13} width={90} />
                <Tooltip
                  contentStyle={{
                    backgroundColor: '#1f2937',
                    border: '1px solid rgba(194, 221, 240, 0.25)',
                    borderRadius: '8px',
                  }}
                  labelStyle={{ color: '#fff' }}
                  formatter={(value) => [formatNumber(value as number), 'Raids']}
                />
                <Bar dataKey="value" radius={[0, 6, 6, 0]} barSize={32}>
                  {balanceData.map((entry, index) => (
                    <Cell key={index} fill={entry.fill} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
          <div className="flex justify-center gap-6 mt-2 text-sm">
            <span className="flex items-center gap-2">
              <ArrowUpRight className="w-4 h-4 text-primary" />
              <span className="text-text-secondary">Gesendet:</span>
              <span className="text-white font-medium">{formatNumber(summary.raid_balance.sent)}</span>
            </span>
            <span className="flex items-center gap-2">
              <ArrowDownLeft className="w-4 h-4 text-success" />
              <span className="text-text-secondary">Empfangen:</span>
              <span className="text-white font-medium">{formatNumber(summary.raid_balance.received)}</span>
            </span>
          </div>
        </motion.div>
      )}

      {/* 2. Raid-Impact Zusammenfassung (KPI cards) */}
      {summary && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4"
        >
          <div className="bg-card rounded-xl border border-border p-4">
            <div className="w-10 h-10 rounded-lg bg-primary/10 text-primary flex items-center justify-center mb-3">
              <Users className="w-5 h-5" />
            </div>
            <div className="text-sm text-text-secondary mb-1">Ø Viewer empfangen</div>
            <div className="text-xl font-bold text-white">{formatNumber(summary.avg_viewers_received, 1)}</div>
          </div>
          <div className="bg-card rounded-xl border border-border p-4">
            <div className="w-10 h-10 rounded-lg bg-success/10 text-success flex items-center justify-center mb-3">
              <TrendingUp className="w-5 h-5" />
            </div>
            <div className="text-sm text-text-secondary mb-1">Ø Boost</div>
            <div className={`text-xl font-bold ${boostColor(summary.avg_boost_pct)}`}>
              {summary.avg_boost_pct !== null ? formatPercent(summary.avg_boost_pct) : '-'}
            </div>
          </div>
          <div className="bg-card rounded-xl border border-border p-4">
            <div className="w-10 h-10 rounded-lg bg-accent/10 text-accent flex items-center justify-center mb-3">
              <Clock className="w-5 h-5" />
            </div>
            <div className="text-sm text-text-secondary mb-1">Ø 15m Retention</div>
            <div className={`text-xl font-bold ${retentionColorIncoming(summary.avg_retention_15m)}`}>
              {summary.avg_retention_15m !== null ? formatPercent(summary.avg_retention_15m) : '-'}
            </div>
          </div>
          <div className="bg-card rounded-xl border border-border p-4">
            <div className="w-10 h-10 rounded-lg bg-warning/10 text-warning flex items-center justify-center mb-3">
              <Star className="w-5 h-5" />
            </div>
            <div className="text-sm text-text-secondary mb-1">Bester Raider</div>
            <div className="text-xl font-bold text-white truncate">
              {summary.best_raider ?? '-'}
            </div>
          </div>
        </motion.div>
      )}

      {/* 3. Incoming Raids Tabelle */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <div className="flex items-center gap-3 mb-4">
          <ArrowDownLeft className="w-5 h-5 text-success" />
          <h3 className="text-lg font-bold text-white">Eingehende Raids</h3>
          <span className="text-sm text-text-secondary ml-auto">{incomingRaids.length} Raids</span>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="text-left py-2 text-text-secondary font-medium">Von</th>
                <th className="text-left py-2 text-text-secondary font-medium">Datum</th>
                <th className="text-right py-2 text-text-secondary font-medium">Viewer</th>
                <th className="text-right py-2 text-text-secondary font-medium">Boost %</th>
                <th className="text-right py-2 text-text-secondary font-medium">15m Ret.</th>
                <th className="text-right py-2 text-text-secondary font-medium">Follows</th>
              </tr>
            </thead>
            <tbody>
              {incomingRaids.slice(0, 20).map((raid, i) => (
                <tr key={`${raid.from_channel}-${raid.detected_at}-${i}`} className="border-b border-border/50 hover:bg-background/50">
                  <td className="py-2 text-white font-medium">{raid.from_channel}</td>
                  <td className="py-2 text-text-secondary">{formatDate(raid.detected_at)}</td>
                  <td className="py-2 text-right text-text-secondary">{formatNumber(raid.viewers_sent)}</td>
                  <td className={`py-2 text-right font-medium ${boostColor(raid.impact.boost_pct)}`}>
                    {raid.impact.boost_pct !== null ? `${raid.impact.boost_pct > 0 ? '+' : ''}${formatPercent(raid.impact.boost_pct)}` : '-'}
                  </td>
                  <td className={`py-2 text-right font-medium ${retentionColorIncoming(raid.impact.retention_15m_pct)}`}>
                    {raid.impact.retention_15m_pct !== null ? formatPercent(raid.impact.retention_15m_pct) : '-'}
                  </td>
                  <td className="py-2 text-right text-text-secondary">
                    {raid.impact.follows_after_raid > 0 ? (
                      <span className="text-success flex items-center justify-end gap-1">
                        <UserPlus className="w-3 h-3" />
                        {raid.impact.follows_after_raid}
                      </span>
                    ) : '-'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </motion.div>

      {/* 4. Top Raiders */}
      {topRaiders.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          className="bg-card rounded-xl border border-border p-5"
        >
          <div className="flex items-center gap-3 mb-4">
            <Crown className="w-5 h-5 text-warning" />
            <h3 className="text-lg font-bold text-white">Top Raiders</h3>
          </div>
          <div className="space-y-3">
            {topRaiders.map((raider, i) => (
              <div key={raider.from_channel} className="flex items-center gap-3 p-3 bg-background/50 rounded-lg">
                <div className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold ${
                  i === 0 ? 'bg-warning/20 text-warning' : 'bg-border text-text-secondary'
                }`}>
                  {i + 1}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-white font-medium truncate">{raider.from_channel}</div>
                  <div className="text-xs text-text-secondary">
                    {raider.raid_count} Raid{raider.raid_count !== 1 ? 's' : ''} · Ø {formatNumber(raider.avg_viewers)} Viewer
                  </div>
                </div>
                <div className="text-right">
                  <div className={`text-sm font-medium ${boostColor(raider.avg_boost)}`}>
                    {raider.avg_boost !== null ? `${raider.avg_boost > 0 ? '+' : ''}${formatPercent(raider.avg_boost)}` : '-'}
                  </div>
                  <div className="text-xs text-text-secondary">Ø Boost</div>
                </div>
              </div>
            ))}
          </div>
        </motion.div>
      )}
    </>
  );
}

export default Growth;

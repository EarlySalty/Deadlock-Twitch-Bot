import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { TrendingUp, AlertCircle, Loader2, Clock, Crown, Users, ArrowDownLeft, ArrowUpRight, UserPlus, Star } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend, BarChart, Bar, Cell } from 'recharts';
import { fetchMonthlyStats } from '@/api/analytics';
import { useTagAnalysisExtended, useTitlePerformance, useRaidRetention, useRaidAnalytics } from '@/hooks/useAnalytics';
import { TagPerformanceChart } from '@/components/charts/TagPerformance';
import { RaidRetention } from '@/components/charts/RaidRetention';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import { NoDataCard } from '@/components/cards/NoDataCard';
import { formatNumber, formatPercent, formatDate } from '@/utils/formatters';
import type { MonthlyStats, TimeRange, IncomingRaid } from '@/types/analytics';

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

  if (loadingMonthly) {
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

      {/* Tag & Title Performance */}
      <PlanGateCard featureId="title_performance" title="Titel-Performance">
        {(tagData || titleData) && (
          <TagPerformanceChart
            tagData={tagData ?? []}
            titleData={titleData}
            peerBenchmark={tagPeerBenchmark || titlePeerBenchmark}
          />
        )}

        {!tagData && !titleData && (
          <NoDataCard message="Keine Tag- oder Titel-Performance-Daten verfügbar" />
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

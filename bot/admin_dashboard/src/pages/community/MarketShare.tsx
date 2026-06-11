import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { BarChart3, Crown, Globe, TrendingUp, Users } from 'lucide-react';
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Legend,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { PageHeader } from '@/components/layout/PageHeader';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { useMarketShare } from '@/hooks/useAdmin';
import type { MarketShareScope } from '@/api/types';

const DAY_OPTIONS: { label: string; days: number }[] = [
  { label: '24h', days: 1 },
  { label: '7 Tage', days: 7 },
  { label: '30 Tage', days: 31 },
  { label: '90 Tage', days: 90 },
];

const SCOPE_OPTIONS: { label: string; scope: MarketShareScope }[] = [
  { label: 'Deutschsprachig', scope: 'german' },
  { label: 'Global', scope: 'all' },
];

// Vor dem Kategorie-Poll-Cutover (10.06.2026) enthält die Datenbasis nur die
// sprachgefilterte Teilmenge der Kategorie — globale Anteile davor sind nicht
// mit den Werten danach vergleichbar.
const FULL_CATEGORY_SINCE = new Date('2026-06-10T00:00:00Z');

function formatNumber(value: number): string {
  return Math.round(value).toLocaleString('de-DE');
}

function formatPct(value: number): string {
  return `${value.toLocaleString('de-DE', { maximumFractionDigits: 1 })}%`;
}

function formatBucketLabel(iso: string, bucketSeconds: number): string {
  const date = new Date(iso);
  if (bucketSeconds >= 86_400) {
    return date.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit' });
  }
  return date.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function pillClass(active: boolean): string {
  return [
    'rounded-full border px-4 py-1.5 text-sm font-medium transition',
    active
      ? 'border-primary/60 bg-primary/20 text-white'
      : 'border-white/10 bg-white/[0.03] text-text-secondary hover:border-white/25 hover:text-white',
  ].join(' ');
}

export default function MarketSharePage() {
  const [days, setDays] = useState(7);
  const [scope, setScope] = useState<MarketShareScope>('german');
  const query = useMarketShare(days, scope);

  const data = query.data;
  const current = data?.current ?? null;
  const peak = data?.peak ?? null;

  const chartRows = useMemo(() => {
    if (!data) {
      return [];
    }
    return data.series.map((point) => ({
      label: formatBucketLabel(point.ts, data.bucketSeconds),
      Netzwerk: Math.round(point.partnerViewers * 10) / 10,
      Rest: Math.max(Math.round((point.totalViewers - point.partnerViewers) * 10) / 10, 0),
      'Anteil %': Math.round(point.sharePct * 10) / 10,
    }));
  }, [data]);

  const rangeIncludesLegacyData = useMemo(() => {
    const from = new Date(Date.now() - days * 86_400_000);
    return from < FULL_CATEGORY_SINCE;
  }, [days]);

  const liveShare = scope === 'german' ? current?.germanSharePct : current?.sharePct;
  const liveViewers = scope === 'german' ? current?.germanPartnerViewers : current?.partnerViewers;
  const liveMarket = scope === 'german' ? current?.germanViewers : current?.totalViewers;

  return (
    <motion.section initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="space-y-5">
      <PageHeader
        title="Markt-Dominanz"
        description="Viewer-Anteil unseres Partner-Netzwerks an der Deadlock-Kategorie auf Twitch — live und im Zeitverlauf. Die deutschsprachige Sicht filtert über Stream-Tags (Deutsch/German)."
        secondaryChips={
          <div className="flex flex-wrap items-center gap-2">
            {SCOPE_OPTIONS.map((option) => (
              <button
                key={option.scope}
                type="button"
                className={pillClass(scope === option.scope)}
                onClick={() => setScope(option.scope)}
              >
                {option.label}
              </button>
            ))}
            <span className="mx-1 h-5 w-px bg-white/10" />
            {DAY_OPTIONS.map((option) => (
              <button
                key={option.days}
                type="button"
                className={pillClass(days === option.days)}
                onClick={() => setDays(option.days)}
              >
                {option.label}
              </button>
            ))}
          </div>
        }
      />

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <KpiCard
          title={scope === 'german' ? 'Marktanteil live (DE)' : 'Marktanteil live (global)'}
          value={liveShare !== undefined && liveShare !== null ? formatPct(liveShare) : '—'}
          hint={
            current
              ? `${formatNumber(liveViewers ?? 0)} von ${formatNumber(liveMarket ?? 0)} Viewern`
              : 'Keine Live-Daten'
          }
          tone="primary"
          icon={Crown}
        />
        <KpiCard
          title="Live-Streams"
          value={current ? `${formatNumber(current.partnerStreams)} / ${formatNumber(current.totalStreams)}` : '—'}
          hint={current ? `davon ${formatNumber(current.germanStreams)} mit Deutsch-Tag` : undefined}
          tone="accent"
          icon={Users}
        />
        <KpiCard
          title="Kategorie-Viewer live"
          value={current ? formatNumber(current.totalViewers) : '—'}
          hint={current ? `Stand ${new Date(current.ts).toLocaleString('de-DE')}` : undefined}
          icon={Globe}
        />
        <KpiCard
          title="Peak-Anteil im Zeitraum"
          value={peak ? formatPct(peak.sharePct) : '—'}
          hint={
            peak
              ? `${formatNumber(peak.partnerViewers)} von ${formatNumber(peak.totalViewers)} Viewern am ${new Date(peak.ts).toLocaleString('de-DE')}`
              : undefined
          }
          icon={TrendingUp}
        />
      </div>

      <article className="panel-card rounded-[1.8rem] p-6">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
            Viewer-Verteilung & Marktanteil
          </p>
          {rangeIncludesLegacyData ? (
            <p className="text-xs text-warning">
              Hinweis: Daten vor dem 10.06.2026 enthalten nur die deutschsprachige Teilmenge der Kategorie.
            </p>
          ) : null}
        </div>
        <div className="mt-6 h-96">
          {query.isLoading ? (
            <div className="flex h-full items-center justify-center text-sm text-text-secondary">
              Lade Marktdaten …
            </div>
          ) : chartRows.length ? (
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart data={chartRows}>
                <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                <XAxis dataKey="label" stroke="#9bb3c5" tickLine={false} axisLine={false} minTickGap={32} />
                <YAxis yAxisId="viewers" stroke="#9bb3c5" tickLine={false} axisLine={false} />
                <YAxis
                  yAxisId="share"
                  orientation="right"
                  stroke="#f5b94c"
                  tickLine={false}
                  axisLine={false}
                  unit="%"
                />
                <Tooltip
                  contentStyle={{
                    background: '#0f2431',
                    border: '1px solid rgba(255,255,255,0.1)',
                    borderRadius: '16px',
                  }}
                />
                <Legend />
                <Area
                  yAxisId="viewers"
                  type="monotone"
                  dataKey="Netzwerk"
                  stackId="viewers"
                  stroke="#10b7ad"
                  fill="rgba(16,183,173,0.45)"
                />
                <Area
                  yAxisId="viewers"
                  type="monotone"
                  dataKey="Rest"
                  stackId="viewers"
                  stroke="#3a566b"
                  fill="rgba(58,86,107,0.30)"
                />
                <Line
                  yAxisId="share"
                  type="monotone"
                  dataKey="Anteil %"
                  stroke="#f5b94c"
                  strokeWidth={2}
                  dot={false}
                />
              </ComposedChart>
            </ResponsiveContainer>
          ) : (
            <EmptyState
              icon={BarChart3}
              title="Keine Daten im Zeitraum"
              description="Für den gewählten Zeitraum und Scope liegen keine Kategorie-Samples vor."
            />
          )}
        </div>
      </article>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
          Live: Top-Streams der Kategorie
        </p>
        {current && current.topStreams.length ? (
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="text-xs uppercase tracking-[0.18em] text-text-secondary">
                  <th className="py-2 pr-4 font-semibold">Streamer</th>
                  <th className="py-2 pr-4 font-semibold">Viewer</th>
                  <th className="py-2 pr-4 font-semibold">Netzwerk</th>
                  <th className="py-2 font-semibold">Sprache</th>
                </tr>
              </thead>
              <tbody>
                {current.topStreams.map((stream) => (
                  <tr key={stream.streamer} className="border-t border-white/5">
                    <td className="py-2.5 pr-4 font-medium text-white">{stream.streamer}</td>
                    <td className="py-2.5 pr-4 text-text-secondary">{formatNumber(stream.viewers)}</td>
                    <td className="py-2.5 pr-4">
                      {stream.isPartner ? (
                        <span className="inline-flex items-center gap-1.5 rounded-full border border-primary/50 bg-primary/15 px-2.5 py-0.5 text-xs font-medium text-white">
                          <Crown className="h-3 w-3" /> Partner
                        </span>
                      ) : (
                        <span className="text-xs text-text-secondary">extern</span>
                      )}
                    </td>
                    <td className="py-2.5 text-xs text-text-secondary">
                      {stream.isGerman ? 'Deutsch-Tag' : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="mt-6">
            <EmptyState
              icon={BarChart3}
              title="Kein Live-Tick"
              description="Es liegt kein aktueller Kategorie-Snapshot vor."
            />
          </div>
        )}
      </article>
    </motion.section>
  );
}

import { useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { Activity, BarChart3, Crown, Globe, Radio, TrendingUp, Users } from 'lucide-react';
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Legend,
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
  { label: '180 Tage', days: 180 },
  { label: 'Alles', days: 365 },
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
      'Anteil %': point.totalViewers > 0 ? Math.round(point.sharePct * 10) / 10 : null,
      'Netzwerk-Kanäle': Math.round(point.partnerStreams * 10) / 10,
      'Rest-Kanäle': Math.max(Math.round((point.totalStreams - point.partnerStreams) * 10) / 10, 0),
      'Kanal-Anteil %':
        point.totalStreams > 0
          ? Math.round((point.partnerStreams / point.totalStreams) * 1000) / 10
          : null,
    }));
  }, [data]);

  const rangeIncludesLegacyData = useMemo(() => {
    const from = new Date(Date.now() - days * 86_400_000);
    return from < FULL_CATEGORY_SINCE;
  }, [days]);

  // Dominanz-Zeit: Anteil der Mess-Fenster, in denen das Netzwerk die
  // Mehrheit des Marktes hielt (≥ 50 % Viewer-Anteil).
  const dominancePct = useMemo(() => {
    if (!data) {
      return null;
    }
    const active = data.series.filter((point) => point.totalViewers > 0);
    if (!active.length) {
      return null;
    }
    const dominant = active.filter((point) => point.sharePct >= 50).length;
    return (dominant / active.length) * 100;
  }, [data]);

  // Durchschnittlicher Kanal-Anteil über den Zeitraum (bucket-gewichtet).
  const channelShareAvg = useMemo(() => {
    if (!data) {
      return null;
    }
    let partner = 0;
    let total = 0;
    for (const point of data.series) {
      partner += point.partnerStreams;
      total += point.totalStreams;
    }
    return total > 0 ? (partner / total) * 100 : null;
  }, [data]);

  const liveShare = scope === 'german' ? current?.germanSharePct : current?.sharePct;
  const liveViewers = scope === 'german' ? current?.germanPartnerViewers : current?.partnerViewers;
  const liveMarket = scope === 'german' ? current?.germanViewers : current?.totalViewers;
  const livePartnerStreams =
    scope === 'german' ? current?.germanPartnerStreams : current?.partnerStreams;
  const liveMarketStreams = scope === 'german' ? current?.germanStreams : current?.totalStreams;

  return (
    <motion.section initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="space-y-5">
      <PageHeader
        title="Markt-Dominanz"
        description="Viewer-Anteil unseres Partner-Netzwerks an der Deadlock-Kategorie auf Twitch — live und im Zeitverlauf. Die deutschsprachige Sicht filtert über die Stream-Sprache (de), genau wie die Bot-Discovery; ältere Datenpunkte ohne Sprachinfo über Stream-Tags."
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

      <div className="space-y-5">
        <div>
          <p className="px-1 text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
            Viewer
          </p>
          <div className="mt-2 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
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
              title="Netzwerk-Viewer live"
              value={current ? formatNumber(liveViewers ?? 0) : '—'}
              hint={current ? `von ${formatNumber(liveMarket ?? 0)} Viewern im Markt` : undefined}
              tone="accent"
              icon={Users}
            />
            <KpiCard
              title={scope === 'german' ? 'DE-Markt-Viewer live' : 'Kategorie-Viewer live'}
              value={
                current
                  ? formatNumber(scope === 'german' ? current.germanViewers : current.totalViewers)
                  : '—'
              }
              hint={current ? `Stand ${new Date(current.ts).toLocaleString('de-DE')}` : undefined}
              icon={Globe}
            />
            <KpiCard
              title="Dominanz im Zeitraum"
              value={dominancePct !== null ? formatPct(dominancePct) : '—'}
              hint={
                peak
                  ? `der Zeit ≥ 50 % Marktanteil · Peak ${formatPct(peak.sharePct)} (${formatNumber(peak.partnerViewers)} von ${formatNumber(peak.totalViewers)} Viewern)`
                  : 'der Zeit ≥ 50 % Marktanteil'
              }
              icon={TrendingUp}
            />
          </div>
        </div>
        <div>
          <p className="px-1 text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
            Streamer
          </p>
          <div className="mt-2 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            <KpiCard
              title="Unter Vertrag"
              value={data ? formatNumber(data.roster.partnersTotal) : '—'}
              hint="aktive Partner im Netzwerk"
              tone="primary"
              icon={Crown}
            />
            <KpiCard
              title="Aktiv im Zeitraum"
              value={
                data
                  ? `${formatNumber(data.roster.partnersSeenInRange)} von ${formatNumber(data.roster.partnersTotal)}`
                  : '—'
              }
              hint={`Partner mit Deadlock-Stream in den letzten ${days === 1 ? '24 h' : `${days} Tagen`}`}
              tone="accent"
              icon={Activity}
            />
            <KpiCard
              title="Live jetzt"
              value={
                current
                  ? `${formatNumber(livePartnerStreams ?? 0)} / ${formatNumber(liveMarketStreams ?? 0)}`
                  : '—'
              }
              hint={
                current && (liveMarketStreams ?? 0) > 0
                  ? `${formatPct(((livePartnerStreams ?? 0) / (liveMarketStreams ?? 1)) * 100)} der Markt-Kanäle gehören uns`
                  : 'Netzwerk / Markt-Kanäle'
              }
              icon={Radio}
            />
            <KpiCard
              title="Ø Kanal-Anteil"
              value={channelShareAvg !== null ? formatPct(channelShareAvg) : '—'}
              hint="Anteil Netzwerk-Kanäle an allen Markt-Kanälen im Zeitraum"
              icon={BarChart3}
            />
          </div>
        </div>
      </div>

      <article className="panel-card rounded-[1.8rem] p-6">
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
            Viewer-Verteilung & Marktanteil
          </p>
          {scope === 'all' && rangeIncludesLegacyData ? (
            <p className="text-xs text-warning">
              Hinweis: Daten vor dem 10.06.2026 enthalten nur die deutschsprachige Teilmenge der Kategorie.
            </p>
          ) : null}
        </div>
        <div className="mt-6">
          {query.isLoading ? (
            <div className="flex h-64 items-center justify-center text-sm text-text-secondary">
              Lade Marktdaten …
            </div>
          ) : chartRows.length ? (
            <div className="space-y-8">
              <div>
                <p className="text-xs uppercase tracking-[0.18em] text-text-secondary">
                  Marktanteil des Netzwerks
                </p>
                <div className="mt-3 h-64">
                  <ResponsiveContainer width="100%" height="100%">
                    <ComposedChart data={chartRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis
                        dataKey="label"
                        stroke="#9bb3c5"
                        tickLine={false}
                        axisLine={false}
                        minTickGap={48}
                      />
                      <YAxis stroke="#f5b94c" tickLine={false} axisLine={false} unit="%" />
                      <Tooltip
                        contentStyle={{
                          background: '#0f2431',
                          border: '1px solid rgba(255,255,255,0.1)',
                          borderRadius: '16px',
                        }}
                      />
                      <Area
                        type="monotone"
                        dataKey="Anteil %"
                        stroke="#f5b94c"
                        strokeWidth={2}
                        fill="rgba(245,185,76,0.18)"
                        connectNulls
                      />
                    </ComposedChart>
                  </ResponsiveContainer>
                </div>
                <p className="mt-2 text-xs text-text-secondary">
                  100 % = alle Live-Kanäle des Marktes gehören zum Netzwerk; die Marktgröße dazu
                  zeigt das Viewer-Panel darunter.
                </p>
              </div>
              <div>
                <p className="text-xs uppercase tracking-[0.18em] text-text-secondary">
                  Viewer im Markt — Netzwerk vs. Rest
                </p>
                <div className="mt-3 h-52">
                  <ResponsiveContainer width="100%" height="100%">
                    <ComposedChart data={chartRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis
                        dataKey="label"
                        stroke="#9bb3c5"
                        tickLine={false}
                        axisLine={false}
                        minTickGap={48}
                      />
                      <YAxis stroke="#9bb3c5" tickLine={false} axisLine={false} />
                      <Tooltip
                        contentStyle={{
                          background: '#0f2431',
                          border: '1px solid rgba(255,255,255,0.1)',
                          borderRadius: '16px',
                        }}
                      />
                      <Legend />
                      <Area
                        type="monotone"
                        dataKey="Netzwerk"
                        stackId="viewers"
                        stroke="#10b7ad"
                        fill="rgba(16,183,173,0.45)"
                      />
                      <Area
                        type="monotone"
                        dataKey="Rest"
                        stackId="viewers"
                        stroke="#3a566b"
                        fill="rgba(58,86,107,0.30)"
                      />
                    </ComposedChart>
                  </ResponsiveContainer>
                </div>
              </div>
              <div>
                <p className="text-xs uppercase tracking-[0.18em] text-text-secondary">
                  Kanal-Anteil des Netzwerks
                </p>
                <div className="mt-3 h-52">
                  <ResponsiveContainer width="100%" height="100%">
                    <ComposedChart data={chartRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis
                        dataKey="label"
                        stroke="#9bb3c5"
                        tickLine={false}
                        axisLine={false}
                        minTickGap={48}
                      />
                      <YAxis stroke="#7aa2f7" tickLine={false} axisLine={false} unit="%" />
                      <Tooltip
                        contentStyle={{
                          background: '#0f2431',
                          border: '1px solid rgba(255,255,255,0.1)',
                          borderRadius: '16px',
                        }}
                      />
                      <Area
                        type="monotone"
                        dataKey="Kanal-Anteil %"
                        stroke="#7aa2f7"
                        strokeWidth={2}
                        fill="rgba(122,162,247,0.16)"
                        connectNulls
                      />
                    </ComposedChart>
                  </ResponsiveContainer>
                </div>
                <p className="mt-2 text-xs text-text-secondary">
                  Wie viele der gleichzeitig live geschalteten Markt-Kanäle gehören zum Netzwerk —
                  unabhängig von deren Viewerzahl.
                </p>
              </div>
              <div>
                <p className="text-xs uppercase tracking-[0.18em] text-text-secondary">
                  Live-Kanäle im Markt — Netzwerk vs. Rest
                </p>
                <div className="mt-3 h-52">
                  <ResponsiveContainer width="100%" height="100%">
                    <ComposedChart data={chartRows}>
                      <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                      <XAxis
                        dataKey="label"
                        stroke="#9bb3c5"
                        tickLine={false}
                        axisLine={false}
                        minTickGap={48}
                      />
                      <YAxis stroke="#9bb3c5" tickLine={false} axisLine={false} />
                      <Tooltip
                        contentStyle={{
                          background: '#0f2431',
                          border: '1px solid rgba(255,255,255,0.1)',
                          borderRadius: '16px',
                        }}
                      />
                      <Legend />
                      <Area
                        type="monotone"
                        dataKey="Netzwerk-Kanäle"
                        stackId="streams"
                        stroke="#10b7ad"
                        fill="rgba(16,183,173,0.45)"
                      />
                      <Area
                        type="monotone"
                        dataKey="Rest-Kanäle"
                        stackId="streams"
                        stroke="#3a566b"
                        fill="rgba(58,86,107,0.30)"
                      />
                    </ComposedChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </div>
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
          {scope === 'german' ? 'Live: Top-Streams im DE-Markt' : 'Live: Top-Streams der Kategorie'}
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
                      {stream.language ? stream.language.toUpperCase() : stream.isGerman ? 'DE' : '—'}
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

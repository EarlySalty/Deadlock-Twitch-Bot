import { useLanguage } from '@/context/LanguageContext';
import { useMemo, useState } from 'react';
import { Rise } from '../motion/Rise';
import {
  FlaskConical,
  Gamepad2,
  BarChart3,
  TrendingUp,
  Users,
  Loader2,
  AlertCircle,
  ArrowRight,
} from 'lucide-react';
import {
  BarChart,
  Bar,
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  Cell,
} from 'recharts';
import {
  useExpOverview,
  useExpGameBreakdown,
  useExpGameTransitions,
  useExpGrowthCurves,
} from '@/hooks/useAnalytics';
import { formatNumber } from '@/utils/formatters';
import type { TimeRange, ExpGameBreakdown, ExpGrowthCurve } from '@/types/analytics';

interface ExperimentalProps {
  streamer: string | null;
  days: TimeRange;
}

// ---------------------------------------------------------------------------
//  Game Color System
//  - Deadlock / Just Chatting → first two categorical brand colors
//  - Top-3 by avg viewers → Gold / Silver / Bronze (always, regardless of sort)
//  - All other games → hash-based color from palette (same game = same color)
// ---------------------------------------------------------------------------
const SPECIAL_COLORS: Record<string, string> = {
  'Deadlock':      '#C5A059',
  'Just Chatting': '#00D9FF',
};

const HASH_PALETTE = [
  '#C5A059', '#00D9FF', '#FF5A3C', '#00C46A', '#B5A488', 'var(--color-warning)',
  '#C5A059', '#00D9FF', '#FF5A3C', '#00C46A', '#B5A488', 'var(--color-warning)',
];

function hashGameColor(name: string | null | undefined): string {
  if (!name) return HASH_PALETTE[0];
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
  return HASH_PALETTE[h % HASH_PALETTE.length];
}

// Medal tiers are always computed from avgViewers, independent of current sort
const MEDAL = ['#C5A059', '#B5A488', '#FF5A3C'] as const; // gold / silver / bronze

type SortKey = 'avgViewers' | 'sessions' | 'peakViewers';

const SORT_OPTIONS: { key: SortKey; label: string }[] = [
  { key: 'avgViewers',  label: 'Ø Viewer' },
  { key: 'sessions',    label: 'Sessions' },
  { key: 'peakViewers', label: 'Peak'     },
];

function resolveColor(game: string, medalRank: number): string {
  if (SPECIAL_COLORS[game]) return SPECIAL_COLORS[game];
  if (medalRank >= 0 && medalRank < 3) return MEDAL[medalRank];
  return hashGameColor(game);
}

function ExpGameBreakdownChart({ data }: { data: ExpGameBreakdown[] }) {
  const { t, locale } = useLanguage();
  const [sortKey, setSortKey] = useState<SortKey>('avgViewers');

  const { chartData, medalGames } = useMemo(() => {
    // Medal tier always based on avgViewers regardless of active sort
    const byViewers = [...data]
      .filter(d => !SPECIAL_COLORS[d.game])
      .sort((a, b) => b.avgViewers - a.avgViewers)
      .map(d => d.game);

    const sorted = [...data].sort((a, b) => b[sortKey] - a[sortKey]);
    const top12  = sorted.slice(0, 12);

    const chartData = top12.map(item => {
      const medalRank = byViewers.indexOf(item.game);
      return {
        ...item,
        fill:      resolveColor(item.game, medalRank),
        shortGame: item.game.length > 18 ? item.game.slice(0, 16) + '…' : item.game,
        isSpecial: !!SPECIAL_COLORS[item.game],
        medal:     medalRank >= 0 && medalRank < 3 ? (['🥇','🥈','🥉'] as const)[medalRank] : null,
      };
    });

    return { chartData, medalGames: byViewers.slice(0, 3) };
  }, [data, sortKey]);

  if (!chartData.length) {
    return (
      <div className="flex items-center justify-center h-40 text-text-secondary text-sm">{t("Noch keine Daten vorhanden")}</div>
    );
  }

  return (
    <div className="space-y-3">
      {/* Sort Controls */}
      <div className="flex items-center gap-1.5 flex-wrap">
        <span className="text-xs text-text-secondary mr-1">{t("Sortiert nach:")}</span>
        {SORT_OPTIONS.map(opt => (
          <button
            key={opt.key}
            onClick={() => setSortKey(opt.key)}
            className={[
              'px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors',
              sortKey === opt.key
                ? 'bg-primary/20 text-primary border border-primary/40'
                : 'text-text-secondary hover:text-white border border-transparent hover:border-border',
            ].join(' ')}
          >
            {t(opt.label)}
          </button>
        ))}
        {/* Legend dots */}
        <div className="ml-auto flex items-center gap-3 text-xs text-text-secondary">
          {medalGames[0] && (
            <span className="flex items-center gap-1">
              <span className="inline-block w-2 h-2 rounded-full" style={{ backgroundColor: MEDAL[0] }} />
              {medalGames[0].length > 12 ? medalGames[0].slice(0, 10) + '…' : medalGames[0]}
            </span>
          )}
          {Object.entries(SPECIAL_COLORS).map(([name, color]) =>
            data.some(d => d.game === name) ? (
              <span key={name} className="flex items-center gap-1">
                <span className="inline-block w-2 h-2 rounded-full" style={{ backgroundColor: color }} />
                {name}
              </span>
            ) : null
          )}
        </div>
      </div>

      {/* key={sortKey} forces full remount on sort change → no Recharts 3.x animation-interpolation crash */}
      <ResponsiveContainer width="100%" height={280}>
        <BarChart key={sortKey} data={chartData} margin={{ top: 5, right: 20, left: 0, bottom: 60 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.06)" />
          <XAxis
            dataKey="shortGame"
            tick={{ fill: '#B5A488', fontSize: 11 }}
            angle={-35}
            textAnchor="end"
            interval={0}
          />
          <YAxis tick={{ fill: '#B5A488', fontSize: 11 }} tickFormatter={(value) => Number(value).toLocaleString(locale)} />
          <Tooltip
            contentStyle={{
              backgroundColor: 'var(--color-card)',
              border: '1px solid rgba(255,255,255,0.1)',
              borderRadius: 8,
              color: '#fff',
            }}
            formatter={((value: number | undefined, _name: string, props: any) => {
              const medal = props?.payload?.medal ?? '';
              const label = sortKey === 'avgViewers'
                ? t("Ø Viewer")
                : sortKey === 'sessions'
                  ? t("Sessions")
                  : t("Peak Viewer");
              return [`${formatNumber(value, 1, locale)} ${medal}`.trim(), label];
            }) as any}
          />
          <Bar dataKey={sortKey} name={t("Wert")} radius={[4, 4, 0, 0]} isAnimationActive={false}>
            {chartData.map((entry, i) => (
              <Cell key={i} fill={entry.fill} fillOpacity={entry.isSpecial ? 1 : 0.85} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}

function ExpGrowthCurvesChart({ data }: { data: ExpGrowthCurve[] }) {
  const { t, locale } = useLanguage();
  // Filter out null/undefined/empty game names to prevent Recharts 3.x toString crash
  const games = useMemo(
    () => [...new Set(data.map(d => d.game).filter((g): g is string => !!g))].slice(0, 6),
    [data],
  );

  const pivoted = useMemo(() => {
    const byMinute: Record<number, Record<string, number>> = {};
    for (const point of data) {
      if (!games.includes(point.game)) continue;
      if (!byMinute[point.minuteFromStart]) byMinute[point.minuteFromStart] = {};
      byMinute[point.minuteFromStart][point.game] = point.avgViewers;
    }
    return Object.entries(byMinute)
      .sort(([a], [b]) => Number(a) - Number(b))
      .map(([min, vals]) => ({ minute: Number(min), ...vals }));
  }, [data, games]);

  if (!pivoted.length) {
    return (
      <div className="flex items-center justify-center h-40 text-text-secondary text-sm">{t("Noch keine Verlaufsdaten vorhanden")}</div>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={280}>
      <LineChart data={pivoted} margin={{ top: 5, right: 20, left: 0, bottom: 5 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.06)" />
        <XAxis
          dataKey="minute"
          tick={{ fill: '#B5A488', fontSize: 11 }}
          tickFormatter={(v) => `${v}m`}
        />
        <YAxis tick={{ fill: '#B5A488', fontSize: 11 }} tickFormatter={(value) => Number(value).toLocaleString(locale)} />
        <Tooltip
          contentStyle={{
            backgroundColor: 'var(--color-card)',
            border: '1px solid rgba(255,255,255,0.1)',
            borderRadius: 8,
            color: '#fff',
          }}
          labelFormatter={(v) => t("Minute {value0}", { value0: String(v) })}
          formatter={((value: number) => [formatNumber(value, 1, locale), t("Ø Viewer")]) as any}
        />
        <Legend wrapperStyle={{ color: '#B5A488', fontSize: 12 }} />
        {games.map((game) => (
          <Line
            key={game}
            type="monotone"
            dataKey={game}
            name={game && game.length > 20 ? game.slice(0, 18) + '…' : (game ?? '')}
            stroke={hashGameColor(game)}
            strokeWidth={2}
            dot={false}
            activeDot={{ r: 4 }}
            isAnimationActive={false}
            connectNulls={false}
          />
        ))}
      </LineChart>
    </ResponsiveContainer>
  );
}

function KpiCard({
  label,
  value,
  sub,
  icon: Icon,
}: {
  label: string;
  value: string | number;
  sub?: string;
  icon: React.ComponentType<{ className?: string }>;
}) {
  return (
    <Rise
      className="bg-card rounded-xl border border-border p-5 flex min-w-0 flex-col gap-2"
    >
      <div className="flex items-center gap-2 text-text-secondary text-sm">
        <Icon className="w-4 h-4 text-primary" />
        <span>{label}</span>
      </div>
      <p className="text-2xl font-bold text-white break-words">{value}</p>
      {sub && <p className="text-xs text-text-secondary">{sub}</p>}
    </Rise>
  );
}

export function Experimental({ streamer, days }: ExperimentalProps) {
  const { t, locale } = useLanguage();
  const { data: overview, isLoading: loadingOverview } = useExpOverview(streamer, days);
  const { data: breakdown = [], isLoading: loadingBreakdown } = useExpGameBreakdown(streamer, days);
  const { data: transitions = [], isLoading: loadingTransitions } = useExpGameTransitions(streamer, days);
  const { data: growthCurves = [], isLoading: loadingCurves } = useExpGrowthCurves(streamer, days);

  const isAnyLoading = loadingOverview || loadingBreakdown || loadingTransitions || loadingCurves;

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <AlertCircle className="w-10 h-10 text-text-secondary" />
        <p className="text-text-secondary text-lg">{t("Wähle einen Streamer aus")}</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <FlaskConical className="w-6 h-6 text-accent" />
        <div>
          <h2 className="text-xl font-bold text-white flex flex-wrap items-center gap-2">{t("Labor – Experimentelle Analytics")}<span className="text-xs font-semibold px-2 py-0.5 rounded-full bg-accent/20 text-accent border border-accent/30">
              Beta
            </span>
          </h2>
          <p className="text-sm text-text-secondary mt-0.5">{t("Alle Spiele – Session-Tracking jenseits von Deadlock")}</p>
        </div>
        {isAnyLoading && <Loader2 className="w-5 h-5 animate-spin text-primary ml-auto" />}
      </div>

      {/* KPI Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <KpiCard
          label={t("Sessions")}
          value={overview ? formatNumber(overview.totalSessions, 0, locale) : '–'}
          icon={BarChart3}
          sub={t("letzte {value0} Tage", { value0: days })}
        />
        <KpiCard
          label={t("Spiele gespielt")}
          value={overview ? formatNumber(overview.gamesPlayed, 0, locale) : '–'}
          icon={Gamepad2}
        />
        <KpiCard
          label={t("Bestes Spiel")}
          value={overview?.bestGame || '–'}
          sub={overview?.bestGameAvgViewers ? t("Ø {value0} Viewer", { value0: formatNumber(overview.bestGameAvgViewers, 1, locale) }) : undefined}
          icon={TrendingUp}
        />
        <KpiCard
          label={t("Ø Viewer")}
          value={overview ? formatNumber(overview.avgViewers, 1, locale) : '–'}
          icon={Users}
          sub={t("alle Spiele")}
        />
      </div>

      {/* Game Breakdown Bar Chart */}
      <Rise
        step={{ seconds: 0.1 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
          <BarChart3 className="w-4 h-4 text-primary" />{t("Ø Viewer pro Spiel")}</h3>
        {loadingBreakdown ? (
          <div className="flex items-center justify-center h-40">
            <Loader2 className="w-6 h-6 animate-spin text-primary" />
          </div>
        ) : (
          <ExpGameBreakdownChart data={breakdown} />
        )}
      </Rise>

      {/* Growth Curves */}
      <Rise
        step={{ seconds: 0.15 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
          <TrendingUp className="w-4 h-4 text-primary" />{t("Viewer-Verlauf pro Spiel (Minuten ab Streamstart)")}</h3>
        {loadingCurves ? (
          <div className="flex items-center justify-center h-40">
            <Loader2 className="w-6 h-6 animate-spin text-primary" />
          </div>
        ) : (
          <ExpGrowthCurvesChart data={growthCurves} />
        )}
      </Rise>

      {/* Game Transitions Table */}
      <Rise
        step={{ seconds: 0.2 }}
        className="bg-card rounded-xl border border-border p-5"
      >
        <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
          <ArrowRight className="w-4 h-4 text-primary" />{t("Spielwechsel")}</h3>
        {loadingTransitions ? (
          <div className="flex items-center justify-center h-32">
            <Loader2 className="w-6 h-6 animate-spin text-primary" />
          </div>
        ) : transitions.length === 0 ? (
          <p className="text-text-secondary text-sm text-center py-8">{t("Noch keine Spielwechsel aufgezeichnet")}</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-text-secondary border-b border-border">
                  <th className="text-left py-2 pr-4 font-medium">{t("Von")}</th>
                  <th className="text-left py-2 pr-4 font-medium">{t("Nach")}</th>
                  <th className="text-right py-2 pr-4 font-medium">{t("Anzahl")}</th>
                  <th className="text-right py-2 font-medium">{t("Ø Viewer")}</th>
                </tr>
              </thead>
              <tbody>
                {transitions.slice(0, 20).map((t, i) => (
                  <tr
                    key={i}
                    className="border-b border-border/40 hover:bg-white/5 transition-colors"
                  >
                    <td className="py-2 pr-4 text-text-secondary truncate max-w-[160px]">
                      {t.fromGame}
                    </td>
                    <td className="py-2 pr-4 text-white truncate max-w-[160px]">
                      {t.toGame}
                    </td>
                    <td className="py-2 pr-4 text-right text-white font-mono">
                      {formatNumber(t.count, 0, locale)}
                    </td>
                    <td className="py-2 text-right text-text-secondary font-mono">
                      {formatNumber(t.avgViewersBefore, 0, locale)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Rise>

      {/* Per-Game Stats Table */}
      {breakdown.length > 0 && (
        <Rise
          step={{ seconds: 0.25 }}
          className="bg-card rounded-xl border border-border p-5"
        >
          <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
            <Gamepad2 className="w-4 h-4 text-primary" />
            Spiel-Statistiken
          </h3>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-text-secondary border-b border-border">
                  <th className="text-left py-2 pr-4 font-medium">Spiel</th>
                  <th className="text-right py-2 pr-4 font-medium">Sessions</th>
                  <th className="text-right py-2 pr-4 font-medium">Ø Viewer</th>
                  <th className="text-right py-2 pr-4 font-medium">Peak</th>
                  <th className="text-right py-2 pr-4 font-medium">Ø Dauer</th>
                  <th className="text-right py-2 font-medium">Ø Follower</th>
                </tr>
              </thead>
              <tbody>
                {breakdown.map((row, i) => (
                  <tr
                    key={i}
                    className="border-b border-border/40 hover:bg-white/5 transition-colors"
                  >
                    <td className="py-2 pr-4 flex items-center gap-2">
                      <span
                        className="inline-block w-2.5 h-2.5 rounded-full flex-shrink-0"
                        style={{ backgroundColor: hashGameColor(row.game) }}
                      />
                      <span className="text-white truncate max-w-[180px]">{row.game}</span>
                    </td>
                    <td className="py-2 pr-4 text-right text-text-secondary font-mono">
                      {formatNumber(row.sessions, 0, locale)}
                    </td>
                    <td className="py-2 pr-4 text-right text-white font-mono">
                      {formatNumber(row.avgViewers, 1, locale)}
                    </td>
                    <td className="py-2 pr-4 text-right text-text-secondary font-mono">
                      {formatNumber(row.peakViewers, 0, locale)}
                    </td>
                    <td className="py-2 pr-4 text-right text-text-secondary font-mono">
                      {formatNumber(row.avgDurationMin, 0, locale)}m
                    </td>
                    <td className="py-2 text-right text-text-secondary font-mono">
                      {row.avgFollowerDelta >= 0 ? '+' : ''}
                      {formatNumber(row.avgFollowerDelta, 1, locale)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Rise>
      )}
    </div>
  );
}

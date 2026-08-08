import { motion } from 'framer-motion';
import { Area, AreaChart, ResponsiveContainer, Tooltip, YAxis } from 'recharts';
import {
  ArrowDownRight,
  ArrowUpRight,
  Crown,
  Flame,
  MessageSquare,
  Timer,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { formatNumber } from '@/utils/formatters';
import type {
  InternalHomeComparisonMetric,
  InternalHomeLastStreamSummary,
  InternalHomePersonalBestEntry,
  InternalHomePersonalBests,
  InternalHomeStreamComparison,
  InternalHomeViewersOverTimePoint,
} from '@/api/home';

// Marken-Gold (Industrial Gold, 0xC8A86B) fuer den Verlauf statt StreamElements-Blau.
const GOLD = '200, 168, 107';

function formatDurationShort(seconds: number | null | undefined): string {
  if (seconds == null) return '–';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function achievedLabel(iso: string | null): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleDateString('de-DE', { day: '2-digit', month: 'short', year: 'numeric' });
}

/** Signiertes Delta: gruen bei +, rot bei -, nichts bei Nullbasis (pct === null). */
function DeltaBadge({ pct }: { pct: number | null }) {
  if (pct == null) {
    return <span className="text-xs font-medium text-text-secondary">neu</span>;
  }
  const up = pct >= 0;
  const Icon = up ? ArrowUpRight : ArrowDownRight;
  return (
    <span
      className={`inline-flex items-center gap-0.5 text-xs font-bold ${
        up ? 'text-success' : 'text-danger'
      }`}
    >
      <Icon className="h-3.5 w-3.5" />
      {up ? '+' : ''}
      {formatNumber(pct)}%
    </span>
  );
}

function ComparisonRow({
  label,
  icon: Icon,
  metric,
}: {
  label: string;
  icon: LucideIcon;
  metric: InternalHomeComparisonMetric;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2.5">
      <div className="flex items-center gap-2.5">
        <div className="icon-duotone flex h-7 w-7 items-center justify-center rounded-lg border border-border bg-background/60 text-text-secondary">
          <Icon className="h-3.5 w-3.5" />
        </div>
        <span className="text-sm font-medium text-text-secondary">{label}</span>
      </div>
      <div className="flex items-center gap-3">
        <span className="kpi-number text-lg font-bold text-white">
          {metric.current != null ? formatNumber(metric.current) : '–'}
        </span>
        <DeltaBadge pct={metric.pct} />
      </div>
    </div>
  );
}

function BestTile({
  label,
  icon: Icon,
  entry,
  display,
  isRecord,
}: {
  label: string;
  icon: LucideIcon;
  entry: InternalHomePersonalBestEntry | null;
  display: string;
  isRecord: boolean;
}) {
  const achieved = achievedLabel(entry?.achieved_at ?? null);
  return (
    <div className="group relative overflow-hidden rounded-xl border border-border bg-background/55 p-3 transition-[transform,translate,scale,border-color,background-color,box-shadow] duration-200 hover:-translate-y-0.5 hover:border-border-hover hover:bg-background/75">
      <div
        className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
        style={{ background: `radial-gradient(120% 80% at 50% 0%, rgba(${GOLD}, 0.22), transparent 60%)` }}
      />
      <div className="mb-2 flex items-center justify-between">
        <div
          className="icon-duotone flex h-7 w-7 items-center justify-center rounded-lg border"
          style={{
            background: `rgba(${GOLD}, 0.14)`,
            borderColor: `rgba(${GOLD}, 0.28)`,
            color: `rgb(${GOLD})`,
          }}
        >
          <Icon className="h-3.5 w-3.5" />
        </div>
        {isRecord ? (
          <span
            className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide"
            style={{ background: `rgba(${GOLD}, 0.16)`, color: `rgb(${GOLD})` }}
          >
            <Flame className="h-3 w-3" />
            Neuer Rekord
          </span>
        ) : null}
      </div>
      <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</div>
      <div
        className="kpi-number mt-0.5 text-xl font-bold text-white"
        style={{ textShadow: `0 0 18px rgba(${GOLD}, 0.5)` }}
      >
        {display}
      </div>
      {achieved ? <div className="mt-1 text-[11px] text-text-secondary">am {achieved}</div> : null}
    </div>
  );
}

/**
 * Persoenliche Bestwerte + Stream-zu-Stream-Recap + Zuschauerverlauf.
 * Vibe angelehnt an die StreamElements-Stream-Summary, aber im Gold/Glass-Look.
 * "Neuer Rekord" nur bei Metriken, die der letzte Stream vergleichbar liefert
 * (Peak/Oe-Viewer/Follower/Dauer) -- NICHT bei Chat (messages vs. unique_chatters).
 */
export function StreamRecapCard({
  personalBests,
  streamComparison,
  viewersOverTime,
  lastStream,
  delay = 0,
}: {
  personalBests: InternalHomePersonalBests | null;
  streamComparison: InternalHomeStreamComparison | null;
  viewersOverTime: InternalHomeViewersOverTimePoint[] | null;
  lastStream: InternalHomeLastStreamSummary | null;
  delay?: number;
}) {
  const pb = personalBests;
  const hasBests =
    pb != null &&
    [pb.peak_viewers, pb.avg_viewers, pb.follower_gain, pb.unique_chatters, pb.longest_stream_seconds].some(
      (entry) => entry?.value != null
    );
  const points = viewersOverTime ?? [];
  const hasChart = points.length > 1;

  // Nichts anzuzeigen -> Karte ganz weglassen (kein leeres Panel).
  if (!hasBests && !streamComparison && !hasChart) {
    return null;
  }

  // "Neuer Rekord": letzter Stream haelt den Bestwert (PB = MAX inkl. letztem Stream,
  // also value >= PB-value == der letzte Stream IST der Rekord). Chat ausgenommen.
  const beats = (last: number | null | undefined, best: number | null | undefined, eps = 0) =>
    last != null && best != null && last >= best - eps;
  const peakRecord = beats(lastStream?.peak_viewers, pb?.peak_viewers.value);
  const avgRecord = beats(lastStream?.avg_viewers, pb?.avg_viewers.value, 0.05);
  const followerRecord = beats(lastStream?.follower_delta, pb?.follower_gain.value);
  const durationRecord = beats(lastStream?.duration_seconds, pb?.longest_stream_seconds.value);

  return (
    <motion.section
      className="panel-card card-glow rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.32, delay }}
    >
      <div className="text-[11px] font-semibold uppercase tracking-[0.22em]" style={{ color: `rgb(${GOLD})` }}>
        Dein Kanal in Rekorden
      </div>
      <h2 className="mt-1 text-xl font-semibold text-white">
        Pers&ouml;nliche Bestwerte <span className="text-text-secondary">(&#9685;&#8255;&#9685;)</span>
      </h2>
      <p className="mt-1 text-sm text-text-secondary">Deine Rekorde, seit wir mitschreiben.</p>

      {streamComparison ? (
        <div className="mt-5 rounded-xl border border-border bg-background/40 p-4">
          <div className="mb-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            So lief dein letzter Stream
          </div>
          <p className="mb-2 text-xs text-text-secondary">im Vergleich zum Stream davor</p>
          <div className="divide-y divide-border/70">
            <ComparisonRow label="Peak-Viewer" icon={TrendingUp} metric={streamComparison.peak_viewers} />
            <ComparisonRow label="Neue Follower" icon={Users} metric={streamComparison.new_followers} />
            <ComparisonRow label="Unique Chatter" icon={MessageSquare} metric={streamComparison.unique_chatters} />
          </div>
        </div>
      ) : null}

      {hasChart ? (
        <div className="mt-5">
          <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            Zuschauer im Verlauf
          </div>
          <div className="h-40 w-full">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={points} margin={{ top: 6, right: 6, bottom: 0, left: 0 }}>
                <defs>
                  <linearGradient id="recapViewerGold" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor={`rgba(${GOLD}, 0.55)`} />
                    <stop offset="100%" stopColor={`rgba(${GOLD}, 0.02)`} />
                  </linearGradient>
                </defs>
                <YAxis hide domain={[0, 'dataMax + 1']} />
                <Tooltip
                  cursor={{ stroke: `rgba(${GOLD}, 0.4)` }}
                  contentStyle={{
                    background: 'rgba(11, 11, 11, 0.92)',
                    border: `1px solid rgba(${GOLD}, 0.3)`,
                    borderRadius: 10,
                    fontSize: 12,
                  }}
                  labelFormatter={(t) => `${Math.round(Number(t) / 60)} min`}
                  formatter={(value) => [formatNumber(Number(value)), 'Zuschauer']}
                />
                <Area
                  type="monotone"
                  dataKey="viewers"
                  stroke={`rgb(${GOLD})`}
                  strokeWidth={2}
                  fill="url(#recapViewerGold)"
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </div>
      ) : null}

      {hasBests && pb ? (
        <div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
          <BestTile
            label="Peak-Viewer"
            icon={TrendingUp}
            entry={pb.peak_viewers}
            display={pb.peak_viewers.value != null ? formatNumber(pb.peak_viewers.value) : '–'}
            isRecord={peakRecord}
          />
          <BestTile
            label={'Ø-Viewer'}
            icon={Users}
            entry={pb.avg_viewers}
            display={pb.avg_viewers.value != null ? formatNumber(pb.avg_viewers.value) : '–'}
            isRecord={avgRecord}
          />
          <BestTile
            label="Follower (1 Stream)"
            icon={Crown}
            entry={pb.follower_gain}
            display={pb.follower_gain.value != null ? `+${formatNumber(pb.follower_gain.value)}` : '–'}
            isRecord={followerRecord}
          />
          <BestTile
            label="Aktivster Chat"
            icon={MessageSquare}
            entry={pb.unique_chatters}
            display={pb.unique_chatters.value != null ? formatNumber(pb.unique_chatters.value) : '–'}
            isRecord={false}
          />
          <BestTile
            label={'Längster Stream'}
            icon={Timer}
            entry={pb.longest_stream_seconds}
            display={formatDurationShort(pb.longest_stream_seconds.value)}
            isRecord={durationRecord}
          />
        </div>
      ) : null}
    </motion.section>
  );
}

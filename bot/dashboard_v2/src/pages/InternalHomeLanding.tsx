import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { useQuery } from '@tanstack/react-query';
import { Line, LineChart, ResponsiveContainer } from 'recharts';
import {
  fetchInternalHome,
  type InternalHomeActionEntry,
  type InternalHomeChangelogEntry,
} from '@/api/home';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';
import { formatNumber, formatDuration } from '@/utils/formatters';
import {
  ArrowRight,
  BarChart3,
  FileText,
  Film,
  Heart,
  Home,
  Loader2,
  MessageSquare,
  Settings,
  Sparkles,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { WelcomeTour } from '@/components/onboarding/WelcomeTour';

function MiniStat({
  label,
  value,
  prefix = '',
  icon: Icon,
  accent = 'primary',
}: {
  label: string;
  value: number | null | undefined;
  prefix?: string;
  icon?: LucideIcon;
  accent?: 'primary' | 'accent' | 'success' | 'warning';
}) {
  const accentColor = {
    primary: 'bg-primary/15 border-primary/25 text-primary',
    accent: 'bg-accent/15 border-accent/25 text-accent',
    success: 'bg-success/15 border-success/25 text-success',
    warning: 'bg-warning/15 border-warning/25 text-warning',
  }[accent];

  const glowRgb =
    accent === 'success'
      ? '46,204,113'
      : accent === 'warning'
        ? '245,182,66'
        : accent === 'accent'
          ? '168,85,247'
          : '6,182,212';
  return (
    <div className="group relative overflow-hidden rounded-xl border border-border bg-background/55 p-3 transition-all duration-200 hover:-translate-y-0.5 hover:border-border-hover hover:bg-background/75">
      <div
        className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
        style={{
          background: `radial-gradient(120% 80% at 50% 0%, rgba(${glowRgb}, 0.2), transparent 60%)`,
        }}
      />
      {Icon ? (
        <div className={`mb-2 flex h-7 w-7 items-center justify-center rounded-lg border ${accentColor}`}>
          <Icon className="h-3.5 w-3.5" />
        </div>
      ) : null}
      <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</div>
      <div
        className="mt-0.5 text-xl font-bold text-white"
        style={{ textShadow: `0 0 18px rgba(${glowRgb}, 0.55)` }}
      >
        {value != null ? `${prefix}${formatNumber(value)}` : '\u2013'}
      </div>
    </div>
  );
}

const WEEK_KPI_META: Record<string, { icon: LucideIcon }> = {
  '\u00D8 Viewer': { icon: Users },
  Follower: { icon: TrendingUp },
  'Chat-Aktivitaet': { icon: MessageSquare },
  'Stream-Stunden': { icon: BarChart3 },
};

function WeekKpi({
  label,
  current,
  change,
  suffix = '',
  series,
}: {
  label: string;
  current: number | null | undefined;
  change: number | null | undefined;
  suffix?: string;
  series?: number[];
}) {
  const meta = WEEK_KPI_META[label];
  const Icon = meta?.icon ?? BarChart3;
  const sparkData =
    series && series.length > 0 ? series.map((value, index) => ({ index, value })) : null;
  const trendUp = (change ?? 0) >= 0;
  const sparkColor = trendUp ? 'var(--color-success)' : 'var(--color-danger)';

  const glowToneClass = trendUp ? 'card-glow-success kpi-glow-success' : 'card-glow-danger kpi-glow-danger';
  return (
    <div className={`panel-card card-glow ${glowToneClass} kpi-glow-always internal-home-kpi rounded-xl p-4`}>
      <div className="mb-3 flex items-center gap-2.5">
        <div
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${
            trendUp
              ? 'bg-gradient-to-br from-success/40 to-teal/35 shadow-[0_0_18px_rgba(46,204,113,0.4)]'
              : 'bg-gradient-to-br from-danger/45 to-orange/35 shadow-[0_0_18px_rgba(255,107,94,0.4)]'
          }`}
        >
          <Icon className="h-4 w-4 text-white" />
        </div>
        <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</div>
      </div>
      <div className="text-2xl font-bold text-white kpi-value-glow">
        {current != null ? `${formatNumber(current)}${suffix}` : '\u2013'}
      </div>
      {change != null ? (
        <div className={`mt-1.5 text-xs font-semibold ${trendUp ? 'text-success' : 'text-danger'}`}>
          {trendUp ? '\u2191' : '\u2193'} {Math.abs(change).toFixed(1)}% vs. Vorwoche
        </div>
      ) : null}
      {sparkData ? (
        <div className="mt-3 h-9 w-full">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={sparkData} margin={{ top: 2, right: 2, bottom: 2, left: 2 }}>
              <Line
                type="monotone"
                dataKey="value"
                stroke={sparkColor}
                strokeWidth={2.2}
                dot={false}
                isAnimationActive={false}
                style={{ filter: `drop-shadow(0 0 6px ${sparkColor})` }}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      ) : null}
    </div>
  );
}

function BackgroundBlobs() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      <div className="absolute -top-32 right-[-8rem] h-[28rem] w-[28rem] rounded-full bg-primary/22 blur-3xl" />
      <div className="absolute top-[24%] -left-28 h-[22rem] w-[22rem] rounded-full bg-accent/24 blur-3xl" />
      <div className="absolute bottom-[-8rem] left-[34%] h-[24rem] w-[24rem] rounded-full bg-success/20 blur-3xl" />
    </div>
  );
}

function SkeletonBlock({ className = '' }: { className?: string }) {
  return <div className={`animate-pulse rounded-lg bg-white/6 ${className}`} />;
}

function DashboardSkeleton() {
  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <BackgroundBlobs />
      <div className="relative mx-auto max-w-[1440px]">
        <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
          <aside className="panel-card card-glow self-start rounded-2xl p-4">
            <div className="flex items-center gap-3">
              <SkeletonBlock className="h-10 w-10 shrink-0 rounded-full" />
              <div className="min-w-0 flex-1 space-y-2">
                <SkeletonBlock className="h-3 w-3/4" />
                <SkeletonBlock className="h-2.5 w-1/2" />
              </div>
            </div>
            <div className="mt-4 border-t border-border" />
            <div className="mt-4 space-y-2">
              <SkeletonBlock className="h-2.5 w-12" />
              <div className="space-y-1.5">
                {[0, 1, 2, 3].map((i) => (
                  <SkeletonBlock key={`nav-main-${i}`} className="h-9 w-full" />
                ))}
              </div>
            </div>
            <div className="mt-4 space-y-2">
              <SkeletonBlock className="h-2.5 w-12" />
              <div className="space-y-1.5">
                {[0, 1, 2].map((i) => (
                  <SkeletonBlock key={`nav-tools-${i}`} className="h-9 w-full" />
                ))}
              </div>
            </div>
          </aside>

          <main className="space-y-4 md:space-y-5">
            <section className="panel-card card-glow card-glow-accent hero-aura flex flex-col gap-4 rounded-2xl px-5 py-4 md:flex-row md:items-center md:justify-between">
              <div className="space-y-2.5">
                <SkeletonBlock className="h-3 w-40" />
                <SkeletonBlock className="h-8 w-64" />
                <SkeletonBlock className="h-3 w-48" />
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <SkeletonBlock className="h-10 w-32" />
                <SkeletonBlock className="h-10 w-44" />
              </div>
            </section>

            <section className="grid gap-4 lg:grid-cols-3">
              <div className="panel-card card-glow card-glow-soft rounded-2xl p-5">
                <div className="mb-5 flex items-center gap-3">
                  <SkeletonBlock className="h-9 w-9 rounded-xl" />
                  <div className="flex-1 space-y-2">
                    <SkeletonBlock className="h-2.5 w-24" />
                    <SkeletonBlock className="h-4 w-36" />
                  </div>
                </div>
                <div className="flex flex-col items-center">
                  <SkeletonBlock className="h-24 w-24 rounded-full" />
                </div>
                <div className="mt-5 space-y-3">
                  {[0, 1, 2, 3].map((i) => (
                    <div key={`hs-bar-${i}`} className="flex items-center gap-3">
                      <SkeletonBlock className="h-8 w-8 rounded-lg" />
                      <div className="min-w-0 flex-1 space-y-2">
                        <SkeletonBlock className="h-3 w-full" />
                        <SkeletonBlock className="h-1.5 w-full" />
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              <div className="panel-card card-glow rounded-2xl p-5 lg:col-span-2">
                <SkeletonBlock className="h-3 w-28" />
                <SkeletonBlock className="mt-2 h-6 w-72" />
                <SkeletonBlock className="mt-2 h-3 w-48" />
                <div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {[0, 1, 2, 3].map((i) => (
                    <div key={`ls-stat-${i}`} className="rounded-xl border border-border bg-background/50 p-3">
                      <SkeletonBlock className="mb-2 h-7 w-7 rounded-lg" />
                      <SkeletonBlock className="h-2.5 w-16" />
                      <SkeletonBlock className="mt-2 h-5 w-12" />
                    </div>
                  ))}
                </div>
              </div>
            </section>

            <section className="grid grid-cols-2 gap-4 md:grid-cols-4">
              {[0, 1, 2, 3].map((i) => (
                <div key={`week-kpi-${i}`} className="panel-card card-glow rounded-xl p-4">
                  <div className="mb-3 flex items-center gap-2.5">
                    <SkeletonBlock className="h-8 w-8 rounded-lg" />
                    <SkeletonBlock className="h-2.5 w-20" />
                  </div>
                  <SkeletonBlock className="h-7 w-16" />
                  <SkeletonBlock className="mt-2 h-3 w-28" />
                </div>
              ))}
            </section>

            <section className="grid gap-4 lg:grid-cols-2">
              {[0, 1].map((i) => (
                <div key={`bottom-${i}`} className="panel-card card-glow rounded-2xl p-5 md:p-6">
                  <SkeletonBlock className="h-3 w-20" />
                  <SkeletonBlock className="mt-2 h-6 w-44" />
                  <div className="mt-4 space-y-2.5">
                    {[0, 1, 2, 3].map((j) => (
                      <div key={`row-${i}-${j}`} className="rounded-xl border border-border bg-background/55 p-3.5">
                        <div className="flex items-center gap-2">
                          <SkeletonBlock className="h-5 w-24 rounded-full" />
                          <SkeletonBlock className="h-5 w-20 rounded-full" />
                          <SkeletonBlock className="h-5 w-14 rounded-full" />
                        </div>
                        <SkeletonBlock className="mt-2 h-3 w-3/4" />
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </section>
          </main>
        </div>
      </div>
    </div>
  );
}

function SidebarLink({
  href,
  icon: Icon,
  label,
  active = false,
}: {
  href: string;
  icon: LucideIcon;
  label: string;
  active?: boolean;
}) {
  const activeClasses =
    'border border-primary/25 bg-primary/10 text-primary lg:rounded-l-none lg:border-y-0 lg:border-r-0 lg:border-t-0 lg:border-l-2 lg:border-primary lg:pl-2.5';
  const inactiveClasses = 'border border-transparent text-text-secondary hover:bg-white/5 hover:text-white';

  return (
    <a
      href={href}
      className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-semibold no-underline transition-colors whitespace-nowrap ${active ? activeClasses : inactiveClasses}`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span>{label}</span>
    </a>
  );
}

interface SidebarNavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active?: boolean;
}

const INTERNAL_HOME_BOT_MODERATOR_LOGIN = 'deutschedeadlockcommunity';

function initialInternalHomeStreamer(): string | null {
  const params = new URLSearchParams(window.location.search);
  const streamer = params.get('streamer')?.trim().toLowerCase() || '';
  return streamer || null;
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return 'Unbekannt';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unbekannt';
  return date.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatCalendarDate(value: string | null | undefined): string {
  if (!value) return 'Unbekannt';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unbekannt';
  return date.toLocaleDateString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  });
}

function formatDateWithTime(iso: string | null | undefined): string {
  if (!iso) return '\u2013';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '\u2013';
  return date.toLocaleDateString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatDurationFromSeconds(seconds: number | null | undefined): string {
  if (!seconds) return '\u2013';
  return formatDuration(seconds);
}

function formatRelativeShort(value: string | null | undefined): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  const diffMs = Date.now() - date.getTime();
  const diffMin = Math.round(diffMs / 60000);
  if (diffMin < 1) return 'gerade eben';
  if (diffMin < 60) return `vor ${diffMin} min`;
  const diffH = Math.round(diffMin / 60);
  if (diffH < 24) return `vor ${diffH} h`;
  const diffD = Math.round(diffH / 24);
  if (diffD < 14) return `vor ${diffD} Tagen`;
  return formatCalendarDate(value);
}

function actionLogTone(entry: InternalHomeActionEntry): {
  label: string;
  badgeClass: string;
} {
  const severity = String(entry.severity || 'info').toLowerCase();
  if (severity === 'critical' || severity === 'error') {
    return { label: 'Kritisch', badgeClass: 'border-error/35 bg-error/10 text-error' };
  }
  if (severity === 'warning') {
    return { label: 'Warnung', badgeClass: 'border-warning/35 bg-warning/10 text-warning' };
  }
  if (severity === 'success') {
    return { label: 'Positiv', badgeClass: 'border-success/35 bg-success/10 text-success' };
  }
  return { label: 'Info', badgeClass: 'border-accent/35 bg-accent/10 text-accent' };
}

function formatActionUser(entry: InternalHomeActionEntry): string {
  const targetLogin = entry.targetLogin?.trim();
  const actorLogin = entry.actorLogin?.trim();
  if (targetLogin) return `@${targetLogin}`;
  if (actorLogin) return `@${actorLogin}`;
  return 'System';
}

function isBanAction(entry: InternalHomeActionEntry): boolean {
  const haystack = [
    entry.eventType,
    entry.statusLabel,
    entry.title,
    entry.summary,
    entry.reason,
    entry.description,
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  return haystack.includes('ban') || haystack.includes('banned') || haystack.includes('gebannt');
}

function isServicePitchWarningAction(entry: InternalHomeActionEntry): boolean {
  const haystack = [
    entry.eventType,
    entry.statusLabel,
    entry.title,
    entry.summary,
    entry.reason,
    entry.description,
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  return (
    haystack.includes('service_pitch_warning') ||
    haystack.includes('service-pitch') ||
    haystack.includes('service pitch') ||
    haystack.includes('pitch warn')
  );
}

function stripActionNoise(value: string): string {
  return value
    .replace(/auto[_\s-]*raid[_\s-]*on[_\s-]*offline/gi, '')
    .replace(/auto[_\s-]*offline[_\s-]*raid/gi, '')
    .replace(/\s{2,}/g, ' ')
    .replace(/\s+([,.;:!?])/g, '$1')
    .trim();
}

function stripActionModeratorSegment(value: string): string {
  return value
    .replace(/\|\s*mod(?:erator)?\s*:\s*@?[a-z0-9_]+/gi, '')
    .replace(/\bmod(?:erator)?\s*:\s*@?[a-z0-9_]+/gi, '')
    .replace(/\s{2,}/g, ' ')
    .replace(/\s+([,.;:!?])/g, '$1')
    .trim();
}

function splitActionDetailSegments(value: string | null | undefined): string[] {
  if (!value) return [];
  return value
    .split('|')
    .map((segment) => stripActionNoise(segment.trim()))
    .filter(Boolean);
}

function normalizeActionEventType(entry: InternalHomeActionEntry): string {
  return String(entry.eventType || '').trim().toLowerCase();
}

function isVisibleChannelAction(entry: InternalHomeActionEntry, channelLogin: string): boolean {
  const eventType = normalizeActionEventType(entry);
  if (!eventType) return false;
  if (eventType === 'ban' || eventType === 'ban_keyword_hit' || eventType === 'unban') return true;
  if (eventType === 'raid' || eventType === 'raid_history') return true;
  if (eventType === 'service_pitch_warning') {
    if (!channelLogin) return true;
    const actorLogin = String(entry.actorLogin || '').trim().toLowerCase();
    if (!actorLogin) return true;
    return actorLogin === channelLogin;
  }
  return false;
}

function buildPriorityActionDetails(
  entry: InternalHomeActionEntry,
  isServicePitchWarning: boolean
): string[] {
  const detailLines: string[] = [];
  const summary = stripActionNoise(stripActionModeratorSegment(entry.summary?.trim() || ''));
  const targetLogin = entry.targetLogin?.trim() || '';
  const actorLogin = entry.actorLogin?.trim() || '';
  const metric = stripActionNoise(entry.metric?.trim() || '');
  const reason = stripActionNoise(entry.reason?.trim() || '');

  if (summary) detailLines.push(summary);
  if (targetLogin) detailLines.push(`Nutzer: @${targetLogin}`);
  if (isServicePitchWarning) {
    if (actorLogin) detailLines.push(`Kanal: @${actorLogin}`);
  } else {
    detailLines.push(`Moderator: @${INTERNAL_HOME_BOT_MODERATOR_LOGIN}`);
  }
  if (metric) detailLines.push(`Metrik: ${metric}`);
  if (reason) detailLines.push(`Grund: ${reason}`);
  detailLines.push(...splitActionDetailSegments(entry.description));

  const seen = new Set<string>();
  return detailLines.filter((line) => {
    const normalized = line.trim().toLowerCase();
    if (!normalized || seen.has(normalized)) return false;
    seen.add(normalized);
    return true;
  });
}

function sortActionLogByTimeline(
  entries: InternalHomeActionEntry[],
  limit: number
): InternalHomeActionEntry[] {
  if (limit <= 0 || entries.length === 0) return [];
  const withMeta = entries.map((entry, index) => {
    const parsedTimestamp = Date.parse(entry.timestamp || '');
    return {
      entry,
      index,
      timestampMs: Number.isFinite(parsedTimestamp)
        ? parsedTimestamp
        : Number.NEGATIVE_INFINITY,
    };
  });

  withMeta.sort((left, right) => {
    if (left.timestampMs === right.timestampMs) return left.index - right.index;
    return right.timestampMs - left.timestampMs;
  });

  return withMeta.slice(0, limit).map(({ entry }) => entry);
}

function actionKey(entry: InternalHomeActionEntry, index: number): string {
  if (entry.id !== null && entry.id !== undefined) return String(entry.id);
  return `action-${index}`;
}

function changelogKey(entry: InternalHomeChangelogEntry, index: number): string {
  if (entry.id !== null && entry.id !== undefined) return String(entry.id);
  return `changelog-${index}`;
}

type ActionFilter = 'all' | 'raid' | 'ban' | 'warning';

const ACTION_FILTERS: Array<{ id: ActionFilter; label: string }> = [
  { id: 'all', label: 'Alle' },
  { id: 'raid', label: 'Raids' },
  { id: 'ban', label: 'Bans' },
  { id: 'warning', label: 'Warnungen' },
];

const ACTION_PAGE_SIZE = 5;

function matchesActionFilter(entry: InternalHomeActionEntry, filter: ActionFilter): boolean {
  if (filter === 'all') return true;
  const eventType = String(entry.eventType || '').trim().toLowerCase();
  if (filter === 'raid') return eventType === 'raid' || eventType === 'raid_history';
  if (filter === 'ban') {
    return eventType === 'ban' || eventType === 'ban_keyword_hit' || eventType === 'unban';
  }
  return isServicePitchWarningAction(entry);
}

export function InternalHomeLanding() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const [selectedStreamer, setSelectedStreamer] = useState<string | null>(
    initialInternalHomeStreamer
  );
  const [actionFilter, setActionFilter] = useState<ActionFilter>('all');
  const [actionLimit, setActionLimit] = useState<number>(ACTION_PAGE_SIZE);
  const normalizedSelectedStreamer = selectedStreamer?.trim().toLowerCase() || null;

  useEffect(() => {
    setActionLimit(ACTION_PAGE_SIZE);
  }, [actionFilter, normalizedSelectedStreamer]);

  const partnerStreamers = useMemo(
    () =>
      streamers
        .map((channel) => ({ ...channel, login: channel.login?.trim().toLowerCase() || '' }))
        .filter((channel) => channel.isPartner && channel.login),
    [streamers]
  );
  const partnerLoginSet = useMemo(
    () => new Set(partnerStreamers.map((channel) => channel.login)),
    [partnerStreamers]
  );

  const isAdminView = Boolean(authStatus?.isAdmin || authStatus?.isLocalhost);
  const streamerOverride = isAdminView ? normalizedSelectedStreamer : null;
  const hasValidAdminSelection =
    streamerOverride !== null && partnerLoginSet.has(streamerOverride);
  const canRequestInternalHome = !loadingAuth && (!isAdminView || hasValidAdminSelection);

  const {
    data,
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery({
    queryKey: ['internal-home', streamerOverride],
    queryFn: () => fetchInternalHome(streamerOverride),
    staleTime: Number.POSITIVE_INFINITY,
    enabled: canRequestInternalHome,
  });

  const planName = authStatus?.plan?.planName || 'Free';

  useEffect(() => {
    if (loadingAuth || !isAdminView || loadingStreamers) return;
    if (normalizedSelectedStreamer && partnerLoginSet.has(normalizedSelectedStreamer)) return;
    const ownLogin = authStatus?.twitchLogin?.trim().toLowerCase() || '';
    const fallbackStreamer =
      ownLogin && partnerLoginSet.has(ownLogin)
        ? ownLogin
        : partnerStreamers[0]?.login || null;
    if (fallbackStreamer !== normalizedSelectedStreamer) setSelectedStreamer(fallbackStreamer);
  }, [
    authStatus?.twitchLogin,
    isAdminView,
    loadingAuth,
    loadingStreamers,
    normalizedSelectedStreamer,
    partnerLoginSet,
    partnerStreamers,
  ]);

  useEffect(() => {
    if (loadingAuth) return;
    const params = new URLSearchParams(window.location.search);
    const nextStreamer = isAdminView ? normalizedSelectedStreamer || '' : '';
    const currentStreamer = params.get('streamer')?.trim().toLowerCase() || '';
    if (nextStreamer) params.set('streamer', nextStreamer);
    else if (currentStreamer) params.delete('streamer');

    const nextSearch = params.toString();
    const nextUrl = `${window.location.pathname}${
      nextSearch ? `?${nextSearch}` : ''
    }${window.location.hash}`;
    const currentUrl = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    if (nextUrl !== currentUrl) window.history.replaceState({}, '', nextUrl);
  }, [isAdminView, loadingAuth, normalizedSelectedStreamer]);

  if (!canRequestInternalHome) {
    const emptyAdminState =
      !loadingAuth && isAdminView && !loadingStreamers && partnerStreamers.length === 0;

    return (
      <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
        <BackgroundBlobs />
        <div className="relative mx-auto max-w-[1440px]">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            {emptyAdminState ? (
              <div className="space-y-2">
                <h2 className="text-xl font-bold text-white">Kein Partner auswaehlbar</h2>
                <p className="text-sm text-text-secondary">
                  In der Admin-Ansicht werden nur aktive Partner-Profile angezeigt.
                </p>
              </div>
            ) : (
              <div className="flex items-center gap-3 text-text-secondary">
                <Loader2 className="h-5 w-5 animate-spin text-primary" />
                <span>Admin-Profil wird vorbereitet ...</span>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return <DashboardSkeleton />;
  }

  if (isError) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';

    return (
      <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
        <BackgroundBlobs />
        <div className="relative mx-auto max-w-[1440px]">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            <h2 className="text-xl font-bold text-white">Startseite nicht verfuegbar</h2>
            <p className="mt-1 text-sm text-text-secondary">{errorMessage}</p>
            <button
              onClick={() => void refetch()}
              className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"
            >
              <ArrowRight className="h-4 w-4" />
              Erneut laden
            </button>
          </div>
        </div>
      </div>
    );
  }

  const home = data ?? {};
  const twitchLogin = home.twitchLogin?.trim() || '';
  const displayName = home.displayName?.trim() || twitchLogin || 'Creator';
  const canAccessAnalyticsDashboard = Boolean(
    authStatus?.canAccessAnalyticsDashboard ?? authStatus?.access?.analytics ?? true
  );
  const restrictedPartnerStatus = String(authStatus?.partnerStatus || '').trim().toLowerCase();
  const hasRestrictedAnalyticsAccess = !canAccessAnalyticsDashboard;

  const healthScore = data?.healthScore ?? null;
  const lastStream = data?.lastStreamSummary ?? null;
  const weekComp = data?.weekComparison ?? null;
  const liveStatus = data?.liveStatus ?? null;

  const score = Math.max(0, Math.min(100, healthScore?.overall ?? 0));
  const subScores = healthScore?.sub_scores ?? {
    growth: 0,
    retention: 0,
    engagement: 0,
    community: 0,
  };

  const rawActionLog = home.actionLog ?? [];
  const channelScopeLogin = (normalizedSelectedStreamer || twitchLogin || '')
    .trim()
    .toLowerCase();
  const baseActionLog = rawActionLog
    .filter((entry) => String(entry.id || '').trim() !== 'impact-note')
    .filter((entry) => isVisibleChannelAction(entry, channelScopeLogin));
  const filteredActionLog = baseActionLog.filter((entry) =>
    matchesActionFilter(entry, actionFilter)
  );
  const actionLog = sortActionLogByTimeline(filteredActionLog, actionLimit);
  const totalFilteredActions = filteredActionLog.length;
  const hasMoreActions = totalFilteredActions > actionLog.length;

  const recentStreams = (home.recentStreams ?? []).slice(0, 5);
  const changelogEntries = (home.changelog?.entries ?? []).slice(0, 3);
  const mainNavItems: SidebarNavItem[] = [
    { href: PREVIEW_HOME_ROUTE, label: 'Home', icon: Home, active: true },
    ...(canAccessAnalyticsDashboard
      ? [{ href: analyticsTabHref('overview'), label: 'Analyse', icon: BarChart3 }]
      : []),
    { href: '/social-media-admin', label: 'Social Media Dashboard', icon: Film },
  ];
  const toolNavItems: SidebarNavItem[] = [
    { href: PREVIEW_VERWALTUNG_ROUTE, label: 'Verwaltung', icon: Settings },
    { href: PREVIEW_PRICING_ROUTE, label: `Plan: ${planName}`, icon: Sparkles },
    { href: PREVIEW_CHANGELOG_ROUTE, label: 'Changelog', icon: FileText },
  ];
  const scoreColorClass =
    score >= 70 ? 'text-success' : score >= 40 ? 'text-warning' : 'text-danger';
  const gaugeStrokeClass =
    score >= 70 ? 'text-success' : score >= 40 ? 'text-warning' : 'text-danger';
  const healthItems = [
    {
      label: 'Wachstum',
      value: subScores.growth,
      icon: TrendingUp,
      iconClass: 'border-primary/25 bg-primary/15 text-primary',
    },
    {
      label: 'Retention',
      value: subScores.retention,
      icon: Users,
      iconClass: 'border-accent/25 bg-accent/15 text-accent',
    },
    {
      label: 'Engagement',
      value: subScores.engagement,
      icon: MessageSquare,
      iconClass: 'border-warning/25 bg-warning/15 text-warning',
    },
    {
      label: 'Community',
      value: subScores.community,
      icon: Heart,
      iconClass: 'border-success/25 bg-success/15 text-success',
    },
  ] as const;

  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <WelcomeTour />
      <BackgroundBlobs />

      <div className="relative mx-auto max-w-[1440px]">
        <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
          <motion.aside
            className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.32 }}
          >
            <div className="space-y-4">
              <div className="flex items-center gap-3">
                <div className="gradient-accent sidebar-avatar-glow flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-bold text-white">
                  {displayName?.[0]?.toUpperCase() ?? '?'}
                </div>
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-white">{displayName}</div>
                  <div className="mt-1 inline-flex max-w-full items-center rounded-full border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-accent">
                    {planName}
                  </div>
                </div>
              </div>

              <div className="border-t border-border" />

              <div className="space-y-2">
                <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                  Main
                </div>
                <nav
                  data-tour-id="tour-nav"
                  className="flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible lg:pb-0"
                >
                  {mainNavItems.map((item, index) => (
                    <motion.div
                      key={item.href}
                      initial={{ opacity: 0, x: -6 }}
                      animate={{ opacity: 1, x: 0 }}
                      transition={{ duration: 0.22, delay: 0.05 + index * 0.04 }}
                    >
                      <SidebarLink
                        href={item.href}
                        icon={item.icon}
                        label={item.label}
                        active={item.active}
                      />
                    </motion.div>
                  ))}
                </nav>
              </div>

              <div className="space-y-2">
                <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                  Tools
                </div>
                <div className="flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible lg:pb-0">
                  {toolNavItems.map((item, index) => (
                    <motion.div
                      key={item.href}
                      initial={{ opacity: 0, x: -6 }}
                      animate={{ opacity: 1, x: 0 }}
                      transition={{ duration: 0.22, delay: 0.2 + index * 0.04 }}
                    >
                      <SidebarLink
                        href={item.href}
                        icon={item.icon}
                        label={item.label}
                      />
                    </motion.div>
                  ))}
                </div>
              </div>

              {isAdminView ? (
                <>
                  <div className="border-t border-border" />
                  <div className="space-y-2">
                    <label
                      className="block text-[10px] font-semibold uppercase tracking-[0.18em] text-text-secondary"
                      htmlFor="internal-home-streamer-switch"
                    >
                      Partner
                    </label>
                    <select
                      id="internal-home-streamer-switch"
                      value={normalizedSelectedStreamer || ''}
                      onChange={(event) => setSelectedStreamer(event.target.value || null)}
                      disabled={loadingStreamers || partnerStreamers.length === 0}
                      className="w-full rounded-xl border border-border bg-background/80 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      {partnerStreamers.length === 0 ? (
                        <option value="">Keine Partner</option>
                      ) : (
                        partnerStreamers.map((channel) => (
                          <option key={channel.login} value={channel.login}>
                            {channel.login}
                          </option>
                        ))
                      )}
                    </select>
                  </div>
                </>
              ) : null}
            </div>
          </motion.aside>

          <main className="space-y-4 md:space-y-5">
            <motion.section
              className="panel-card card-glow card-glow-accent hero-aura flex flex-col gap-4 rounded-2xl px-5 py-4 md:flex-row md:items-center md:justify-between"
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.32, delay: 0.04 }}
            >
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-text-secondary">
                    Willkommen zurueck
                  </div>
                  {liveStatus ? (
                    liveStatus.is_live ? (
                      <span className="glow-pill-live inline-flex items-center gap-1.5 rounded-full border border-danger/40 bg-danger/15 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-[0.18em] text-danger">
                        <span className="relative flex h-2 w-2">
                          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-danger opacity-75" />
                          <span className="relative inline-flex h-2 w-2 rounded-full bg-danger" />
                        </span>
                        Live · {formatNumber(liveStatus.viewer_count || 0)}
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-background/60 px-2.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                        <span className="h-2 w-2 rounded-full bg-text-secondary/60" />
                        Offline
                        {liveStatus.last_seen_at ? ` · ${formatRelativeShort(liveStatus.last_seen_at)}` : ''}
                      </span>
                    )
                  ) : null}
                </div>
                <h1 className="display-font mt-1 text-2xl font-bold text-white md:text-[2rem]">
                  {displayName}
                </h1>
                <p className="mt-1 text-sm text-text-secondary">
                  {liveStatus?.is_live && liveStatus.title
                    ? liveStatus.title
                    : 'Dein Kanal auf einen Blick'}
                </p>
              </div>

              <div className="flex flex-wrap items-center gap-3">
                <button
                  onClick={() => void refetch()}
                  disabled={isFetching}
                  className="inline-flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-not-allowed disabled:opacity-70"
                >
                  {isFetching ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <ArrowRight className="h-4 w-4" />
                  )}
                  Neu laden
                </button>
                {canAccessAnalyticsDashboard ? (
                  <a
                    href={analyticsTabHref('overview')}
                    className="gradient-accent inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-bold text-white no-underline shadow-lg shadow-primary/20 transition-transform hover:-translate-y-0.5"
                  >
                    Analyse Dashboard
                    <ArrowRight className="h-4 w-4" />
                  </a>
                ) : null}
              </div>
            </motion.section>

            {hasRestrictedAnalyticsAccess ? (
              <motion.section
                className="panel-card rounded-2xl border border-warning/30 bg-warning/10 px-5 py-4"
                initial={{ opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.24, delay: 0.06 }}
              >
                <div className="space-y-1">
                  <div className="text-sm font-semibold text-white">
                    Analyse-Zugriff aktuell eingeschraenkt
                  </div>
                  <p className="text-sm text-text-secondary">
                    {restrictedPartnerStatus === 'token_error'
                      ? 'Dein Twitch-OAuth hat aktuell einen Fehler. Home, Verwaltung und Pricing bleiben offen, bis du die Verbindung neu autorisierst.'
                      : 'Dieser Account hat aktuell keinen Zugriff auf das Analyse-Dashboard. Home, Verwaltung und Pricing bleiben weiterhin erreichbar.'}
                  </p>
                </div>
              </motion.section>
            ) : null}

            <motion.section
              className="grid gap-4 lg:grid-cols-3"
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.32, delay: 0.08 }}
            >
              {healthScore ? (
                <div
                  data-tour-id="tour-health"
                  className="panel-card card-glow card-glow-soft rounded-2xl p-5"
                >
                  <div className="mb-5 flex items-center gap-3">
                    <div className="gradient-accent sidebar-avatar-glow flex h-9 w-9 items-center justify-center rounded-xl">
                      <Heart className="h-4 w-4 text-white" />
                    </div>
                    <div>
                      <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                        Health Score
                      </div>
                      <h2 className="text-lg font-semibold text-white">Kanal-Gesundheit</h2>
                    </div>
                  </div>

                  <div className="flex flex-col items-center">
                    <div className="relative h-24 w-24">
                      <svg viewBox="0 0 100 100" className="h-full w-full -rotate-90">
                        <circle
                          cx="50"
                          cy="50"
                          r="42"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="8"
                          className="text-white/6"
                        />
                        <circle
                          cx="50"
                          cy="50"
                          r="42"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="8"
                          strokeLinecap="round"
                          strokeDasharray={`${(score / 100) * 264} 264`}
                          className={`${gaugeStrokeClass} score-ring-glow`}
                        />
                      </svg>
                      <div className="absolute inset-0 flex flex-col items-center justify-center">
                        <span className={`text-3xl font-bold ${scoreColorClass} kpi-value-glow`}>{score}</span>
                        <span className="text-xs text-white/45">/ 100</span>
                      </div>
                    </div>

                    {healthScore.trend != null ? (
                      <div
                        className={`mt-3 text-sm font-semibold ${
                          healthScore.trend >= 0 ? 'text-success' : 'text-danger'
                        }`}
                      >
                        {healthScore.trend >= 0 ? '\u2191' : '\u2193'}{' '}
                        {Math.abs(healthScore.trend)}% vs. Vorwoche
                      </div>
                    ) : null}
                  </div>

                  <div className="mt-5 space-y-3">
                    {healthItems.map((item) => (
                      <div key={item.label} className="flex items-center gap-3">
                        <div
                          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border ${item.iconClass}`}
                        >
                          <item.icon className="h-4 w-4" />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center justify-between gap-3">
                            <span className="text-sm font-medium text-white">{item.label}</span>
                            <span className="text-sm font-semibold text-text-secondary">
                              {item.value}
                            </span>
                          </div>
                          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/6">
                            <div
                              className={`h-full rounded-full ${
                                item.value >= 70
                                  ? 'bg-success'
                                  : item.value >= 40
                                    ? 'bg-warning'
                                    : 'bg-danger'
                              }`}
                              style={{ width: `${Math.max(0, Math.min(100, item.value))}%` }}
                            />
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}

              <div
                data-tour-id="tour-stream"
                className={`panel-card card-glow rounded-2xl p-5 ${healthScore ? 'lg:col-span-2' : 'lg:col-span-3'}`}
              >
                <div className="text-[11px] font-semibold uppercase tracking-[0.22em] text-primary">
                  Letzter Stream
                </div>
                <h2 className="mt-1 text-xl font-semibold text-white">
                  {lastStream?.started_at
                    ? `${formatDateWithTime(lastStream.started_at)} · ${formatDurationFromSeconds(
                        lastStream.duration_seconds
                      )}`
                    : 'Keine Stream-Daten verfuegbar'}
                </h2>
                <p className="mt-1 text-sm text-text-secondary">
                  {lastStream?.ended_at
                    ? `Ende: ${formatDateWithTime(lastStream.ended_at)}`
                    : 'Sobald ein Stream abgeschlossen ist, erscheint die Zusammenfassung hier.'}
                </p>

                {lastStream ? (
                  <div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <MiniStat
                      label={'\u00D8 Viewer'}
                      value={lastStream.avg_viewers}
                      icon={Users}
                      accent="primary"
                    />
                    <MiniStat
                      label="Peak"
                      value={lastStream.peak_viewers}
                      icon={TrendingUp}
                      accent="accent"
                    />
                    <MiniStat
                      label="Follower"
                      value={lastStream.follower_delta}
                      prefix="+"
                      icon={Heart}
                      accent="success"
                    />
                    <MiniStat
                      label="Chat"
                      value={lastStream.chat_messages}
                      icon={MessageSquare}
                      accent="warning"
                    />
                  </div>
                ) : null}
              </div>
            </motion.section>

            {weekComp ? (
              <motion.section
                data-tour-id="tour-week"
                className="grid grid-cols-2 gap-4 md:grid-cols-4"
                initial={{ opacity: 0, y: 16 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.32, delay: 0.12 }}
              >
                <WeekKpi
                  label={'\u00D8 Viewer'}
                  current={weekComp.current_week.avg_viewers}
                  change={weekComp.changes.avg_viewers_pct}
                  series={weekComp.daily_series?.avg_viewers}
                />
                <WeekKpi
                  label="Follower"
                  current={weekComp.current_week.total_followers}
                  change={weekComp.changes.followers_pct}
                  series={weekComp.daily_series?.followers}
                />
                <WeekKpi
                  label="Chat-Aktivitaet"
                  current={weekComp.current_week.chat_activity}
                  change={weekComp.changes.chat_activity_pct}
                  suffix="/h"
                  series={weekComp.daily_series?.chat_activity}
                />
                <WeekKpi
                  label="Stream-Stunden"
                  current={weekComp.current_week.stream_hours}
                  change={weekComp.changes.stream_hours_pct}
                  suffix="h"
                  series={weekComp.daily_series?.stream_hours}
                />
              </motion.section>
            ) : null}

            {recentStreams.length > 0 ? (
              <motion.section
                className="panel-card card-glow rounded-2xl p-5 md:p-6"
                initial={{ opacity: 0, y: 16 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.32, delay: 0.14 }}
              >
                <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
                  <div>
                    <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
                      Stream-Verlauf
                    </p>
                    <h2 className="display-font text-xl font-bold text-white">
                      Letzte Streams
                    </h2>
                  </div>
                  <a
                    href={analyticsTabHref('streams')}
                    className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-background/70 px-3 py-1.5 text-xs font-semibold text-text-secondary no-underline transition-colors hover:border-border-hover hover:text-white"
                  >
                    Alle Streams
                    <ArrowRight className="h-3.5 w-3.5" />
                  </a>
                </div>

                <ul className="space-y-2">
                  {recentStreams.map((stream, index) => {
                    const startedAt = stream.startedAt;
                    const durationSeconds =
                      stream.durationMinutes != null ? stream.durationMinutes * 60 : null;
                    const followerDelta = stream.followerDelta ?? 0;
                    const followerColor =
                      followerDelta > 0
                        ? 'text-success'
                        : followerDelta < 0
                          ? 'text-danger'
                          : 'text-text-secondary';
                    return (
                      <motion.li
                        key={stream.id ?? `stream-row-${index}`}
                        initial={{ opacity: 0, x: -8 }}
                        animate={{ opacity: 1, x: 0 }}
                        transition={{ duration: 0.24, delay: index * 0.04 }}
                      >
                        <a
                          href={analyticsTabHref('streams')}
                          className="internal-home-stream accent-bar block rounded-xl border border-border bg-background/55 pl-5 pr-3.5 py-3.5 no-underline"
                        >
                          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                            <div className="min-w-0 flex-1">
                              <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-secondary">
                                <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 font-semibold text-white">
                                  {formatCalendarDate(startedAt)}
                                </span>
                                {durationSeconds != null ? (
                                  <span className="rounded-full border border-border/70 bg-background/70 px-2.5 py-1 font-semibold text-text-secondary">
                                    {formatDuration(durationSeconds)}
                                  </span>
                                ) : null}
                              </div>
                              {stream.title ? (
                                <p className="mt-2 truncate text-sm font-semibold text-white">
                                  {stream.title}
                                </p>
                              ) : null}
                            </div>
                            <div className="grid grid-cols-3 gap-3 text-right md:gap-5">
                              <div>
                                <div className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
                                  {'Ø Viewer'}
                                </div>
                                <div className="text-sm font-bold text-white">
                                  {stream.avgViewers != null
                                    ? formatNumber(stream.avgViewers)
                                    : '–'}
                                </div>
                              </div>
                              <div>
                                <div className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
                                  Peak
                                </div>
                                <div className="text-sm font-bold text-white">
                                  {stream.peakViewers != null
                                    ? formatNumber(stream.peakViewers)
                                    : '–'}
                                </div>
                              </div>
                              <div>
                                <div className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
                                  Follower
                                </div>
                                <div className={`text-sm font-bold ${followerColor}`}>
                                  {followerDelta > 0 ? '+' : ''}
                                  {formatNumber(followerDelta)}
                                </div>
                              </div>
                            </div>
                          </div>
                        </a>
                      </motion.li>
                    );
                  })}
                </ul>
              </motion.section>
            ) : null}

            <motion.section
              className="grid gap-4 lg:grid-cols-2"
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.32, delay: 0.16 }}
            >
              <aside id="changelog" className="panel-card card-glow rounded-2xl p-5 md:p-6">
                <div className="mb-4">
                  <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
                    Updates
                  </p>
                  <h2 className="display-font text-xl font-bold text-white">
                    Was gibt&apos;s Neues
                  </h2>
                </div>

                {changelogEntries.length === 0 ? (
                  <div className="rounded-xl border border-border bg-background/60 p-4 text-sm text-text-secondary">
                    Keine neuen Updates verfuegbar.
                  </div>
                ) : (
                  <div className="space-y-2.5">
                    {changelogEntries.map((entry, index) => {
                      const title = entry.title?.trim() || 'Update';
                      const content = entry.content?.trim() || 'Kein Beschreibungstext';
                      const primaryDate = entry.entryDate || entry.createdAt;

                      return (
                        <article
                          key={changelogKey(entry, index)}
                          className="panel-card internal-home-changelog-entry rounded-xl p-3.5"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2 text-[11px]">
                            <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 font-semibold text-white">
                              {formatCalendarDate(primaryDate)}
                            </span>
                            {entry.createdAt ? (
                              <span className="text-text-secondary">
                                {formatDateTime(entry.createdAt)}
                              </span>
                            ) : null}
                          </div>
                          <p className="mt-2 text-sm font-semibold text-white">{title}</p>
                          <p className="mt-1 text-xs leading-5 text-text-secondary">{content}</p>
                        </article>
                      );
                    })}
                  </div>
                )}
              </aside>

              <article className="panel-card card-glow rounded-2xl p-5 md:p-6">
                <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
                  <div>
                    <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
                      Aktivitaet
                    </p>
                    <h2 className="display-font text-xl font-bold text-white">
                      Letzte Aktionen
                    </h2>
                  </div>
                  <span className="inline-flex items-center rounded-full border border-border bg-background/70 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-secondary">
                    {totalFilteredActions} Eintraege
                  </span>
                </div>

                <div className="mb-4 flex flex-wrap gap-1.5">
                  {ACTION_FILTERS.map((filter) => {
                    const isActive = actionFilter === filter.id;
                    return (
                      <button
                        key={filter.id}
                        type="button"
                        onClick={() => setActionFilter(filter.id)}
                        className={`rounded-full border px-3 py-1 text-[11px] font-semibold uppercase tracking-wider transition-colors ${
                          isActive
                            ? 'border-primary/40 bg-primary/15 text-primary'
                            : 'border-border bg-background/60 text-text-secondary hover:border-border-hover hover:text-white'
                        }`}
                      >
                        {filter.label}
                      </button>
                    );
                  })}
                </div>

                {actionLog.length === 0 ? (
                  <div className="rounded-xl border border-border bg-background/60 p-4 text-sm text-text-secondary">
                    Keine Aktionen vorhanden.
                  </div>
                ) : (
                  <ul className="space-y-2.5">
                    {actionLog.map((entry, index) => {
                      const entryIsBan = isBanAction(entry);
                      const isServicePitch = isServicePitchWarningAction(entry);
                      const isPriorityWarning = entryIsBan || isServicePitch;
                      const tone = actionLogTone(entry);
                      const rawTitle =
                        entry.title?.trim() || entry.eventType?.trim() || 'Bot Aktion';
                      const title = stripActionNoise(rawTitle) || 'Bot Aktion';
                      const rawSummary =
                        entry.summary?.trim() ||
                        entry.description?.trim() ||
                        entry.reason?.trim() ||
                        entry.metric?.trim() ||
                        '';
                      const summaryText = stripActionNoise(rawSummary);
                      const statusText = entryIsBan
                        ? 'BAN'
                        : isServicePitch
                          ? 'SERVICE-PITCH'
                          : entry.statusLabel?.trim() || tone.label;
                      const accountText = formatActionUser(entry);
                      const statusBadgeClass = isPriorityWarning
                        ? 'border-warning/35 bg-warning/10 text-warning'
                        : tone.badgeClass;
                      const cardClass = isPriorityWarning
                        ? 'internal-home-action-item accent-bar rounded-xl border border-warning/35 bg-warning/10 pl-5 pr-3.5 py-3.5'
                        : 'internal-home-action-item accent-bar rounded-xl border border-border bg-background/55 pl-5 pr-3.5 py-3.5';
                      const accentTone = entryIsBan
                        ? 'danger'
                        : isServicePitch
                          ? 'warning'
                          : entry.eventType === 'raid' || entry.eventType === 'raid_history'
                            ? 'success'
                            : 'primary';
                      const detailLines = isPriorityWarning
                        ? buildPriorityActionDetails(entry, isServicePitch)
                        : [];

                      return (
                        <motion.li
                          key={actionKey(entry, index)}
                          className={cardClass}
                          data-tone={accentTone}
                          initial={{ opacity: 0, y: 6 }}
                          animate={{ opacity: 1, y: 0 }}
                          transition={{ duration: 0.22, delay: index * 0.04 }}
                        >
                          <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-secondary">
                            <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 font-semibold text-white">
                              {formatDateTime(entry.timestamp)}
                            </span>
                            <span className="rounded-full border border-border/70 bg-background/70 px-2.5 py-1 font-semibold text-text-secondary">
                              {accountText}
                            </span>
                            <span
                              className={`rounded-full border px-2.5 py-1 font-semibold uppercase tracking-wider ${statusBadgeClass}`}
                            >
                              {statusText}
                            </span>
                          </div>

                          {isPriorityWarning ? (
                            <>
                              {detailLines.length === 0 ? (
                                <p className="mt-2 text-xs leading-5 text-text-primary">
                                  Keine Details gespeichert.
                                </p>
                              ) : (
                                <div className="mt-2 space-y-1 text-xs leading-5 text-text-primary">
                                  {detailLines.map((line, detailIndex) => (
                                    <p key={`detail-${detailIndex}`}>{line}</p>
                                  ))}
                                </div>
                              )}
                            </>
                          ) : (
                            <p className="mt-2 text-sm leading-5 text-text-secondary">
                              <span className="font-semibold text-white">{title}</span>
                              {summaryText ? ` \u00B7 ${summaryText}` : ''}
                            </p>
                          )}
                        </motion.li>
                      );
                    })}
                  </ul>
                )}

                {hasMoreActions ? (
                  <button
                    type="button"
                    onClick={() => setActionLimit((prev) => prev + ACTION_PAGE_SIZE)}
                    className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-border bg-background/60 px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:border-border-hover hover:text-white"
                  >
                    Mehr laden
                    <ArrowRight className="h-4 w-4" />
                  </button>
                ) : null}
              </article>
            </motion.section>
          </main>
        </div>
      </div>
    </div>
  );
}

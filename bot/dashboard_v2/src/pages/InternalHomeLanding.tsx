import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { useQuery } from '@tanstack/react-query';
import {
  fetchInternalHome,
  type InternalHomeActionEntry,
  type InternalHomeChangelogEntry,
} from '@/api/home';
import { fetchApi } from '@/api/core';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import { formatNumber, formatDuration } from '@/utils/formatters';
import {
  ArrowRight,
  BarChart3,
  FileText,
  Heart,
  Loader2,
  MessageSquare,
  Settings,
  Sparkles,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { WelcomeTour } from '@/components/onboarding/WelcomeTour';

// ---------------------------------------------------------------------------
// Types for the new backend fields (health_score, last_stream_summary, week_comparison)
// These come from the raw API response but are not yet mapped by fetchInternalHome.
// We fetch them via a lightweight raw query.
// ---------------------------------------------------------------------------

interface HealthScoreData {
  overall: number;
  trend: number | null;
  sub_scores: {
    growth: number;
    retention: number;
    engagement: number;
    community: number;
  };
}

interface LastStreamSummary {
  started_at: string | null;
  ended_at: string | null;
  duration_seconds: number | null;
  avg_viewers: number | null;
  peak_viewers: number | null;
  follower_delta: number | null;
  chat_messages: number | null;
}

interface WeekComparisonData {
  current_week: {
    avg_viewers: number | null;
    total_followers: number | null;
    chat_activity: number | null;
    stream_hours: number | null;
  };
  previous_week: {
    avg_viewers: number | null;
    total_followers: number | null;
    chat_activity: number | null;
    stream_hours: number | null;
  };
  changes: {
    avg_viewers_pct: number | null;
    followers_pct: number | null;
    chat_activity_pct: number | null;
    stream_hours_pct: number | null;
  };
}

interface RawInternalHomeExtras {
  health_score?: HealthScoreData | null;
  last_stream_summary?: LastStreamSummary | null;
  week_comparison?: WeekComparisonData | null;
}

// ---------------------------------------------------------------------------
// Small fetch helper for the additional internal-home fields.
// ---------------------------------------------------------------------------

async function fetchInternalHomeExtras(streamer?: string | null): Promise<RawInternalHomeExtras> {
  try {
    const raw = await fetchApi<RawInternalHomeExtras>('/internal-home', {
      ...(streamer ? { streamer } : {}),
    });
    return {
      health_score: raw.health_score ?? null,
      last_stream_summary: raw.last_stream_summary ?? null,
      week_comparison: raw.week_comparison ?? null,
    };
  } catch {
    return {};
  }
}

// ---------------------------------------------------------------------------
// Inline helper components
// ---------------------------------------------------------------------------

function MiniStat({ label, value, prefix = '', icon: Icon, accent = 'primary' }: {
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
  return (
    <div className="bg-background/50 rounded-xl border border-border p-3">
      {Icon && (
        <div className={`w-7 h-7 rounded-lg border flex items-center justify-center mb-2 ${accentColor}`}>
          <Icon className="w-3.5 h-3.5" />
        </div>
      )}
      <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</div>
      <div className="text-xl font-bold text-white mt-0.5">
        {value != null ? `${prefix}${formatNumber(value)}` : '\u2013'}
      </div>
    </div>
  );
}

const WEEK_KPI_META: Record<string, { icon: LucideIcon }> = {
  '\u00D8 Viewer': { icon: Users },
  'Follower': { icon: TrendingUp },
  'Chat-Aktivitaet': { icon: MessageSquare },
  'Stream-Stunden': { icon: BarChart3 },
};

function WeekKpi({ label, current, change, suffix = '' }: { label: string; current: number | null | undefined; change: number | null | undefined; suffix?: string }) {
  const meta = WEEK_KPI_META[label];
  const Icon = meta?.icon ?? BarChart3;
  return (
    <div className="panel-card soft-elevate rounded-xl p-4 internal-home-kpi">
      <div className="flex items-center gap-2.5 mb-3">
        <div className="w-8 h-8 rounded-lg gradient-accent flex items-center justify-center shrink-0">
          <Icon className="w-4 h-4 text-white" />
        </div>
        <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{label}</div>
      </div>
      <div className="text-2xl font-bold text-white">
        {current != null ? `${formatNumber(current)}${suffix}` : '\u2013'}
      </div>
      {change != null && (
        <div className={`text-xs mt-1.5 font-semibold ${change >= 0 ? 'text-success' : 'text-danger'}`}>
          {change >= 0 ? '\u2191' : '\u2193'} {Math.abs(change).toFixed(1)}% vs. Vorwoche
        </div>
      )}
    </div>
  );
}

function QuickAction({ href, icon: Icon, label, primary = false }: { href: string; icon: LucideIcon; label: string; primary?: boolean }) {
  return (
    <a
      href={href}
      className="panel-card soft-elevate inline-flex items-center gap-3 rounded-xl px-4 py-3 text-sm font-semibold text-white no-underline internal-home-quick-action"
    >
      <div className={`w-8 h-8 rounded-lg flex items-center justify-center shrink-0 ${primary ? 'gradient-accent' : 'bg-white/8 border border-white/10'}`}>
        <Icon className="w-4 h-4 text-white" />
      </div>
      {label}
    </a>
  );
}

// ---------------------------------------------------------------------------
// Existing helper functions (kept from original)
// ---------------------------------------------------------------------------

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
  return date.toLocaleString('de-DE', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' });
}

function formatCalendarDate(value: string | null | undefined): string {
  if (!value) return 'Unbekannt';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'Unbekannt';
  return date.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

function formatDateWithTime(iso: string | null | undefined): string {
  if (!iso) return '\u2013';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '\u2013';
  return date.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' });
}

function formatDurationFromSeconds(seconds: number | null | undefined): string {
  if (!seconds) return '\u2013';
  return formatDuration(seconds);
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
  const haystack = [entry.eventType, entry.statusLabel, entry.title, entry.summary, entry.reason, entry.description]
    .filter(Boolean).join(' ').toLowerCase();
  return haystack.includes('ban') || haystack.includes('banned') || haystack.includes('gebannt');
}

function isServicePitchWarningAction(entry: InternalHomeActionEntry): boolean {
  const haystack = [entry.eventType, entry.statusLabel, entry.title, entry.summary, entry.reason, entry.description]
    .filter(Boolean).join(' ').toLowerCase();
  return (
    haystack.includes('service_pitch_warning') || haystack.includes('service-pitch') ||
    haystack.includes('service pitch') || haystack.includes('pitch warn')
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
  return value.split('|').map((s) => stripActionNoise(s.trim())).filter(Boolean);
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

function buildPriorityActionDetails(entry: InternalHomeActionEntry, isServicePitchWarning: boolean): string[] {
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

function sortActionLogByTimeline(entries: InternalHomeActionEntry[], limit: number): InternalHomeActionEntry[] {
  if (limit <= 0 || entries.length === 0) return [];
  const withMeta = entries.map((entry, index) => {
    const parsedTimestamp = Date.parse(entry.timestamp || '');
    return { entry, index, timestampMs: Number.isFinite(parsedTimestamp) ? parsedTimestamp : Number.NEGATIVE_INFINITY };
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

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export function InternalHomeLanding() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const [selectedStreamer, setSelectedStreamer] = useState<string | null>(initialInternalHomeStreamer);
  const normalizedSelectedStreamer = selectedStreamer?.trim().toLowerCase() || null;
  const partnerStreamers = useMemo(
    () =>
      streamers
        .map((c) => ({ ...c, login: c.login?.trim().toLowerCase() || '' }))
        .filter((c) => c.isPartner && c.login),
    [streamers]
  );
  const partnerLoginSet = useMemo(() => new Set(partnerStreamers.map((c) => c.login)), [partnerStreamers]);
  const isAdminView = Boolean(authStatus?.isAdmin || authStatus?.isLocalhost);
  const streamerOverride = isAdminView ? normalizedSelectedStreamer : null;
  const hasValidAdminSelection = streamerOverride !== null && partnerLoginSet.has(streamerOverride);
  const canRequestInternalHome = !loadingAuth && (!isAdminView || hasValidAdminSelection);

  // Core internal-home query (mapped data)
  const { data, isLoading, isError, error, refetch, isFetching } = useQuery({
    queryKey: ['internal-home', streamerOverride],
    queryFn: () => fetchInternalHome(streamerOverride),
    staleTime: Number.POSITIVE_INFINITY,
    enabled: canRequestInternalHome,
  });

  // Extras query (raw fields: health_score, last_stream_summary, week_comparison)
  const { data: extras } = useQuery({
    queryKey: ['internal-home-extras', streamerOverride],
    queryFn: () => fetchInternalHomeExtras(streamerOverride),
    staleTime: Number.POSITIVE_INFINITY,
    enabled: canRequestInternalHome,
  });

  const planName = authStatus?.plan?.planName || 'Free';

  // Admin streamer fallback
  useEffect(() => {
    if (loadingAuth || !isAdminView || loadingStreamers) return;
    if (normalizedSelectedStreamer && partnerLoginSet.has(normalizedSelectedStreamer)) return;
    const ownLogin = authStatus?.twitchLogin?.trim().toLowerCase() || '';
    const fallbackStreamer = ownLogin && partnerLoginSet.has(ownLogin) ? ownLogin : partnerStreamers[0]?.login || null;
    if (fallbackStreamer !== normalizedSelectedStreamer) setSelectedStreamer(fallbackStreamer);
  }, [authStatus?.twitchLogin, isAdminView, loadingAuth, loadingStreamers, normalizedSelectedStreamer, partnerLoginSet, partnerStreamers]);

  // URL sync
  useEffect(() => {
    if (loadingAuth) return;
    const params = new URLSearchParams(window.location.search);
    const nextStreamer = isAdminView ? normalizedSelectedStreamer || '' : '';
    const currentStreamer = params.get('streamer')?.trim().toLowerCase() || '';
    if (nextStreamer) params.set('streamer', nextStreamer);
    else if (currentStreamer) params.delete('streamer');
    const nextSearch = params.toString();
    const nextUrl = `${window.location.pathname}${nextSearch ? `?${nextSearch}` : ''}${window.location.hash}`;
    const currentUrl = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    if (nextUrl !== currentUrl) window.history.replaceState({}, '', nextUrl);
  }, [isAdminView, loadingAuth, normalizedSelectedStreamer]);

  // ---- Early returns ----

  if (!canRequestInternalHome) {
    const emptyAdminState = !loadingAuth && isAdminView && !loadingStreamers && partnerStreamers.length === 0;
    return (
      <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
        <div className="relative max-w-[1280px] mx-auto">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            {emptyAdminState ? (
              <div className="space-y-2">
                <h2 className="text-xl font-bold text-white">Kein Partner auswaehlbar</h2>
                <p className="text-sm text-text-secondary">In der Admin-Ansicht werden nur aktive Partner-Profile angezeigt.</p>
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
    return (
      <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
        <div className="relative max-w-[1280px] mx-auto space-y-4 md:space-y-5">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            <div className="flex items-center gap-3 text-text-secondary">
              <Loader2 className="h-5 w-5 animate-spin text-primary" />
              <span>Startseite wird geladen ...</span>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (isError) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';
    return (
      <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
        <div className="relative max-w-[1280px] mx-auto">
          <div className="panel-card rounded-2xl p-6 md:p-8">
            <h2 className="text-xl font-bold text-white">Startseite nicht verfuegbar</h2>
            <p className="mt-1 text-sm text-text-secondary">{errorMessage}</p>
            <button onClick={() => void refetch()} className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover">
              <ArrowRight className="h-4 w-4" />
              Erneut laden
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Data extraction ----

  const home = data ?? {};
  const twitchLogin = home.twitchLogin?.trim() || '';
  const displayName = home.displayName?.trim() || twitchLogin || 'Creator';

  const healthScore = extras?.health_score ?? null;
  const lastStream = extras?.last_stream_summary ?? null;
  const weekComp = extras?.week_comparison ?? null;

  const score = healthScore?.overall ?? 0;
  const sub_scores = healthScore?.sub_scores ?? { growth: 0, retention: 0, engagement: 0, community: 0 };

  // Action log (last 5)
  const rawActionLog = home.actionLog ?? [];
  const channelScopeLogin = (normalizedSelectedStreamer || twitchLogin || '').trim().toLowerCase();
  const baseActionLog = rawActionLog
    .filter((entry) => String(entry.id || '').trim() !== 'impact-note')
    .filter((entry) => isVisibleChannelAction(entry, channelScopeLogin));
  const actionLog = sortActionLogByTimeline(baseActionLog, 5);

  // Changelog (last 3)
  const changelogEntries = (home.changelog?.entries ?? []).slice(0, 3);

  // ---- Render ----

  return (
    <div className="internal-home-vibe min-h-screen relative px-3 py-4 md:px-7 md:py-8">
      {/* Welcome Tour - shown once on first visit */}
      <WelcomeTour />

      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-32 right-[-8rem] h-[28rem] w-[28rem] rounded-full bg-primary/22 blur-3xl" />
        <div className="absolute top-[24%] -left-28 h-[22rem] w-[22rem] rounded-full bg-accent/24 blur-3xl" />
        <div className="absolute bottom-[-8rem] left-[34%] h-[24rem] w-[24rem] rounded-full bg-success/20 blur-3xl" />
      </div>

      <div className="relative max-w-[1280px] mx-auto space-y-4 md:space-y-5">

        {/* ===== 1. Welcome Header + Admin Selector ===== */}
        <motion.section
          className="panel-card rounded-2xl p-5 md:p-6"
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.32 }}
        >
          <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex items-center gap-4">
              <div className="w-14 h-14 rounded-full gradient-accent flex items-center justify-center text-2xl font-bold text-white shrink-0 shadow-lg shadow-primary/20">
                {displayName?.[0]?.toUpperCase() ?? '?'}
              </div>
              <div>
                <div className="inline-flex items-center gap-1.5 rounded-full border border-border bg-black/20 px-3 py-1 text-[11px] uppercase tracking-[0.16em] text-text-secondary mb-1.5">
                  <Sparkles className="w-3 h-3 text-accent" />
                  {planName}
                </div>
                <h1 className="text-2xl font-bold text-white">Willkommen, {displayName}!</h1>
                <p className="text-sm text-text-secondary">Dein Kanal auf einen Blick</p>
              </div>
            </div>

            <div className="flex items-center gap-3">
              {isAdminView && (
                <div className="rounded-xl border border-border bg-background/65 p-2 sm:min-w-[220px]">
                  <label className="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-text-secondary" htmlFor="internal-home-streamer-switch">
                    Partner
                  </label>
                  <select
                    id="internal-home-streamer-switch"
                    value={normalizedSelectedStreamer || ''}
                    onChange={(e) => setSelectedStreamer(e.target.value || null)}
                    disabled={loadingStreamers || partnerStreamers.length === 0}
                    className="w-full rounded-lg border border-border bg-background/80 px-2 py-1.5 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {partnerStreamers.length === 0 ? (
                      <option value="">Keine Partner</option>
                    ) : (
                      partnerStreamers.map((c) => (
                        <option key={c.login} value={c.login}>{c.login}</option>
                      ))
                    )}
                  </select>
                </div>
              )}
              <button
                onClick={() => void refetch()}
                disabled={isFetching}
                className="inline-flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-not-allowed disabled:opacity-70"
              >
                {isFetching ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowRight className="h-4 w-4" />}
                Neu laden
              </button>
            </div>
          </div>
        </motion.section>

        {/* ===== 2. Kanal-Gesundheitscheck ===== */}
        {healthScore && (
          <motion.section
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.32, delay: 0.04 }}
          >
            <div className="panel-card rounded-xl p-5">
              <div className="flex items-center gap-3 mb-4">
                <div className="w-8 h-8 rounded-lg gradient-accent flex items-center justify-center shrink-0">
                  <Heart className="w-4 h-4 text-white" />
                </div>
                <h2 className="text-lg font-semibold text-white">Kanal-Gesundheit</h2>
              </div>
              <div className="flex flex-col sm:flex-row items-center gap-8">
                {/* Circular gauge */}
                <div className="relative w-32 h-32 shrink-0">
                  <svg viewBox="0 0 100 100" className="w-full h-full -rotate-90">
                    <circle cx="50" cy="50" r="42" fill="none" stroke="currentColor" strokeWidth="8" className="text-white/5" />
                    <circle
                      cx="50" cy="50" r="42" fill="none" strokeWidth="8" strokeLinecap="round"
                      strokeDasharray={`${(score / 100) * 264} 264`}
                      className={score >= 70 ? 'text-emerald-400' : score >= 40 ? 'text-amber-400' : 'text-red-400'}
                    />
                  </svg>
                  <div className="absolute inset-0 flex flex-col items-center justify-center">
                    <span className={`text-3xl font-bold ${score >= 70 ? 'text-emerald-400' : score >= 40 ? 'text-amber-400' : 'text-red-400'}`}>
                      {score}
                    </span>
                    <span className="text-xs text-white/40">/ 100</span>
                  </div>
                </div>
                {/* Sub-scores */}
                <div className="flex-1 grid grid-cols-2 gap-3 w-full">
                  {([
                    { label: 'Wachstum', value: sub_scores.growth, icon: TrendingUp, accent: 'text-primary bg-primary/15 border-primary/25' },
                    { label: 'Retention', value: sub_scores.retention, icon: Users, accent: 'text-accent bg-accent/15 border-accent/25' },
                    { label: 'Engagement', value: sub_scores.engagement, icon: MessageSquare, accent: 'text-warning bg-warning/15 border-warning/25' },
                    { label: 'Community', value: sub_scores.community, icon: Heart, accent: 'text-success bg-success/15 border-success/25' },
                  ] as const).map((item) => (
                    <div key={item.label} className="flex items-center gap-2.5">
                      <div className={`w-7 h-7 rounded-lg border flex items-center justify-center shrink-0 ${item.accent}`}>
                        <item.icon className="w-3.5 h-3.5" />
                      </div>
                      <div className="min-w-[60px]">
                        <div className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{item.label}</div>
                        <div className="text-sm font-bold text-white">{item.value}</div>
                      </div>
                      <div className="flex-1 h-1.5 bg-white/5 rounded-full overflow-hidden">
                        <div
                          className={`h-full rounded-full ${item.value >= 70 ? 'bg-success' : item.value >= 40 ? 'bg-warning' : 'bg-danger'}`}
                          style={{ width: `${item.value}%` }}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
              {/* Trend line */}
              {healthScore.trend != null && (
                <div className={`mt-3 text-sm ${healthScore.trend >= 0 ? 'text-emerald-400' : 'text-red-400'}`}>
                  {healthScore.trend >= 0 ? '\u2191' : '\u2193'} {Math.abs(healthScore.trend)}% vs. Vorwoche
                </div>
              )}
            </div>
          </motion.section>
        )}

        {/* ===== 3. Letzter Stream Zusammenfassung ===== */}
        <motion.section
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.32, delay: 0.08 }}
        >
          <div className="panel-card rounded-xl p-5">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-8 h-8 rounded-lg gradient-accent flex items-center justify-center shrink-0">
                <BarChart3 className="w-4 h-4 text-white" />
              </div>
              <div>
                <h2 className="text-lg font-semibold text-white">Letzter Stream</h2>
                {lastStream?.started_at && (
                  <p className="text-xs text-text-secondary mt-0.5">
                    {formatDateWithTime(lastStream.started_at)} &middot; {formatDurationFromSeconds(lastStream.duration_seconds)}
                  </p>
                )}
              </div>
            </div>
            {lastStream ? (
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <MiniStat label={'\u00D8 Viewer'} value={lastStream.avg_viewers} icon={Users} accent="primary" />
                <MiniStat label="Peak" value={lastStream.peak_viewers} icon={TrendingUp} accent="accent" />
                <MiniStat label="Follower" value={lastStream.follower_delta} prefix="+" icon={Heart} accent="success" />
                <MiniStat label="Chat" value={lastStream.chat_messages} icon={MessageSquare} accent="warning" />
              </div>
            ) : (
              <p className="text-sm text-text-secondary">Kein Stream-Daten verfuegbar</p>
            )}
          </div>
        </motion.section>

        {/* ===== 4. Wochenvergleich (4 KPI cards) ===== */}
        {weekComp && (
          <motion.section
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.32, delay: 0.12 }}
          >
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
              <WeekKpi label={'\u00D8 Viewer'} current={weekComp.current_week.avg_viewers} change={weekComp.changes.avg_viewers_pct} />
              <WeekKpi label="Follower" current={weekComp.current_week.total_followers} change={weekComp.changes.followers_pct} />
              <WeekKpi label="Chat-Aktivitaet" current={weekComp.current_week.chat_activity} change={weekComp.changes.chat_activity_pct} suffix="/h" />
              <WeekKpi label="Stream-Stunden" current={weekComp.current_week.stream_hours} change={weekComp.changes.stream_hours_pct} suffix="h" />
            </div>
          </motion.section>
        )}

        {/* ===== 5. Quick-Actions ===== */}
        <motion.section
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.32, delay: 0.16 }}
        >
          <div className="flex flex-wrap gap-3">
            <QuickAction href="/twitch/dashboard-v2" icon={BarChart3} label="Analyse Dashboard" primary />
            <QuickAction href="/twitch/verwaltung" icon={Settings} label="Verwaltung" />
            <QuickAction href="/twitch/pricing" icon={Sparkles} label={`Plan: ${planName}`} />
            <QuickAction href="#changelog" icon={FileText} label="Changelog" />
          </div>
        </motion.section>

        {/* ===== 6. Changelog + Action Log ===== */}
        <motion.section
          className="grid gap-4 lg:grid-cols-2"
          initial={{ opacity: 0, y: 16 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.32, delay: 0.2 }}
        >
          {/* Changelog */}
          <aside id="changelog" className="panel-card rounded-2xl p-5 md:p-6">
            <div className="mb-4">
              <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Updates</p>
              <h2 className="display-font text-xl font-bold text-white">Was gibt&apos;s Neues</h2>
            </div>
            {changelogEntries.length === 0 ? (
              <div className="rounded-xl border border-border bg-background/60 p-4 text-sm text-text-secondary">Keine neuen Updates verfuegbar.</div>
            ) : (
              <div className="space-y-2.5">
                {changelogEntries.map((entry, index) => {
                  const title = entry.title?.trim() || 'Update';
                  const content = entry.content?.trim() || 'Kein Beschreibungstext';
                  const primaryDate = entry.entryDate || entry.createdAt;
                  return (
                    <article key={changelogKey(entry, index)} className="internal-home-changelog-entry panel-card rounded-xl p-3.5">
                      <div className="flex flex-wrap items-center justify-between gap-2 text-[11px]">
                        <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 font-semibold text-white">{formatCalendarDate(primaryDate)}</span>
                        {entry.createdAt ? <span className="text-text-secondary">{formatDateTime(entry.createdAt)}</span> : null}
                      </div>
                      <p className="mt-2 text-sm font-semibold text-white">{title}</p>
                      <p className="mt-1 text-xs leading-5 text-text-secondary">{content}</p>
                    </article>
                  );
                })}
              </div>
            )}
          </aside>

          {/* Action Log (last 5) */}
          <article className="panel-card rounded-2xl p-5 md:p-6">
            <div className="mb-4 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
              <div>
                <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Aktivitaet</p>
                <h2 className="display-font text-xl font-bold text-white">Letzte Aktionen</h2>
              </div>
              <span className="inline-flex items-center rounded-full border border-border bg-background/70 px-3 py-1 text-[11px] font-semibold tracking-wider text-text-secondary uppercase">{actionLog.length} Eintraege</span>
            </div>
            {actionLog.length === 0 ? (
              <div className="rounded-xl border border-border bg-background/60 p-4 text-sm text-text-secondary">Keine Aktionen vorhanden.</div>
            ) : (
              <ul className="space-y-2.5">
                {actionLog.map((entry, index) => {
                  const entryIsBan = isBanAction(entry);
                  const isServicePitch = isServicePitchWarningAction(entry);
                  const isPriorityWarning = entryIsBan || isServicePitch;
                  const tone = actionLogTone(entry);
                  const rawTitle = entry.title?.trim() || entry.eventType?.trim() || 'Bot Aktion';
                  const title = stripActionNoise(rawTitle) || 'Bot Aktion';
                  const rawSummary = entry.summary?.trim() || entry.description?.trim() || entry.reason?.trim() || entry.metric?.trim() || '';
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
                    ? 'internal-home-action-item rounded-xl border border-warning/35 bg-warning/10 p-3.5'
                    : 'internal-home-action-item rounded-xl border border-border bg-background/55 p-3.5';
                  const detailLines = isPriorityWarning
                    ? buildPriorityActionDetails(entry, isServicePitch)
                    : [];

                  return (
                    <li key={actionKey(entry, index)} className={cardClass}>
                      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-secondary">
                        <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 font-semibold text-white">{formatDateTime(entry.timestamp)}</span>
                        <span className="rounded-full border border-border/70 bg-background/70 px-2.5 py-1 font-semibold text-text-secondary">{accountText}</span>
                        <span className={`rounded-full border px-2.5 py-1 font-semibold uppercase tracking-wider ${statusBadgeClass}`}>{statusText}</span>
                      </div>
                      {isPriorityWarning ? (
                        <>
                          {detailLines.length === 0 ? (
                            <p className="mt-2 text-xs leading-5 text-text-primary">Keine Details gespeichert.</p>
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
                    </li>
                  );
                })}
              </ul>
            )}
          </article>
        </motion.section>

      </div>
    </div>
  );
}

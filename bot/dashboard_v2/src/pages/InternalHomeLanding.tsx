import { useEffect, useMemo, useState } from 'react';
import { Rise } from '../motion/Rise';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  fetchInternalHome,
  type InternalHomeChangelogEntry,
} from '@/api/home';
import { setAdminMode } from '@/api/auth';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import {
  PREVIEW_PRICING_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';
import { formatNumber, formatDuration } from '@/utils/formatters';
import {
  ArrowRight,
  BarChart3,
  Heart,
  Loader2,
  MessageSquare,
  ShieldCheck,
  TrendingUp,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { WelcomeTour } from '@/components/onboarding/WelcomeTour';
import { StreamRecapCard } from '@/components/cards/StreamRecapCard';

function MiniStat({
  label,
  value,
  prefix = '',
  suffix = '',
  icon: Icon,
  accent = 'primary',
}: {
  label: string;
  value: number | null | undefined;
  prefix?: string;
  suffix?: string;
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
    <div className="group relative overflow-hidden rounded-xl border border-border bg-background/55 p-3 transition-[transform,translate,scale,border-color,background-color,box-shadow] duration-200 hover:-translate-y-0.5 hover:border-border-hover hover:bg-background/75">
      <div
        className="pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-300 group-hover:opacity-100"
        style={{
          background: `radial-gradient(120% 80% at 50% 0%, rgba(${glowRgb}, 0.2), transparent 60%)`,
        }}
      />
      {Icon ? (
        <div className={`icon-duotone mb-2 flex h-7 w-7 items-center justify-center rounded-lg border ${accentColor}`}>
          <Icon className="h-3.5 w-3.5" />
        </div>
      ) : null}
      <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-text-secondary">{label}</div>
      <div
        className="kpi-number mt-1 text-xl font-bold text-white"
        style={{ textShadow: `0 0 18px rgba(${glowRgb}, 0.55)` }}
      >
        {value != null ? `${prefix}${formatNumber(value)}${suffix}` : '\u2013'}
      </div>
    </div>
  );
}

function SkeletonBlock({ className = '' }: { className?: string }) {
  return <div className={`animate-pulse rounded-lg bg-white/6 ${className}`} />;
}

function DashboardSkeleton() {
  return (
    <>
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
    </>
  );
}

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

function changelogKey(entry: InternalHomeChangelogEntry, index: number): string {
  if (entry.id !== null && entry.id !== undefined) return String(entry.id);
  return `changelog-${index}`;
}

export function InternalHomeLanding() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const [selectedStreamer, setSelectedStreamer] = useState<string | null>(
    initialInternalHomeStreamer
  );
  const normalizedSelectedStreamer = selectedStreamer?.trim().toLowerCase() || null;

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

  const queryClient = useQueryClient();
  const adminMode = Boolean(authStatus?.adminMode);
  const adminModeMutation = useMutation({
    mutationFn: async (enabled: boolean) => {
      await queryClient.cancelQueries({ queryKey: ['internal-home'] });
      const result = await setAdminMode(
        enabled,
        authStatus?.csrfToken ?? authStatus?.csrf_token ?? null
      );
      await queryClient.refetchQueries(
        { queryKey: ['auth-status'], exact: true, type: 'active' },
        { throwOnError: true }
      );
      return result;
    },
  });

  useEffect(() => {
    if (loadingAuth || !isAdminView || loadingStreamers) return;
    if (normalizedSelectedStreamer && partnerLoginSet.has(normalizedSelectedStreamer)) return;
    const ownLogin = authStatus?.twitchLogin?.trim().toLowerCase() || '';
    const adminDefault = authStatus?.adminDefaultStreamer?.trim().toLowerCase() || '';
    const fallbackStreamer =
      ownLogin && partnerLoginSet.has(ownLogin)
        ? ownLogin
        : adminDefault && partnerLoginSet.has(adminDefault)
          ? adminDefault
          : null;
    if (fallbackStreamer !== normalizedSelectedStreamer) setSelectedStreamer(fallbackStreamer);
  }, [
    authStatus?.twitchLogin,
    authStatus?.adminDefaultStreamer,
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
    const noPartnersAtAll =
      !loadingAuth && isAdminView && !loadingStreamers && partnerStreamers.length === 0;
    const needsAdminPick =
      !loadingAuth && isAdminView && !loadingStreamers && partnerStreamers.length > 0;

    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        {noPartnersAtAll ? (
          <div className="space-y-2">
            <h2 className="text-xl font-bold text-white">Kein Partner auswaehlbar</h2>
            <p className="text-sm text-text-secondary">
              In der Admin-Ansicht werden nur aktive Partner-Profile angezeigt.
            </p>
          </div>
        ) : needsAdminPick ? (
          <div className="space-y-3">
            <h2 className="text-xl font-bold text-white">Partner auswaehlen</h2>
            <p className="text-sm text-text-secondary">
              Waehle einen Partner, dessen Dashboard du ansehen moechtest.
            </p>
            <select
              value={normalizedSelectedStreamer || ''}
              onChange={(event) => setSelectedStreamer(event.target.value || null)}
              className="w-full max-w-sm rounded-xl border border-border bg-background/80 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover"
            >
              <option value="">— Partner waehlen —</option>
              {partnerStreamers.map((channel) => (
                <option key={channel.login} value={channel.login}>
                  {channel.login}
                </option>
              ))}
            </select>
          </div>
        ) : (
          <div className="flex items-center gap-3 text-text-secondary">
            <Loader2 className="h-5 w-5 animate-spin text-primary" />
            <span>Admin-Profil wird vorbereitet ...</span>
          </div>
        )}
      </div>
    );
  }

  if (isLoading) {
    return <DashboardSkeleton />;
  }

  if (isError) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';

    return (
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
  const personalBests = data?.personalBests ?? null;
  const streamComparison = data?.streamComparison ?? null;
  const viewersOverTime = data?.viewersOverTime ?? null;
  const liveStatus = data?.liveStatus ?? null;

  const score = Math.max(0, Math.min(100, healthScore?.overall ?? 0));
  const subScores = healthScore?.sub_scores ?? {
    growth: 0,
    retention: 0,
    engagement: 0,
    community: 0,
  };

  const changelogEntries = (home.changelog?.entries ?? []).slice(0, 3);
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
    <>
      <WelcomeTour
        completionLabel="Zur Abo-Seite"
        onComplete={() => {
          localStorage.removeItem('pricing-tour-dismissed');
          localStorage.setItem('pricing-tour-pending', '1');
          window.location.href = PREVIEW_PRICING_ROUTE;
        }}
      />

      <div className="grid gap-4 md:gap-5 xl:grid-cols-[minmax(0,1fr)_340px] 2xl:grid-cols-[minmax(0,1fr)_420px]">
        <div className="min-w-0 space-y-4 md:space-y-5">
          {isAdminView ? (
            <Rise as="section" className="panel-card card-glow rounded-2xl p-4">
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
                className="mt-2 w-full max-w-sm rounded-xl border border-border bg-background/80 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover disabled:cursor-not-allowed disabled:opacity-60"
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
            </Rise>
          ) : null}
            {adminMode ? (
              <Rise
                className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-warning/40 bg-warning/10 px-4 py-3"
              >
                <div className="flex items-center gap-2 text-sm font-medium text-warning">
                  <ShieldCheck className="h-4 w-4 shrink-0" />
                  Admin-Modus aktiv, du siehst alle Inhalte entsperrt, nicht die echte Nutzer-Ansicht.
                </div>
                <button
                  type="button"
                  onClick={() => adminModeMutation.mutate(false)}
                  disabled={adminModeMutation.isPending}
                  className="rounded-lg border border-warning/40 bg-warning/15 px-3 py-1 text-xs font-semibold text-warning transition-colors hover:bg-warning/25 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  Beenden
                </button>
              </Rise>
            ) : null}
            <Rise
              step={{ seconds: 0.04 }}
              as="section"
              data-tour-id="tour-intro"
              className="panel-card card-glow card-glow-accent hero-aura flex flex-col gap-4 rounded-2xl px-5 py-4 md:flex-row md:items-center md:justify-between"
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
                    className="gradient-accent inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-bold no-underline shadow-lg shadow-primary/20 transition-[transform,translate,scale] hover:-translate-y-0.5"
                  >
                    Analyse Dashboard
                    <ArrowRight className="h-4 w-4" />
                  </a>
                ) : null}
              </div>
            </Rise>

            {hasRestrictedAnalyticsAccess ? (
              <Rise
                step={{ seconds: 0.06 }}
                as="section"
                className="panel-card rounded-2xl border border-warning/30 bg-warning/10 px-5 py-4"
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
              </Rise>
            ) : null}

            <Rise
              step={{ seconds: 0.08 }}
              as="section"
              className="grid gap-4 lg:grid-cols-3"
            >
              {healthScore ? (
                <div
                  data-tour-id="tour-health"
                  className="panel-card card-glow card-glow-soft rounded-2xl p-5"
                >
                  <div className="mb-5 flex items-center gap-3">
                    <div className="gradient-accent sidebar-avatar-glow flex h-9 w-9 items-center justify-center rounded-xl">
                      <Heart className="h-4 w-4 text-on-gold" />
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
                          className={gaugeStrokeClass}
                        />
                      </svg>
                      <div className="absolute inset-0 flex flex-col items-center justify-center">
                        <span className={`kpi-number text-3xl font-bold ${scoreColorClass}`}>{score}</span>
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
                          <div className="mt-2 h-2 overflow-hidden rounded-full bg-white/6">
                            <div
                              className="h-full rounded-full transition-[width] duration-500"
                              style={{
                                width: `${Math.max(0, Math.min(100, item.value))}%`,
                                background: `linear-gradient(90deg, var(--color-primary) 0%, ${
                                  item.value >= 70
                                    ? 'var(--color-success)'
                                    : item.value >= 40
                                      ? 'var(--color-warning)'
                                      : 'var(--color-danger)'
                                } 100%)`,
                              }}
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

                {weekComp ? (
                  <div data-tour-id="tour-week" className="mt-5 border-t border-border pt-5">
                    <div className="mb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                      Woche vs. Vorwoche
                    </div>
                    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                      <MiniStat
                        label={'\u00D8 Viewer'}
                        value={weekComp.current_week.avg_viewers}
                        icon={Users}
                        accent="primary"
                      />
                      <MiniStat
                        label="Follower"
                        value={weekComp.current_week.total_followers}
                        icon={TrendingUp}
                        accent="success"
                      />
                      <MiniStat
                        label="Chat-Aktivitaet"
                        value={weekComp.current_week.chat_activity}
                        suffix="/h"
                        icon={MessageSquare}
                        accent="warning"
                      />
                      <MiniStat
                        label="Stream-Stunden"
                        value={weekComp.current_week.stream_hours}
                        suffix="h"
                        icon={BarChart3}
                        accent="accent"
                      />
                    </div>
                  </div>
                ) : null}
              </div>
            </Rise>

            <StreamRecapCard
              personalBests={personalBests}
              streamComparison={streamComparison}
              viewersOverTime={viewersOverTime}
              lastStream={lastStream}
              delay={0.1}
            />
        </div>

        <Rise
          step={{ seconds: 0.16 }}
          as="aside"
          id="changelog"
          className="panel-card card-glow self-start rounded-2xl p-5 md:p-6"
        >
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
        </Rise>
      </div>
    </>
  );
}

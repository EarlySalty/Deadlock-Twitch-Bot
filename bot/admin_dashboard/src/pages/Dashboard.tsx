import { Activity, AlertTriangle, ArrowUpRight, CreditCard, Database, Radio, RefreshCw, Server, ShieldCheck, Users, ZapOff } from 'lucide-react';
import { Link } from 'react-router';
import type { StreamerRow } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { StatusBadge } from '@/components/shared/StatusBadge';
import {
  useDashboardOverview,
  useDatabaseStats,
  useEventSubStatus,
  useScopeStatus,
  useStreamers,
  useSystemHealth,
} from '@/hooks/useAdmin';
import { coerceArray, coerceRecord, formatBytes, formatDuration, formatNumber, formatRelativeTime } from '@/utils/formatters';

type CardStatus = 'ok' | 'warning' | 'error';

interface StatusStripCardProps {
  title: string;
  value: string;
  hint: string;
  status: CardStatus;
  icon: typeof Activity;
}

interface LiveNowRow {
  login: string;
  displayName: string;
  game: string;
  viewerCount?: number;
  sessionDurationSeconds?: number;
}

interface PendingActionCard {
  title: string;
  count: number;
  to: string;
  icon: typeof Activity;
}

interface ActivityEntry {
  key: string;
  timestamp?: string;
  actor?: string;
  description: string;
  streamerLogin?: string;
}

function readString(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    if (typeof record[key] === 'string') {
      const value = record[key]?.trim();
      if (value) {
        return value;
      }
    }
  }
  return '';
}

function readNumber(record: Record<string, unknown>, ...keys: string[]): number | undefined {
  for (const key of keys) {
    const rawValue = record[key];
    if (rawValue === undefined || rawValue === null || rawValue === '') {
      continue;
    }
    const value = Number(rawValue);
    if (Number.isFinite(value)) {
      return value;
    }
  }
  return undefined;
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]): boolean | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'boolean') {
      return value;
    }
    if (typeof value === 'number') {
      return value !== 0;
    }
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (!normalized) {
        continue;
      }
      if (['1', 'true', 'yes', 'on', 'live', 'active'].includes(normalized)) {
        return true;
      }
      if (['0', 'false', 'no', 'off', 'inactive'].includes(normalized)) {
        return false;
      }
    }
  }
  return undefined;
}

function trimText(value: string, maxLength = 92): string {
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, maxLength - 1).trimEnd()}…`;
}

function parseDate(value: unknown): Date | null {
  if (!value) {
    return null;
  }
  const date = new Date(String(value));
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return date;
}

function durationFromTimestamp(timestamp: string | undefined): number | undefined {
  const parsed = parseDate(timestamp);
  if (!parsed) {
    return undefined;
  }
  return Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
}

function normalizeActivityEntry(entry: Record<string, unknown>, index: number): ActivityEntry | null {
  const title = readString(entry, 'title', 'message', 'label', 'event', 'code');
  const detail = readString(entry, 'description', 'detail', 'context', 'note', 'reason');
  const description = [title, detail].filter(Boolean).join(title && detail ? ' · ' : '');
  if (!description) {
    return null;
  }
  const streamerLogin =
    readString(entry, 'streamerLogin', 'streamer_login', 'login', 'targetLogin', 'target_login') || undefined;
  const timestamp =
    readString(entry, 'timestamp', 'createdAt', 'created_at', 'time', 'executedAt', 'executed_at') || undefined;
  const actor =
    readString(entry, 'actor', 'admin', 'author', 'user', 'source', 'moderator', 'displayName', 'display_name') ||
    undefined;

  return {
    key: `${index}-${title || detail || streamerLogin || 'activity'}`,
    timestamp,
    actor,
    description,
    streamerLogin,
  };
}

function extractRecentActivityEntries(overview: ReturnType<typeof useDashboardOverview>['data']): ActivityEntry[] {
  const raw = coerceRecord(overview?.raw);
  const candidates = [
    overview?.recentActivity,
    coerceArray<Record<string, unknown>>(raw.activity),
    coerceArray<Record<string, unknown>>(coerceRecord(raw.bot_activity).events),
    coerceArray<Record<string, unknown>>(coerceRecord(raw.bot_impact).events),
  ];

  for (const candidate of candidates) {
    if (!candidate) {
      continue;
    }
    const items = candidate
      .map((entry, index) => normalizeActivityEntry(coerceRecord(entry), index))
      .filter((entry): entry is ActivityEntry => Boolean(entry))
      .slice(0, 20);
    if (items.length) {
      return items;
    }
  }

  return [];
}

function extractOverviewLiveRows(overview: ReturnType<typeof useDashboardOverview>['data']): LiveNowRow[] {
  const raw = coerceRecord(overview?.raw);
  const candidateLists = [
    raw.liveNow,
    raw.live_now,
    raw.liveStreamers,
    raw.live_streamers,
  ];

  for (const candidate of candidateLists) {
    const rows: LiveNowRow[] = [];
    for (const entry of coerceArray<Record<string, unknown>>(candidate)) {
      const record = coerceRecord(entry);
      const login = readString(record, 'login', 'streamerLogin', 'streamer_login');
      if (!login) {
        continue;
      }
      const isLive = readBoolean(record, 'isLive', 'is_live', 'live', 'status') ?? false;
      if (!isLive) {
        continue;
      }
      rows.push({
        login,
        displayName: readString(record, 'displayName', 'display_name', 'name') || login,
        game: readString(record, 'game', 'gameName', 'game_name', 'category') || '—',
        viewerCount: readNumber(record, 'viewerCount', 'viewer_count', 'viewers'),
        sessionDurationSeconds: durationFromTimestamp(
          readString(record, 'startedAt', 'started_at', 'sessionStartedAt', 'session_started_at') || undefined,
        ),
      });
    }

    if (rows.length) {
      return rows;
    }
  }

  return [];
}

function extractStreamerSessionDuration(row: StreamerRow): number | undefined {
  const raw = coerceRecord(row.raw);
  const startedAt =
    readString(raw, 'lastStartedAt', 'last_started_at', 'startedAt', 'started_at') || undefined;
  return durationFromTimestamp(startedAt);
}

function extractPendingSubscriptionsCount(overview: ReturnType<typeof useDashboardOverview>['data']): number | undefined {
  const raw = coerceRecord(overview?.raw);
  const directCount =
    readNumber(raw, 'pendingSubscriptionsCount', 'pending_subscriptions_count', 'expiredPlansCount', 'expired_plans_count') ??
    readNumber(coerceRecord(raw.pendingSubscriptions), 'count', 'total') ??
    readNumber(coerceRecord(raw.pending_subscriptions), 'count', 'total');
  if (directCount !== undefined) {
    return directCount;
  }

  const directList = coerceArray<unknown>(raw.pendingSubscriptions ?? raw.pending_subscriptions);
  if (directList.length) {
    return directList.length;
  }

  for (const action of overview?.actions ?? []) {
    const record = coerceRecord(action);
    const identifier = `${readString(record, 'id', 'key', 'title', 'label')} ${readString(record, 'type')}`.toLowerCase();
    if (!identifier.includes('subscription') && !identifier.includes('plan')) {
      continue;
    }
    const count = readNumber(record, 'count', 'total', 'value');
    if (count !== undefined) {
      return count;
    }
  }

  return undefined;
}

function extractEventSubErrorCount(eventSub: ReturnType<typeof useEventSubStatus>['data']): number | undefined {
  const raw = coerceRecord(eventSub?.raw);
  return (
    readNumber(raw, 'errorCount', 'error_count', 'failedCount', 'failed_count', 'deadLetterCount', 'dead_letter_count') ??
    readNumber(coerceRecord(raw.errors), 'count', 'total') ??
    readNumber(coerceRecord(raw.dead_letter), 'count', 'total')
  );
}

function StatusStripCard({ title, value, hint, status, icon: Icon }: StatusStripCardProps) {
  return (
    <article className="panel-card soft-elevate rounded-[1.6rem] p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-text-secondary">{title}</p>
          <div className="mt-4 inline-flex scale-110">
            <StatusBadge status={status} />
          </div>
        </div>
        <div className="rounded-2xl border border-white/10 bg-white/5 p-3 text-white/90">
          <Icon className="h-5 w-5" />
        </div>
      </div>
      <p className="mt-5 text-2xl font-semibold text-white">{value}</p>
      <p className="mt-2 text-sm leading-6 text-text-secondary">{hint}</p>
    </article>
  );
}

export function Dashboard() {
  const overviewQuery = useDashboardOverview();
  const eventSubQuery = useEventSubStatus();
  const scopeQuery = useScopeStatus();
  const healthQuery = useSystemHealth();
  const databaseQuery = useDatabaseStats();
  const streamersQuery = useStreamers('active');

  const isRefreshing = [
    overviewQuery,
    eventSubQuery,
    scopeQuery,
    healthQuery,
    databaseQuery,
    streamersQuery,
  ].some((query) => query.isFetching);

  const overviewLiveRows = extractOverviewLiveRows(overviewQuery.data);
  const fallbackLiveRows = (streamersQuery.data ?? [])
    .filter((row) => row.isLive || String(row.status || '').toLowerCase() === 'live')
    .map((row) => ({
      login: row.login,
      displayName: row.displayName || row.login,
      game: row.lastGame || '—',
      viewerCount: row.viewerCount,
      sessionDurationSeconds: extractStreamerSessionDuration(row),
    }))
    .sort((left, right) => (right.viewerCount ?? 0) - (left.viewerCount ?? 0));
  const liveNowRows = (overviewLiveRows.length ? overviewLiveRows : fallbackLiveRows).sort(
    (left, right) => (right.viewerCount ?? 0) - (left.viewerCount ?? 0),
  );
  const recentActivity = extractRecentActivityEntries(overviewQuery.data);

  const eventSubActiveCount = eventSubQuery.data?.activeSubscriptionCount;
  const eventSubCapacityMax = eventSubQuery.data?.capacity?.max;
  const eventSubLastSnapshotAt = eventSubQuery.data?.capacity?.lastSnapshotAt;
  const eventSubTransport = eventSubQuery.data?.websocketStatus || 'unknown';
  const eventSubValue =
    typeof eventSubActiveCount === 'number'
      ? `${formatNumber(eventSubActiveCount)} aktiv / ${typeof eventSubCapacityMax === 'number' ? formatNumber(eventSubCapacityMax) : '—'} total`
      : '—';
  const eventSubStatus: CardStatus =
    typeof eventSubActiveCount !== 'number'
      ? 'warning'
      : eventSubTransport === 'inactive' || eventSubTransport === 'disconnected' || eventSubActiveCount === 0
        ? 'error'
        : eventSubTransport === 'connected'
          ? 'ok'
          : 'warning';
  const eventSubHint = eventSubQuery.isError
    ? 'Status konnte nicht geladen werden.'
    : `Transport ${eventSubTransport}${eventSubLastSnapshotAt ? ` · Snapshot ${formatRelativeTime(eventSubLastSnapshotAt)}` : ''}`;

  const missingScopeCount = scopeQuery.data?.summary.missingScopeCount;
  const scopeValue = typeof missingScopeCount === 'number' ? `${formatNumber(missingScopeCount)} Streamer brauchen Reauth` : '—';
  const scopeStatus: CardStatus =
    typeof missingScopeCount !== 'number'
      ? 'warning'
      : missingScopeCount === 0
        ? 'ok'
        : missingScopeCount <= 3
          ? 'warning'
          : 'error';
  const scopeHint =
    typeof scopeQuery.data?.summary.totalAuthorized === 'number'
      ? `${formatNumber(scopeQuery.data.summary.totalAuthorized)} OAuth-Profile insgesamt`
      : 'Scope-Status aktuell nicht vollständig verfügbar.';

  const healthWarnings = healthQuery.data?.serviceWarnings ?? [];
  const latestHealthWarning = coerceRecord(healthWarnings[0]);
  const uptimeSeconds = healthQuery.data?.uptimeSeconds;
  const healthValue = typeof uptimeSeconds === 'number' ? formatDuration(uptimeSeconds) : '—';
  const healthStatus: CardStatus =
    typeof uptimeSeconds !== 'number'
      ? 'warning'
      : healthWarnings.length > 0
        ? 'warning'
        : 'ok';
  const healthHint = healthQuery.isError
    ? 'Health-Status konnte nicht geladen werden.'
    : healthWarnings.length
      ? `Last warning: ${trimText(readString(latestHealthWarning, 'message') || 'Unbekannt')}`
      : `Letzter Tick ${formatRelativeTime(healthQuery.data?.lastTickAt)}`;

  const databaseConnectionCount =
    readNumber(coerceRecord(databaseQuery.data?.raw), 'connectionCount', 'connection_count', 'connections', 'latencyMs', 'latency_ms') ??
    readNumber(coerceRecord(databaseQuery.data?.raw), 'latency', 'latency_ms');
  const databaseValue = databaseConnectionCount !== undefined ? String(databaseConnectionCount) : '—';
  const databaseStatus: CardStatus = databaseConnectionCount !== undefined ? 'ok' : 'warning';
  const databaseHint = databaseQuery.isError
    ? 'Database-Snapshot konnte nicht geladen werden.'
    : databaseQuery.data?.databaseSizeBytes
      ? `DB-Größe ${formatBytes(databaseQuery.data.databaseSizeBytes)} · ${formatNumber(databaseQuery.data.tables?.length ?? 0)} Tabellen`
      : 'Keine Connection- oder Latenz-Felder im Payload vorhanden.';

  const pendingActions: PendingActionCard[] = [];
  const unverifiedCount = (streamersQuery.data ?? []).filter((row) => row.partnerStatus === 'active' && !row.verified).length;
  if (unverifiedCount > 0) {
    pendingActions.push({
      title: 'Unverifizierte Streamer',
      count: unverifiedCount,
      to: '/community/streamers?filter=pending',
      icon: Users,
    });
  }
  if ((missingScopeCount ?? 0) > 0) {
    pendingActions.push({
      title: 'Scope-Reauth offen',
      count: missingScopeCount ?? 0,
      to: '/operations/scopes',
      icon: ShieldCheck,
    });
  }
  const pendingSubscriptionsCount = extractPendingSubscriptionsCount(overviewQuery.data);
  if ((pendingSubscriptionsCount ?? 0) > 0) {
    pendingActions.push({
      title: 'Abgelaufene Plans',
      count: pendingSubscriptionsCount ?? 0,
      to: '/money/subscriptions',
      icon: CreditCard,
    });
  }
  const eventSubErrorCount = extractEventSubErrorCount(eventSubQuery.data);
  if ((eventSubErrorCount ?? 0) > 0) {
    pendingActions.push({
      title: 'EventSub-Fehler',
      count: eventSubErrorCount ?? 0,
      to: '/operations/eventsub',
      icon: AlertTriangle,
    });
  }

  const liveColumns: TableColumn<LiveNowRow>[] = [
    {
      key: 'login',
      title: 'Login',
      sortable: true,
      sortValue: (row) => row.login,
      render: (row) => (
        <Link to={`/community/streamers/${encodeURIComponent(row.login)}`} className="font-semibold text-white hover:text-primary">
          {row.login}
        </Link>
      ),
    },
    {
      key: 'displayName',
      title: 'Display Name',
      sortable: true,
      sortValue: (row) => row.displayName,
      render: (row) => <span className="text-white">{row.displayName || '—'}</span>,
    },
    {
      key: 'game',
      title: 'Spiel',
      sortable: true,
      sortValue: (row) => row.game,
      render: (row) => <span className="text-text-secondary">{row.game || '—'}</span>,
    },
    {
      key: 'viewerCount',
      title: 'Viewer',
      sortable: true,
      sortValue: (row) => row.viewerCount ?? 0,
      render: (row) => <span className="font-medium text-white">{formatNumber(row.viewerCount ?? 0)}</span>,
    },
    {
      key: 'duration',
      title: 'Session-Dauer',
      sortable: true,
      sortValue: (row) => row.sessionDurationSeconds ?? 0,
      render: (row) => <span className="text-text-secondary">{row.sessionDurationSeconds ? formatDuration(row.sessionDurationSeconds) : '—'}</span>,
    },
    {
      key: 'actions',
      title: 'Aktionen',
      className: 'min-w-[140px]',
      render: (row) => (
        <Link to={`/community/streamers/${encodeURIComponent(row.login)}`} className="admin-button admin-button-secondary !px-3 !py-2">
          Detail
          <ArrowUpRight className="h-4 w-4" />
        </Link>
      ),
    },
  ];

  return (
    <div className="space-y-5">
      <PageHeader
        title="Cockpit"
        description="Live-Status der Twitch-Bot-Plattform auf einen Blick."
        primaryAction={
          <button
            type="button"
            className="admin-button admin-button-secondary"
            onClick={() => {
              void Promise.all([
                overviewQuery.refetch(),
                eventSubQuery.refetch(),
                scopeQuery.refetch(),
                healthQuery.refetch(),
                databaseQuery.refetch(),
                streamersQuery.refetch(),
              ]);
            }}
          >
            <RefreshCw className={`h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
      />

      <section className="grid grid-cols-2 gap-4 md:grid-cols-4">
        <StatusStripCard title="EventSub" value={eventSubValue} hint={eventSubHint} status={eventSubStatus} icon={Radio} />
        <StatusStripCard title="OAuth Reauth" value={scopeValue} hint={scopeHint} status={scopeStatus} icon={ShieldCheck} />
        <StatusStripCard title="Bot Health" value={healthValue} hint={healthHint} status={healthStatus} icon={Server} />
        <StatusStripCard title="Database" value={databaseValue} hint={databaseHint} status={databaseStatus} icon={Database} />
      </section>

      <section className="grid gap-5 lg:grid-cols-[1.5fr_1fr]">
        <Section title="Live Now" hint="Streamer aktuell live">
          <DataTable
            columns={liveColumns}
            rows={liveNowRows}
            rowKey={(row) => row.login}
            emptyState={
              <EmptyState
                icon={Radio}
                title={streamersQuery.isLoading && !streamersQuery.data ? 'Live-Daten werden geladen' : 'Aktuell ist kein Streamer live'}
                description={
                  streamersQuery.isLoading && !streamersQuery.data
                    ? 'Der Cockpit-Snapshot lädt gerade die aktuellen Live-Streamer.'
                    : 'Sobald ein verwalteter Stream live ist, erscheint er hier mit Viewer-Zahl und Quick-Links.'
                }
              />
            }
          />
        </Section>

        <Section title="Pending Actions" hint="Was Aufmerksamkeit braucht">
          {pendingActions.length ? (
            <div className="space-y-3">
              {pendingActions.map((item) => {
                const Icon = item.icon;
                return (
                  <Link
                    key={`${item.title}-${item.to}`}
                    to={item.to}
                    className="soft-elevate flex items-center justify-between gap-4 rounded-[1.35rem] border border-white/10 bg-white/[0.04] p-4 transition hover:bg-white/[0.06]"
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <div className="rounded-2xl border border-white/10 bg-white/5 p-3 text-white/90">
                        <Icon className="h-4 w-4" />
                      </div>
                      <div className="min-w-0">
                        <p className="font-semibold text-white">{item.title}</p>
                        <p className="text-sm text-text-secondary">{formatNumber(item.count)} offen</p>
                      </div>
                    </div>
                    <ArrowUpRight className="h-4 w-4 shrink-0 text-text-secondary" />
                  </Link>
                );
              })}
            </div>
          ) : (
            <EmptyState
              icon={ShieldCheck}
              title="Keine offenen Punkte"
              description="Aktuell gibt es keine Pending Actions, die unmittelbare Aufmerksamkeit benötigen."
            />
          )}
        </Section>
      </section>

      <Section title="Recent Activity" hint="Letzte Admin-Signale">
        {recentActivity.length ? (
          <div className="space-y-3">
            {recentActivity.map((entry) => (
              <article key={entry.key} className="soft-elevate rounded-[1.35rem] border border-white/10 bg-white/[0.04] p-4">
                <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
                  <div className="space-y-2">
                    <div className="flex flex-wrap items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">
                      <span>{entry.timestamp ? formatRelativeTime(entry.timestamp) : 'Ohne Zeitstempel'}</span>
                      {entry.actor ? <span className="stat-pill !px-3 !py-1 !text-[0.65rem]">{entry.actor}</span> : null}
                      {entry.streamerLogin ? (
                        <Link to={`/community/streamers/${encodeURIComponent(entry.streamerLogin)}`} className="stat-pill !px-3 !py-1 !text-[0.65rem]">
                          {entry.streamerLogin}
                        </Link>
                      ) : null}
                    </div>
                    <p className="text-sm leading-6 text-white">{entry.description}</p>
                  </div>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <EmptyState
            icon={ZapOff}
            title="Keine kürzlichen Signale"
            description="Im aktuellen Cockpit-Payload wurden keine frischen Admin- oder Systemaktivitäten gefunden."
          />
        )}
      </Section>
    </div>
  );
}

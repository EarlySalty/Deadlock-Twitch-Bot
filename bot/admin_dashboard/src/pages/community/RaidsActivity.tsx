import { Clock3, RefreshCw, SearchX } from 'lucide-react';
import { useMemo } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { useConfigOverview } from '@/hooks/useAdmin';
import { coerceArray, coerceRecord, formatNumber, formatRelativeTime } from '@/utils/formatters';

type RaidEntry = {
  id: string;
  streamer: string;
  target: string;
  startedAt?: string;
  viewers?: number;
  reason?: string;
  status: string;
};

function readString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]) {
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
      return !['0', 'false', 'off', 'no'].includes(normalized);
    }
  }
  return undefined;
}

function readNumber(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (value === undefined || value === null || value === '') {
      continue;
    }
    const numeric = Number(value);
    if (Number.isFinite(numeric)) {
      return numeric;
    }
  }
  return undefined;
}

function renderValue(value: string) {
  return <span className="text-white">{value}</span>;
}

function readRaidStatus(record: Record<string, unknown>) {
  const rawStatus = readString(record, 'status', 'state', 'result').toLowerCase();
  if (['success', 'succeeded', 'completed'].includes(rawStatus)) {
    return 'ok';
  }
  if (['failed', 'error', 'rejected'].includes(rawStatus)) {
    return 'error';
  }

  const success = readBoolean(record, 'success', 'ok');
  if (success !== undefined) {
    return success ? 'ok' : 'error';
  }
  return readBoolean(record, 'active', 'running', 'isActive', 'is_active') ? 'active' : 'unknown';
}

function extractRaidEntries(raw: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const list = coerceArray<Record<string, unknown>>(raw[key]);
    if (!list.length) {
      continue;
    }
    return list.map((entry, index) => {
      const record = coerceRecord(entry);
      const streamer =
        readString(record, 'streamer', 'streamerLogin', 'streamer_login', 'channel', 'channelLogin', 'channel_login') || '—';
      const target =
        readString(record, 'target', 'targetLogin', 'target_login', 'toBroadcaster', 'to_broadcaster', 'raidTarget') || '—';
      const startedAt =
        readString(record, 'startedAt', 'started_at', 'executedAt', 'executed_at', 'createdAt', 'created_at') || undefined;

      return {
        id: `${key}-${index}-${streamer}-${target}`,
        streamer,
        target,
        startedAt,
        viewers: readNumber(record, 'viewers', 'viewerCount', 'viewer_count'),
        reason: readString(record, 'reason', 'errorMessage', 'error_message') || undefined,
        status: readRaidStatus(record),
      };
    });
  }

  return [];
}

export default function RaidsActivityPage() {
  const configQuery = useConfigOverview();

  const raidSnapshot = configQuery.data?.raids;
  const raidRaw = coerceRecord(raidSnapshot?.raw);

  const configFields = useMemo(() => {
    const totalManagedStreamers =
      raidSnapshot?.totalManagedStreamers ?? readNumber(raidRaw, 'totalManagedStreamers', 'total_managed_streamers');
    const raidBotEnabledCount =
      raidSnapshot?.raidBotEnabledCount ?? readNumber(raidRaw, 'raidBotEnabledCount', 'raid_bot_enabled_count');
    const livePingEnabledCount =
      raidSnapshot?.livePingEnabledCount ?? readNumber(raidRaw, 'livePingEnabledCount', 'live_ping_enabled_count');
    const allRaidBotEnabled =
      raidSnapshot?.allRaidBotEnabled ?? readBoolean(raidRaw, 'allRaidBotEnabled', 'all_raid_bot_enabled');
    const allLivePingEnabled =
      raidSnapshot?.allLivePingEnabled ?? readBoolean(raidRaw, 'allLivePingEnabled', 'all_live_ping_enabled');

    return [
      {
        label: 'Raid-Bot bei allen aktiv',
        value: allRaidBotEnabled === undefined ? 'Nicht verfügbar' : allRaidBotEnabled ? 'Ja' : 'Nein',
      },
      {
        label: 'Live-Ping bei allen aktiv',
        value: allLivePingEnabled === undefined ? 'Nicht verfügbar' : allLivePingEnabled ? 'Ja' : 'Nein',
      },
      {
        label: 'Verwaltete Streamer',
        value: totalManagedStreamers === undefined ? 'Nicht verfügbar' : formatNumber(totalManagedStreamers),
      },
      {
        label: 'Raid-Bot aktiv',
        value: raidBotEnabledCount === undefined ? 'Nicht verfügbar' : formatNumber(raidBotEnabledCount),
      },
      {
        label: 'Live-Ping aktiv',
        value: livePingEnabledCount === undefined ? 'Nicht verfügbar' : formatNumber(livePingEnabledCount),
      },
    ];
  }, [raidRaw, raidSnapshot]);

  const activeEntries = useMemo(
    () => extractRaidEntries(raidRaw, ['activeSessions', 'active_sessions', 'runningSessions', 'running_sessions']),
    [raidRaw],
  );
  const historyEntries = useMemo(
    () =>
      raidSnapshot?.history?.length
        ? extractRaidEntries({ history: raidSnapshot.history }, ['history'])
        : extractRaidEntries(raidRaw, ['history', 'raidHistory', 'raid_history', 'recentHistory', 'recent_history']),
    [raidRaw, raidSnapshot?.history],
  );

  const columns: TableColumn<RaidEntry>[] = [
    {
      key: 'streamer',
      title: 'Streamer',
      sortable: true,
      sortValue: (row) => row.streamer,
      render: (row) => row.streamer,
    },
    {
      key: 'target',
      title: 'Ziel',
      sortable: true,
      sortValue: (row) => row.target,
      render: (row) => row.target,
    },
    {
      key: 'startedAt',
      title: 'Zeitpunkt',
      sortable: true,
      sortValue: (row) => (row.startedAt ? new Date(row.startedAt).getTime() : 0),
      render: (row) => (row.startedAt ? formatRelativeTime(row.startedAt) : '—'),
    },
    {
      key: 'viewers',
      title: 'Zuschauer',
      sortable: true,
      sortValue: (row) => row.viewers ?? 0,
      render: (row) => (row.viewers === undefined ? '—' : formatNumber(row.viewers)),
    },
    {
      key: 'reason',
      title: 'Grund',
      sortable: true,
      sortValue: (row) => row.reason ?? '',
      render: (row) => row.reason || '—',
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => row.status,
      render: (row) => <StatusBadge status={row.status} />,
    },
  ];

  if (configQuery.isLoading && !configQuery.data) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Raid-Konfiguration wird geladen …</div>;
  }

  if (configQuery.isError) {
    return (
      <section className="space-y-6">
        <PageHeader
          title="Raids"
          description="Konfiguration und Aktivität der Raid-Mechanik."
          primaryAction={
            <button className="admin-button admin-button-secondary" onClick={() => void configQuery.refetch()}>
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
          }
        />
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {configQuery.error instanceof Error ? configQuery.error.message : 'Raid-Konfiguration konnte nicht geladen werden.'}
        </div>
      </section>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Raids"
        description="Konfiguration und Aktivität der Raid-Mechanik."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => void configQuery.refetch()}
            disabled={configQuery.isFetching}
          >
            <RefreshCw className={`h-4 w-4 ${configQuery.isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
      />

      <Section title="Raid-Konfiguration" hint="Aktueller Partnerbestand">
        <div className="space-y-5">
          <div className="grid gap-4 lg:grid-cols-2">
            {configFields.map((field) => (
              <article key={field.label} className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">{field.label}</p>
                <div className="mt-3">{renderValue(field.value)}</div>
              </article>
            ))}
          </div>
          <div className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4 text-sm text-text-secondary">
            Die Zähler stammen aus dem aktuellen Partnerbestand. Die Historie zeigt die letzten 50 abgeschlossenen Raid-Ereignisse.
          </div>
        </div>
      </Section>

      <Section title="Aktive Raids" hint="Aktuell laufende Raid-Sessions">
        {activeEntries.length ? (
          <DataTable columns={columns} rows={activeEntries} rowKey={(row) => row.id} />
        ) : (
          <EmptyState
            icon={Clock3}
            title="Keine aktiven Raid-Sessions"
            description="Für laufende Raids gibt es derzeit keinen eigenen Session-Feed. Abgeschlossene Ereignisse stehen in der Historie."
          />
        )}
      </Section>

      <Section title="Raid-Historie" hint="Letzte abgeschlossene Raids">
        {historyEntries.length ? (
          <DataTable columns={columns} rows={historyEntries} rowKey={(row) => row.id} />
        ) : (
          <EmptyState
            icon={SearchX}
            title="Keine Raid-Historie"
            description="In der Datenbank sind keine abgeschlossenen Raid-Ereignisse vorhanden."
          />
        )}
      </Section>
    </section>
  );
}

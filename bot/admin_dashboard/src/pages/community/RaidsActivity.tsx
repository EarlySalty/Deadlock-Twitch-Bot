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

function readStringArray(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    const entries = coerceArray<unknown>(value)
      .map((entry) => String(entry || '').trim())
      .filter(Boolean);
    if (entries.length) {
      return entries;
    }
  }
  return [];
}

function renderValueWithBadge(value: string, hasValue: boolean) {
  if (!hasValue) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-white">—</span>
        <StatusBadge status="warning" />
      </div>
    );
  }
  return <span className="text-white">{value}</span>;
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
      const status =
        readString(record, 'status', 'state', 'result') ||
        (readBoolean(record, 'active', 'running', 'isActive', 'is_active') ? 'active' : 'unknown');

      return {
        id: `${key}-${index}-${streamer}-${target}`,
        streamer,
        target,
        startedAt,
        status,
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
    const enabled = readBoolean(raidRaw, 'enabled', 'is_enabled', 'raidBotEnabled', 'raid_bot_enabled', 'allRaidBotEnabled');
    const autoRaidEnabled = readBoolean(
      raidRaw,
      'autoRaidEnabled',
      'auto_raid_enabled',
      'livePingEnabled',
      'live_ping_enabled',
      'allLivePingEnabled',
    );
    const defaultDelay = readNumber(
      raidRaw,
      'defaultDelay',
      'default_delay',
      'defaultDelaySeconds',
      'default_delay_seconds',
    );
    const requiredViewerCount = readNumber(
      raidRaw,
      'requiredViewerCount',
      'required_viewer_count',
      'minimumViewerCount',
      'minimum_viewer_count',
    );
    const channelAllowlist = readStringArray(raidRaw, 'channelAllowlist', 'channel_allowlist', 'allowlist');
    const totalManagedStreamers =
      raidSnapshot?.totalManagedStreamers ?? readNumber(raidRaw, 'totalManagedStreamers', 'total_managed_streamers');
    const raidBotEnabledCount =
      raidSnapshot?.raidBotEnabledCount ?? readNumber(raidRaw, 'raidBotEnabledCount', 'raid_bot_enabled_count');
    const livePingEnabledCount =
      raidSnapshot?.livePingEnabledCount ?? readNumber(raidRaw, 'livePingEnabledCount', 'live_ping_enabled_count');

    return [
      { label: 'Enabled', value: enabled === undefined ? '—' : enabled ? 'Ja' : 'Nein', hasValue: enabled !== undefined },
      {
        label: 'Auto Raid Enabled',
        value: autoRaidEnabled === undefined ? '—' : autoRaidEnabled ? 'Ja' : 'Nein',
        hasValue: autoRaidEnabled !== undefined,
      },
      {
        label: 'Default Delay',
        value: defaultDelay === undefined ? '—' : `${formatNumber(defaultDelay)} s`,
        hasValue: defaultDelay !== undefined,
      },
      {
        label: 'Required Viewer Count',
        value: requiredViewerCount === undefined ? '—' : formatNumber(requiredViewerCount),
        hasValue: requiredViewerCount !== undefined,
      },
      {
        label: 'Channel Allowlist',
        value: channelAllowlist.length ? channelAllowlist.join(', ') : '—',
        hasValue: channelAllowlist.length > 0,
      },
      {
        label: 'Managed Streamers',
        value: totalManagedStreamers === undefined ? '—' : formatNumber(totalManagedStreamers),
        hasValue: totalManagedStreamers !== undefined,
      },
      {
        label: 'Raid Bot Enabled Count',
        value: raidBotEnabledCount === undefined ? '—' : formatNumber(raidBotEnabledCount),
        hasValue: raidBotEnabledCount !== undefined,
      },
      {
        label: 'Live Ping Enabled Count',
        value: livePingEnabledCount === undefined ? '—' : formatNumber(livePingEnabledCount),
        hasValue: livePingEnabledCount !== undefined,
      },
    ];
  }, [raidRaw, raidSnapshot?.livePingEnabledCount, raidSnapshot?.raidBotEnabledCount, raidSnapshot?.totalManagedStreamers]);

  const activeEntries = useMemo(
    () => extractRaidEntries(raidRaw, ['activeSessions', 'active_sessions', 'runningSessions', 'running_sessions']),
    [raidRaw],
  );
  const historyEntries = useMemo(
    () => extractRaidEntries(raidRaw, ['history', 'raidHistory', 'raid_history', 'recentHistory', 'recent_history']),
    [raidRaw],
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
      title: 'Gestartet vor',
      sortable: true,
      sortValue: (row) => (row.startedAt ? new Date(row.startedAt).getTime() : 0),
      render: (row) => (row.startedAt ? formatRelativeTime(row.startedAt) : '—'),
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

      <Section title="Raid-Konfiguration" hint="Globale Defaults">
        <div className="space-y-5">
          <div className="grid gap-4 lg:grid-cols-2">
            {configFields.map((field) => (
              <article key={field.label} className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">{field.label}</p>
                <div className="mt-3">{renderValueWithBadge(field.value, field.hasValue)}</div>
              </article>
            ))}
          </div>
          <div className="rounded-[1.4rem] border border-warning/20 bg-warning/[0.04] p-4 text-sm text-text-secondary">
            Konfiguration wird im finalen Visual-Pass auf editierbar gestellt.
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
            description="Der aktuelle Payload enthält keine laufenden Raids oder noch keinen dedizierten Live-Feed."
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
            description="Im ConfigOverview-Payload wurden keine abgeschlossenen Raid-Einträge gefunden."
          />
        )}
      </Section>
    </section>
  );
}

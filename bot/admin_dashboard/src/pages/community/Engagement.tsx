import { RefreshCw, SearchX, Sparkles, UserCheck, UserMinus } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { fetchEngagementSettings } from '@/api/client';
import type { EngagementSettings, StreamerRow } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import { useConfigOverview, useEngagementToggle, useStreamers } from '@/hooks/useAdmin';
import { coerceRecord, formatNumber, formatRelativeTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

type EngagementSnapshotValue = EngagementSettings | null | 'loading' | 'error';

function readString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
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

function matchesStreamerSearch(row: StreamerRow, search: string) {
  const normalized = search.trim().toLowerCase();
  if (!normalized) {
    return true;
  }
  return [row.login, row.displayName].some((value) => String(value || '').toLowerCase().includes(normalized));
}

function renderLabeledBadge(label: string, tone: 'ok' | 'warning' | 'error') {
  const toneClass =
    tone === 'ok'
      ? 'border-success/35 bg-success/15 text-success'
      : tone === 'error'
        ? 'border-danger/35 bg-danger/15 text-danger'
        : 'border-warning/35 bg-warning/15 text-warning';

  return (
    <span
      className={[
        'inline-flex items-center rounded-full border px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em]',
        toneClass,
      ].join(' ')}
    >
      {label}
    </span>
  );
}

function renderPartnerStatus(status: string | undefined) {
  if (!status) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-sm text-white">—</span>
        <StatusBadge status="warning" />
      </div>
    );
  }
  return <StatusBadge status={status} />;
}

function readEngagementStatus(snapshot: EngagementSnapshotValue | undefined) {
  if (snapshot === 'loading') {
    return renderLabeledBadge('Lädt …', 'warning');
  }
  if (snapshot === 'error') {
    return renderLabeledBadge('Fehler', 'error');
  }
  if (snapshot?.enabled) {
    return renderLabeledBadge('Aktiv', 'ok');
  }
  return renderLabeledBadge('Inaktiv', 'warning');
}

function readEnabledAtLabel(snapshot: EngagementSnapshotValue | undefined) {
  if (snapshot === 'loading') {
    return '—';
  }
  if (snapshot === 'error') {
    return '—';
  }
  return snapshot?.enabledAt ? formatRelativeTime(snapshot.enabledAt) : '—';
}

function readEnabledByLabel(snapshot: EngagementSnapshotValue | undefined) {
  if (snapshot === 'loading' || snapshot === 'error') {
    return '—';
  }
  return snapshot?.enabledBy || '—';
}

function readNewestEnabledAt(snapshots: Map<string, EngagementSnapshotValue>) {
  let newestTimestamp = 0;
  let newestValue = '';

  snapshots.forEach((value) => {
    if (!value || value === 'loading' || value === 'error' || !value.enabledAt) {
      return;
    }
    const parsed = new Date(value.enabledAt);
    if (Number.isNaN(parsed.getTime())) {
      return;
    }
    if (parsed.getTime() > newestTimestamp) {
      newestTimestamp = parsed.getTime();
      newestValue = value.enabledAt;
    }
  });

  return newestValue;
}

function useEngagementSnapshot(streamers: StreamerRow[]) {
  const [snapshots, setSnapshots] = useState<Map<string, EngagementSnapshotValue>>(new Map());
  const [refreshNonce, setRefreshNonce] = useState(0);
  const [isRefreshing, setIsRefreshing] = useState(false);

  const logins = useMemo(() => {
    const uniqueLogins = new Set<string>();
    streamers.forEach((row) => {
      const login = row.login.trim().toLowerCase();
      if (login) {
        uniqueLogins.add(login);
      }
    });
    return Array.from(uniqueLogins);
  }, [streamers]);
  const loginsKey = logins.join('|');

  useEffect(() => {
    let cancelled = false;

    if (!logins.length) {
      setSnapshots(new Map());
      setIsRefreshing(false);
      return undefined;
    }

    setIsRefreshing(true);
    setSnapshots(() => {
      const next = new Map<string, EngagementSnapshotValue>();
      logins.forEach((login) => next.set(login, 'loading'));
      return next;
    });

    const queue = [...logins];
    const workerCount = Math.min(8, queue.length);

    async function worker() {
      while (!cancelled) {
        const login = queue.shift();
        if (!login) {
          return;
        }
        try {
          const settings = await fetchEngagementSettings(login);
          if (cancelled) {
            return;
          }
          setSnapshots((previous) => {
            const next = new Map(previous);
            next.set(login, settings);
            return next;
          });
        } catch {
          if (cancelled) {
            return;
          }
          setSnapshots((previous) => {
            const next = new Map(previous);
            next.set(login, 'error');
            return next;
          });
        }
      }
    }

    void Promise.all(Array.from({ length: workerCount }, () => worker())).finally(() => {
      if (!cancelled) {
        setIsRefreshing(false);
      }
    });

    return () => {
      cancelled = true;
    };
  }, [logins, loginsKey, refreshNonce]);

  async function refreshLogin(login: string) {
    const normalized = login.trim().toLowerCase();
    if (!normalized) {
      return;
    }

    setSnapshots((previous) => {
      const next = new Map(previous);
      next.set(normalized, 'loading');
      return next;
    });

    try {
      const settings = await fetchEngagementSettings(normalized);
      setSnapshots((previous) => {
        const next = new Map(previous);
        next.set(normalized, settings);
        return next;
      });
    } catch {
      setSnapshots((previous) => {
        const next = new Map(previous);
        next.set(normalized, 'error');
        return next;
      });
    }
  }

  function refreshAll() {
    setRefreshNonce((previous) => previous + 1);
  }

  return {
    snapshots,
    isRefreshing,
    refreshAll,
    refreshLogin,
  };
}

export default function EngagementPage() {
  const streamersQuery = useStreamers();
  const configQuery = useConfigOverview();
  const toggleMutation = useEngagementToggle();
  const [search, setSearch] = useState('');
  const [confirmState, setConfirmState] = useState<{ login: string; nextEnabled: boolean } | null>(null);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const streamers = streamersQuery.data ?? [];
  const { snapshots, isRefreshing, refreshAll, refreshLogin } = useEngagementSnapshot(streamers);
  const filteredStreamers = useMemo(
    () => streamers.filter((row) => matchesStreamerSearch(row, search)).sort((left, right) => left.login.localeCompare(right.login, 'de')),
    [search, streamers],
  );

  const activeChannels = streamers.reduce((count, row) => {
    const snapshot = snapshots.get(row.login);
    return snapshot && snapshot !== 'loading' && snapshot !== 'error' && snapshot.enabled ? count + 1 : count;
  }, 0);
  const inactiveChannels = Math.max(streamers.length - activeChannels, 0);
  const newestEnabledAt = readNewestEnabledAt(snapshots);

  const configRaw = coerceRecord(configQuery.data?.raw);
  const engagementDefaults = coerceRecord(configRaw.engagement);
  const hasEngagementDefaults = Object.keys(engagementDefaults).length > 0;

  const personaName =
    readString(engagementDefaults, 'personaName', 'persona_name', 'personaOverride', 'persona_override', 'persona') || '—';
  const autoOffTime =
    readNumber(
      engagementDefaults,
      'autoOffAfterMinutes',
      'auto_off_after_minutes',
      'autoOffMinutes',
      'auto_off_minutes',
      'autoOffThresholdMinutes',
      'auto_off_threshold_minutes',
    ) ?? null;
  const autoOffMessages =
    readNumber(
      engagementDefaults,
      'autoOffAfterMessages',
      'auto_off_after_messages',
      'autoOffMessageThreshold',
      'auto_off_message_threshold',
    ) ?? null;
  const lurkerThreshold =
    readNumber(
      engagementDefaults,
      'lurkerThreshold',
      'lurker_threshold',
      'lurkerMessageThreshold',
      'lurker_message_threshold',
    ) ?? null;

  const columns: TableColumn<StreamerRow>[] = [
    {
      key: 'login',
      title: 'Login',
      sortable: true,
      sortValue: (row) => row.login,
      className: 'min-w-[180px]',
      render: (row) => (
        <Link
          to={`/community/streamers/${encodeURIComponent(row.login)}`}
          className="font-semibold text-white transition hover:text-primary"
        >
          {row.login}
        </Link>
      ),
    },
    {
      key: 'displayName',
      title: 'Display Name',
      sortable: true,
      sortValue: (row) => row.displayName || row.login,
      render: (row) => row.displayName || '—',
    },
    {
      key: 'partnerStatus',
      title: 'Partner Status',
      sortable: true,
      sortValue: (row) => row.partnerStatus || 'zzz',
      render: (row) => renderPartnerStatus(row.partnerStatus),
    },
    {
      key: 'engagementStatus',
      title: 'Engagement-Status',
      sortable: true,
      sortValue: (row) => {
        const snapshot = snapshots.get(row.login);
        if (snapshot === 'loading') {
          return 1;
        }
        if (snapshot === 'error') {
          return 0;
        }
        return snapshot?.enabled ? 3 : 2;
      },
      render: (row) => readEngagementStatus(snapshots.get(row.login)),
    },
    {
      key: 'enabledAt',
      title: 'Enabled at',
      sortable: true,
      sortValue: (row) => {
        const snapshot = snapshots.get(row.login);
        if (!snapshot || snapshot === 'loading' || snapshot === 'error' || !snapshot.enabledAt) {
          return 0;
        }
        const parsed = new Date(snapshot.enabledAt);
        return Number.isNaN(parsed.getTime()) ? 0 : parsed.getTime();
      },
      render: (row) => readEnabledAtLabel(snapshots.get(row.login)),
    },
    {
      key: 'enabledBy',
      title: 'Enabled by',
      sortable: true,
      sortValue: (row) => {
        const snapshot = snapshots.get(row.login);
        return snapshot && snapshot !== 'loading' && snapshot !== 'error' ? snapshot.enabledBy || '' : '';
      },
      render: (row) => readEnabledByLabel(snapshots.get(row.login)),
    },
    {
      key: 'action',
      title: 'Aktion',
      render: (row) => {
        const snapshot = snapshots.get(row.login);
        const isEnabled = snapshot && snapshot !== 'loading' && snapshot !== 'error' ? snapshot.enabled : false;
        const disabled = snapshot === 'loading' || snapshot === 'error' || toggleMutation.isPending;

        return (
          <button
            type="button"
            className={`admin-button ${isEnabled ? 'admin-button-danger' : 'admin-button-primary'}`}
            disabled={disabled}
            onClick={() => setConfirmState({ login: row.login, nextEnabled: !isEnabled })}
          >
            {isEnabled ? 'Deaktivieren' : 'Aktivieren'}
          </button>
        );
      },
    },
  ];

  if (streamersQuery.isLoading && !streamersQuery.data) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Engagement-Daten werden geladen …</div>;
  }

  if (streamersQuery.isError) {
    return (
      <section className="space-y-6">
        <PageHeader
          title="Engagement AI"
          description="AI-Chat-Engagement pro Streamer steuern."
          primaryAction={
            <button className="admin-button admin-button-secondary" onClick={() => void streamersQuery.refetch()}>
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
          }
        />
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {streamersQuery.error instanceof Error ? streamersQuery.error.message : 'Streamer konnten nicht geladen werden.'}
        </div>
      </section>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Engagement AI"
        description="AI-Chat-Engagement pro Streamer steuern."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => {
              refreshAll();
              void streamersQuery.refetch();
            }}
            disabled={streamersQuery.isFetching || isRefreshing}
          >
            <RefreshCw className={`h-4 w-4 ${streamersQuery.isFetching || isRefreshing ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
        secondaryChips={<span className="stat-pill">Aktiv: {formatNumber(activeChannels)} von {formatNumber(streamers.length)}</span>}
      />

      <div className="grid gap-4 md:grid-cols-3">
        <KpiCard title="Aktive Channels" value={formatNumber(activeChannels)} hint="Engagement aktuell aktiv" tone="primary" icon={UserCheck} />
        <KpiCard title="Inaktive Channels" value={formatNumber(inactiveChannels)} hint="Disabled oder noch unbekannt" tone="neutral" icon={UserMinus} />
        <KpiCard title="Zuletzt aktiviert" value={newestEnabledAt ? formatRelativeTime(newestEnabledAt) : '—'} hint="Neuester Enable-Timestamp" tone="accent" icon={Sparkles} />
      </div>

      <Section title="Channel-Engagement" hint="Engagement pro Streamer" action={<div className="w-full sm:w-80"><SearchInput placeholder="Nach Login oder Display Name suchen …" onDebouncedChange={setSearch} /></div>}>
        <DataTable
          columns={columns}
          rows={filteredStreamers}
          rowKey={(row) => row.login}
          emptyState={
            <EmptyState
              icon={SearchX}
              title="Keine Streamer im Filter"
              description="Die aktuelle Suche liefert keine verwalteten Streamer für die Engagement-Ansicht."
            />
          }
        />
      </Section>

      <Section title="Persona & Auto-Off (Read-Only)" hint="Globale Engagement-Defaults">
        {hasEngagementDefaults ? (
          <div className="grid gap-4 lg:grid-cols-3">
            <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
              <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Persona</p>
              <div className="mt-3 flex items-center gap-2">
                <span className="text-sm text-white">{personaName}</span>
                {personaName === '—' ? <StatusBadge status="warning" /> : <StatusBadge status="ok" />}
              </div>
            </article>
            <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
              <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Auto-Off-Schwellen</p>
              <div className="mt-3 space-y-2 text-sm text-white">
                <div className="flex items-center justify-between gap-3">
                  <span>Zeit</span>
                  <span>{autoOffTime !== null ? `${formatNumber(autoOffTime)} min` : '—'}</span>
                </div>
                <div className="flex items-center justify-between gap-3">
                  <span>Nachrichten</span>
                  <span>{autoOffMessages !== null ? formatNumber(autoOffMessages) : '—'}</span>
                </div>
              </div>
            </article>
            <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
              <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Lurker-Schwelle</p>
              <div className="mt-3 flex items-center gap-2">
                <span className="text-sm text-white">{lurkerThreshold !== null ? formatNumber(lurkerThreshold) : '—'}</span>
                {lurkerThreshold === null ? <StatusBadge status="warning" /> : <StatusBadge status="ok" />}
              </div>
            </article>
          </div>
        ) : (
          <div className="rounded-[1.5rem] border border-warning/20 bg-warning/[0.04] p-5 text-sm leading-6 text-text-secondary">
            Persona-Steuerung wird in einer späteren Iteration ergänzt.
          </div>
        )}
      </Section>

      <ConfirmDialog
        open={Boolean(confirmState)}
        title={confirmState?.nextEnabled ? 'Engagement aktivieren?' : 'Engagement deaktivieren?'}
        description={
          confirmState
            ? `${confirmState.login} wird ${confirmState.nextEnabled ? 'für AI-Engagement aktiviert' : 'für AI-Engagement deaktiviert'}.`
            : ''
        }
        confirmLabel={confirmState?.nextEnabled ? 'Aktivieren' : 'Deaktivieren'}
        tone={confirmState?.nextEnabled ? 'default' : 'danger'}
        busy={toggleMutation.isPending}
        onCancel={() => setConfirmState(null)}
        onConfirm={() => {
          if (!confirmState) {
            return;
          }

          void toggleMutation.mutateAsync({ login: confirmState.login, enabled: confirmState.nextEnabled }).then((result) => {
            if (result.ok) {
              setToast({ open: true, tone: 'success', message: result.message });
              void refreshLogin(confirmState.login);
            } else {
              setToast({ open: true, tone: 'error', message: result.message });
            }
            setConfirmState(null);
          }).catch((error: unknown) => {
            setToast({
              open: true,
              tone: 'error',
              message: error instanceof Error ? error.message : 'Engagement konnte nicht umgeschaltet werden.',
            });
            setConfirmState(null);
          });
        }}
      />

      <Toast open={toast.open} tone={toast.tone} message={toast.message} onClose={() => setToast((previous) => ({ ...previous, open: false }))} />
    </section>
  );
}

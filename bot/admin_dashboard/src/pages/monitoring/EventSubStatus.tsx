import { RadioTower } from 'lucide-react';
import { PageHeader } from '@/components/layout/PageHeader';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { StatusBadge } from '@/components/shared/StatusBadge';
import type { EventSubSubscription } from '@/api/types';
import { useEventSubStatus } from '@/hooks/useAdmin';
import { coerceRecord, formatDateTime } from '@/utils/formatters';

export function EventSubStatusPage() {
  const eventSubQuery = useEventSubStatus();
  const data = eventSubQuery.data;
  const subscriptions = data?.subscriptions ?? [];
  const lastKnown = (data as Record<string, unknown> | undefined)?.lastKnownSubscriptions as EventSubSubscription[] | undefined ?? [];
  const lastKnownAt = (data as Record<string, unknown> | undefined)?.lastKnownSnapshotAt as string | undefined;

  const columns: TableColumn<EventSubSubscription>[] = [
    {
      key: 'type',
      title: 'Typ',
      sortable: true,
      sortValue: (row) => row.type ?? '',
      render: (row) => <span>{row.type || '—'}</span>,
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => row.status ?? '',
      render: (row) => <StatusBadge status={row.status} />,
    },
    {
      key: 'transport',
      title: 'Transport',
      render: (row) => row.transport || '—',
    },
    {
      key: 'created',
      title: 'Erstellt',
      sortable: true,
      sortValue: (row) => row.createdAt ?? '',
      render: (row) => formatDateTime(row.createdAt),
    },
  ];

  return (
    <section className="space-y-5">
      <PageHeader title="EventSub Status" description="WebSocket-Status, Subscription-Lage und Raw-Conditions der Twitch-Integration." />

      <div className="grid gap-4 md:grid-cols-3">
        <KpiCard title="WebSocket" value={data?.websocketStatus || '—'} hint={data?.websocketSessionId || 'keine Session-ID'} tone="accent" />
        <KpiCard title="Active Subs" value={String(data?.activeSubscriptionCount ?? subscriptions.length)} hint={data?.websocketConnectedAt ? `verbunden seit ${formatDateTime(data.websocketConnectedAt)}` : 'keine Connected-Zeit'} />
        <KpiCard title="Capacity" value={`${data?.capacity?.used ?? 0}/${data?.capacity?.max ?? 0}`} hint={data?.capacity?.lastSnapshotAt ? `Snapshot ${formatDateTime(data.capacity.lastSnapshotAt)}` : 'ohne Snapshot'} />
      </div>

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Subscriptions</p>
        <div className="mt-4">
          <DataTable
            columns={columns}
            rows={subscriptions}
            rowKey={(row, index) => row.id || `${index}`}
            emptyState={
              <EmptyState
                icon={RadioTower}
                title="Keine EventSub-Subscriptions"
                description="Der Snapshot enthält aktuell keine aktiven oder historischen EventSub-Einträge."
              />
            }
          />
        </div>
      </article>

      {subscriptions.length === 0 && lastKnown.length > 0 ? (
        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">
            Letzter bekannter Snapshot
            {lastKnownAt ? <span className="ml-2 font-normal normal-case text-text-secondary/70">{formatDateTime(lastKnownAt)}</span> : null}
          </p>
          <p className="mt-1 text-xs text-text-secondary">WebSocket aktuell inaktiv — zeigt den letzten Snapshot mit aktiven Subscriptions.</p>
          <div className="mt-4">
            <DataTable
              columns={columns}
              rows={lastKnown}
              rowKey={(row, index) => String((row as Record<string, unknown>).id ?? index)}
            />
          </div>
        </article>
      ) : null}

      <article className="panel-card rounded-[1.8rem] p-6">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Raw Condition Snapshot</p>
        <pre className="mt-4 overflow-auto rounded-[1.4rem] border border-white/10 bg-slate-950/55 p-4 text-xs leading-6 text-emerald-100">
          {JSON.stringify(
            subscriptions.map((subscription) => ({
              id: subscription.id,
              type: subscription.type,
              condition: coerceRecord(subscription.condition),
            })),
            null,
            2,
          )}
        </pre>
      </article>
    </section>
  );
}

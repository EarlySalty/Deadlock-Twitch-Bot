import { CreditCard } from 'lucide-react';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import type { SubscriptionRecord } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { EmptyState } from '@/components/shared/EmptyState';
import { useSubscriptions } from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';
import { StatusBadge } from '@/components/shared/StatusBadge';

export function Subscriptions() {
  const subscriptionsQuery = useSubscriptions();
  const rows = subscriptionsQuery.data ?? [];

  const columns: TableColumn<SubscriptionRecord>[] = [
    {
      key: 'login',
      title: 'Login',
      sortable: true,
      sortValue: (row) => row.login ?? row.customerReference ?? '',
      render: (row) => <span className="font-medium text-white">{row.login || row.customerReference || '—'}</span>,
    },
    {
      key: 'plan',
      title: 'Plan',
      sortable: true,
      sortValue: (row) => row.planId ?? '',
      render: (row) => row.planId || '—',
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => row.status ?? '',
      render: (row) => <StatusBadge status={row.status} />,
    },
    {
      key: 'periodEnd',
      title: 'Period End',
      sortable: true,
      sortValue: (row) => row.currentPeriodEnd ?? row.trialEndsAt ?? '',
      render: (row) => formatDateTime(row.currentPeriodEnd || row.trialEndsAt),
    },
  ];

  return (
    <section className="space-y-5">
      <PageHeader
        title="Subscription Übersicht"
        description="Stripe-Subscriptions mit aktuellem Plan- und Periodenstatus."
      />
      <article className="panel-card rounded-[1.8rem] p-6">
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(row, index) => `${row.customerReference ?? row.login ?? index}`}
          emptyState={
            <EmptyState
              icon={CreditCard}
              title="Keine Subscriptions sichtbar"
              description="Der aktuelle Snapshot enthält keine Stripe-Subscriptions."
            />
          }
        />
      </article>
    </section>
  );
}

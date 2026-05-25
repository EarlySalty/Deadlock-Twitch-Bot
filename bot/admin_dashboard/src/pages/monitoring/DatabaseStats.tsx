import { DatabaseZap } from 'lucide-react';
import { PageHeader } from '@/components/layout/PageHeader';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import type { DatabaseTableStat } from '@/api/types';
import { useDatabaseStats } from '@/hooks/useAdmin';
import { formatBytes, formatNumber } from '@/utils/formatters';

export function DatabaseStats() {
  const databaseQuery = useDatabaseStats();
  const rows = databaseQuery.data?.tables ?? [];

  const columns: TableColumn<DatabaseTableStat>[] = [
    {
      key: 'table',
      title: 'Tabelle',
      sortable: true,
      sortValue: (row) => row.table,
      render: (row) => <span className="font-medium text-white">{row.table}</span>,
    },
    {
      key: 'rows',
      title: 'Rows',
      sortable: true,
      sortValue: (row) => row.rowCount ?? 0,
      render: (row) => formatNumber(row.rowCount ?? 0),
    },
    {
      key: 'size',
      title: 'Größe',
      sortable: true,
      sortValue: (row) => row.sizeBytes ?? 0,
      render: (row) => formatBytes(row.sizeBytes ?? 0),
    },
  ];

  return (
    <section className="space-y-5">
      <PageHeader title="Database Stats" description="Tabellengrößen und Row-Counts aus dem aktuellen Admin-Snapshot." />

      <div className="grid gap-4 md:grid-cols-2">
        <KpiCard title="DB Gesamtgröße" value={formatBytes(databaseQuery.data?.databaseSizeBytes)} hint="vom Admin-Endpoint geliefert" tone="primary" />
        <KpiCard title="Tabellen im Snapshot" value={formatNumber(rows.length)} hint="nur gelieferte Tabellen" />
      </div>

      <article className="panel-card rounded-[1.8rem] p-6">
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(row) => row.table}
          emptyState={
            <EmptyState
              icon={DatabaseZap}
              title="Keine Tabellenstatistiken vorhanden"
              description="Der aktuelle Datenbank-Snapshot enthält noch keine Tabellenzeilen."
            />
          }
        />
      </article>
    </section>
  );
}

import { useEffect, useMemo, useState } from 'react';
import { Clock3, Layers3, Link2, ListChecks, RefreshCw, SearchX } from 'lucide-react';
import { Link } from 'react-router';
import type { AuditLogEntry } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { SearchInput } from '@/components/shared/SearchInput';
import { useAuditLog } from '@/hooks/useAdmin';
import { formatDateTime, formatNumber, formatRelativeTime } from '@/utils/formatters';

const DEFAULT_LIMIT = 100;
const DEFAULT_LOOKBACK_DAYS = 7;

function buildDefaultSinceDate(): string {
  const date = new Date();
  date.setUTCDate(date.getUTCDate() - DEFAULT_LOOKBACK_DAYS);
  return date.toISOString().slice(0, 10);
}

function buildSinceIso(value: string): string | undefined {
  const normalized = value.trim();
  if (!normalized) {
    return undefined;
  }
  const date = new Date(`${normalized}T00:00:00Z`);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}

function isLoginTarget(value: string | null | undefined): boolean {
  return Boolean(value && /^[a-z0-9_]{3,25}$/i.test(value.trim()));
}

function matchesSearch(entry: AuditLogEntry, search: string): boolean {
  const normalized = search.trim().toLowerCase();
  if (!normalized) {
    return true;
  }
  return [entry.actor ?? '', entry.target ?? '', entry.description, entry.action]
    .some((value) => value.toLowerCase().includes(normalized));
}

function SourceChip({
  source,
  active,
  onClick,
}: {
  source: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        'rounded-full border px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.16em] transition',
        active
          ? 'border-primary/50 bg-primary/18 text-white'
          : 'border-white/10 bg-white/5 text-text-secondary hover:border-white/20 hover:text-white',
      ].join(' ')}
    >
      {source}
    </button>
  );
}

function SourceBadge({ source }: { source: string }) {
  return (
    <span className="inline-flex rounded-full border border-white/10 bg-white/5 px-3 py-1 text-[0.68rem] font-semibold uppercase tracking-[0.16em] text-white">
      {source}
    </span>
  );
}

function renderTargetCell(entry: AuditLogEntry) {
  if (!entry.target) {
    return '—';
  }
  if (isLoginTarget(entry.target)) {
    return (
      <Link
        to={`/community/streamers/${encodeURIComponent(entry.target)}`}
        className="inline-flex items-center gap-2 font-medium text-white hover:text-primary"
      >
        <Link2 className="h-4 w-4" />
        {entry.target}
      </Link>
    );
  }
  return <span className="font-medium text-white">{entry.target}</span>;
}

export default function AuditLogPage() {
  const [sinceDate, setSinceDate] = useState(buildDefaultSinceDate);
  const [limit, setLimit] = useState(DEFAULT_LIMIT);
  const [selectedSources, setSelectedSources] = useState<string[]>([]);
  const [search, setSearch] = useState('');

  const sinceIso = buildSinceIso(sinceDate);
  const auditQuery = useAuditLog({ since: sinceIso, limit });
  const data = auditQuery.data;

  useEffect(() => {
    setLimit(DEFAULT_LIMIT);
  }, [sinceDate]);

  useEffect(() => {
    if (!data?.sources?.length) {
      setSelectedSources([]);
      return;
    }
    setSelectedSources((current) => current.filter((source) => data.sources.includes(source)));
  }, [data?.sources]);

  const visibleEntries = useMemo(() => {
    const activeSources = new Set(selectedSources);
    return (data?.entries ?? []).filter((entry) => {
      if (activeSources.size > 0 && !activeSources.has(entry.source)) {
        return false;
      }
      return matchesSearch(entry, search);
    });
  }, [data?.entries, search, selectedSources]);

  const latestEntry = visibleEntries[0] ?? data?.entries?.[0];
  const hoursWindow = sinceIso
    ? Math.max(1, Math.round((Date.now() - new Date(sinceIso).getTime()) / 3_600_000))
    : 0;

  const columns: TableColumn<AuditLogEntry>[] = [
    {
      key: 'timestamp',
      title: 'Zeit',
      sortable: true,
      sortValue: (row) => row.timestamp,
      className: 'min-w-[160px]',
      render: (row) => (
        <div title={formatDateTime(row.timestamp)}>
          <div className="font-medium text-white">{formatRelativeTime(row.timestamp)}</div>
          <div className="mt-1 text-xs text-text-secondary">{formatDateTime(row.timestamp)}</div>
        </div>
      ),
    },
    {
      key: 'source',
      title: 'Source',
      sortable: true,
      sortValue: (row) => row.source,
      render: (row) => <SourceBadge source={row.source} />,
    },
    {
      key: 'action',
      title: 'Action',
      sortable: true,
      sortValue: (row) => row.action,
      render: (row) => <span className="font-medium text-white">{row.action}</span>,
    },
    {
      key: 'actor',
      title: 'Actor',
      sortable: true,
      sortValue: (row) => row.actor ?? '',
      render: (row) => row.actor || '—',
    },
    {
      key: 'target',
      title: 'Target',
      sortable: true,
      sortValue: (row) => row.target ?? '',
      className: 'min-w-[180px]',
      render: renderTargetCell,
    },
    {
      key: 'description',
      title: 'Beschreibung',
      sortable: true,
      sortValue: (row) => row.description,
      className: 'min-w-[360px]',
      render: (row) => <span className="leading-6 text-white/95">{row.description}</span>,
    },
  ];

  if (auditQuery.isLoading && !data) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Audit-Log wird geladen …</div>;
  }

  if (auditQuery.isError) {
    return (
      <section className="space-y-6">
        <PageHeader
          title="Audit Log"
          description="Alle Admin-Aktionen mit Wer/Was/Wann."
          primaryAction={
            <button
              className="admin-button admin-button-secondary"
              onClick={() => void auditQuery.refetch()}
              disabled={auditQuery.isFetching}
            >
              <RefreshCw className={`h-4 w-4 ${auditQuery.isFetching ? 'animate-spin' : ''}`} />
              Refresh
            </button>
          }
        />
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {auditQuery.error instanceof Error ? auditQuery.error.message : 'Audit-Log konnte nicht geladen werden.'}
        </div>
      </section>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Audit Log"
        description="Alle Admin-Aktionen mit Wer/Was/Wann."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => void auditQuery.refetch()}
            disabled={auditQuery.isFetching}
          >
            <RefreshCw className={`h-4 w-4 ${auditQuery.isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
        secondaryChips={
          <span className="stat-pill">
            Letzte {formatNumber(hoursWindow)} Stunden: {formatNumber(data?.totalCount ?? 0)} Eintraege
          </span>
        }
      />

      <div className="grid gap-4 md:grid-cols-3">
        <KpiCard
          title="Eintraege gesamt"
          value={formatNumber(data?.totalCount ?? 0)}
          hint={`${formatNumber(visibleEntries.length)} aktuell sichtbar`}
          tone="primary"
          icon={ListChecks}
        />
        <KpiCard
          title="Quellen aktiv"
          value={formatNumber(data?.sources.length ?? 0)}
          hint={selectedSources.length ? `${formatNumber(selectedSources.length)} gefiltert` : 'Alle Quellen sichtbar'}
          tone="accent"
          icon={Layers3}
        />
        <KpiCard
          title="Letzte Aktion vor"
          value={latestEntry ? formatRelativeTime(latestEntry.timestamp) : '—'}
          hint={latestEntry ? formatDateTime(latestEntry.timestamp) : 'Noch keine Eintraege'}
          tone="neutral"
          icon={Clock3}
        />
      </div>

      <Section title="Filter" hint="Zeitraum, Quellen und Freitext für Target, Actor oder Beschreibung.">
        <div className="space-y-5">
          <div className="space-y-3">
            <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Sources</p>
            <div className="flex flex-wrap gap-2">
              <SourceChip
                source="alle"
                active={selectedSources.length === 0}
                onClick={() => setSelectedSources([])}
              />
              {(data?.sources ?? []).map((source) => (
                <SourceChip
                  key={source}
                  source={source}
                  active={selectedSources.includes(source)}
                  onClick={() =>
                    setSelectedSources((current) =>
                      current.includes(source)
                        ? current.filter((entry) => entry !== source)
                        : [...current, source],
                    )
                  }
                />
              ))}
            </div>
          </div>

          <div className="grid gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
            <label className="space-y-2">
              <span className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Seit</span>
              <input
                type="date"
                value={sinceDate}
                onChange={(event) => setSinceDate(event.target.value)}
                className="admin-input"
              />
            </label>
            <div className="space-y-2">
              <span className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Suche</span>
              <SearchInput
                placeholder="Target, Actor oder Beschreibung durchsuchen"
                defaultValue={search}
                onDebouncedChange={setSearch}
              />
            </div>
          </div>
        </div>
      </Section>

      <Section
        title="Aktionen"
        hint="Absteigend nach Timestamp. Bei Bedarf kann der geladene Zeitraum mit Mehr laden erweitert werden."
      >
        <DataTable
          columns={columns}
          rows={visibleEntries}
          rowKey={(row) => row.id}
          initialSortKey="timestamp"
          initialSortDirection="desc"
          emptyState={
            <EmptyState
              icon={SearchX}
              title={auditQuery.isLoading ? 'Audit-Log wird geladen' : 'Keine Audit-Einträge'}
              description={
                auditQuery.isLoading
                  ? 'Die Audit-Daten werden gerade nachgeladen.'
                  : 'Für den gewählten Zeitraum und die aktuellen Filter wurden keine Aktionen gefunden.'
              }
            />
          }
        />

        <div className="mt-4 flex items-center justify-between gap-4">
          <p className="text-sm text-text-secondary">
            {formatNumber(visibleEntries.length)} von {formatNumber(data?.totalCount ?? 0)} Eintraegen geladen
          </p>
          {data?.hasMore ? (
            <button
              type="button"
              className="admin-button admin-button-secondary"
              onClick={() => setLimit((current) => Math.min(current * 2, 500))}
              disabled={auditQuery.isFetching}
            >
              Mehr laden
            </button>
          ) : null}
        </div>
      </Section>
    </section>
  );
}

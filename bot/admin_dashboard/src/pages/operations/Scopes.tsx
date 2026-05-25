import { useState, type ReactNode } from 'react';
import { AlertTriangle, Link2, RefreshCw, ShieldAlert, ShieldCheck } from 'lucide-react';
import { Link } from 'react-router-dom';
import type { ScopeStatusResponse, ScopeStatusRow } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { KpiCard } from '@/components/shared/KpiCard';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { useScopeStatus } from '@/hooks/useAdmin';
import { formatNumber } from '@/utils/formatters';

function InlineScopeChip({
  label,
  tone = 'neutral',
  icon,
}: {
  label: string;
  tone?: 'neutral' | 'critical' | 'missing';
  icon?: ReactNode;
}) {
  const toneClass =
    tone === 'critical'
      ? 'border-amber-400/35 bg-amber-500/14 text-amber-100'
      : tone === 'missing'
        ? 'border-red-400/35 bg-red-500/14 text-red-100'
        : 'border-white/10 bg-white/[0.04] text-slate-100';

  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium ${toneClass}`}>
      {icon}
      <span>{label}</span>
    </span>
  );
}

function readStatusTone(missingCount: number): 'ok' | 'warning' | 'error' {
  if (missingCount > 3) {
    return 'error';
  }
  if (missingCount > 0) {
    return 'warning';
  }
  return 'ok';
}

function matchesSearch(item: ScopeStatusRow, search: string) {
  const normalized = search.trim().toLowerCase();
  if (!normalized) {
    return true;
  }
  return [item.login, item.displayName]
    .some((value) => String(value || '').toLowerCase().includes(normalized));
}

function summarizeGrantedScopes(scopes: string[]) {
  if (!scopes.length) {
    return [];
  }
  return scopes.slice(0, 4);
}

function sortScopeItems(items: ScopeStatusRow[]) {
  return [...items].sort((left, right) => {
    const missingDiff = right.missingScopes.length - left.missingScopes.length;
    if (missingDiff !== 0) {
      return missingDiff;
    }
    return left.login.localeCompare(right.login, 'de');
  });
}

function renderSummaryChip(label: string, value: number, tone: 'neutral' | 'warning' = 'neutral') {
  return (
    <span
      className={[
        'stat-pill',
        tone === 'warning' ? '!border-red-400/30 !bg-red-500/10 !text-red-100' : '',
      ].join(' ')}
    >
      {label}: {formatNumber(value)}
    </span>
  );
}

function renderMissingScopes(scopes: string[], criticalScopes: Set<string>) {
  if (!scopes.length) {
    return <StatusBadge status="ok" />;
  }

  return (
    <div className="flex flex-wrap gap-2">
      {scopes.map((scope) => (
        <InlineScopeChip
          key={scope}
          label={scope}
          tone="missing"
          icon={criticalScopes.has(scope) ? <AlertTriangle className="h-3.5 w-3.5" /> : undefined}
        />
      ))}
    </div>
  );
}

function renderGrantedScopes(scopes: string[]) {
  if (!scopes.length) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-sm text-white">—</span>
        <StatusBadge status="warning" />
      </div>
    );
  }

  const visibleScopes = summarizeGrantedScopes(scopes);
  const hiddenCount = scopes.length - visibleScopes.length;

  return (
    <div className="space-y-2" title={scopes.join(', ')}>
      <div className="text-sm font-medium text-white">{formatNumber(scopes.length)} Scopes</div>
      <div className="flex flex-wrap gap-2">
        {visibleScopes.map((scope) => (
          <InlineScopeChip key={scope} label={scope} />
        ))}
        {hiddenCount > 0 ? <InlineScopeChip label={`+${hiddenCount} more`} /> : null}
      </div>
    </div>
  );
}

function renderStatusWithFallback(value: string | undefined) {
  if (!value) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-sm text-white">—</span>
        <StatusBadge status="warning" />
      </div>
    );
  }
  return <StatusBadge status={value} />;
}

function renderHeader(response: ScopeStatusResponse | undefined, onRefresh: () => void, isRefreshing: boolean) {
  const summary = response?.summary;
  return (
    <PageHeader
      title="Scopes & OAuth"
      description="OAuth-Scopes pro Streamer mit Diff zur Soll-Konfiguration."
      primaryAction={
        <button className="admin-button admin-button-secondary" onClick={onRefresh} disabled={isRefreshing}>
          <RefreshCw className={`h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
          Refresh
        </button>
      }
      secondaryChips={
        <>
          {renderSummaryChip('Total Authorized', summary?.totalAuthorized ?? 0)}
          {renderSummaryChip('Voller Scope-Satz', summary?.fullScopeCount ?? 0)}
          {renderSummaryChip('Reauth nötig', summary?.missingScopeCount ?? 0, (summary?.missingScopeCount ?? 0) > 0 ? 'warning' : 'neutral')}
        </>
      }
    />
  );
}

export default function ScopesPage() {
  const [search, setSearch] = useState('');
  const scopeQuery = useScopeStatus();

  if (scopeQuery.isLoading && !scopeQuery.data) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">OAuth-Scopes werden geladen …</div>;
  }

  if (scopeQuery.isError) {
    return (
      <section className="space-y-6">
        {renderHeader(undefined, () => void scopeQuery.refetch(), scopeQuery.isFetching)}
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {scopeQuery.error instanceof Error ? scopeQuery.error.message : 'Scope-Status konnte nicht geladen werden.'}
        </div>
      </section>
    );
  }

  const response = scopeQuery.data;
  const summary = response?.summary ?? { totalAuthorized: 0, fullScopeCount: 0, missingScopeCount: 0 };
  const criticalScopes = new Set(response?.criticalScopes ?? []);
  const filteredItems = sortScopeItems((response?.items ?? []).filter((item) => matchesSearch(item, search)));
  const reauthTone = readStatusTone(summary.missingScopeCount);

  const columns: TableColumn<ScopeStatusRow>[] = [
    {
      key: 'login',
      title: 'Login',
      sortable: true,
      sortValue: (row) => row.login,
      className: 'min-w-[180px]',
      render: (row) => (
        <Link
          to={`/community/streamers/${encodeURIComponent(row.login)}`}
          className="inline-flex items-center gap-2 font-semibold text-white hover:text-primary"
        >
          <Link2 className="h-4 w-4" />
          {row.login}
        </Link>
      ),
    },
    {
      key: 'display-name',
      title: 'Display Name',
      sortable: true,
      sortValue: (row) => row.displayName || row.login,
      render: (row) => row.displayName || '—',
    },
    {
      key: 'partner-status',
      title: 'Partner Status',
      sortable: true,
      sortValue: (row) => row.partnerStatus || 'zzz',
      render: (row) => renderStatusWithFallback(row.partnerStatus),
    },
    {
      key: 'oauth-status',
      title: 'OAuth Status',
      sortable: true,
      sortValue: (row) => row.oauthStatus || 'zzz',
      render: (row) => renderStatusWithFallback(row.oauthStatus),
    },
    {
      key: 'granted-scopes',
      title: 'Granted Scopes',
      sortable: true,
      sortValue: (row) => row.grantedScopes.length,
      className: 'min-w-[260px]',
      render: (row) => renderGrantedScopes(row.grantedScopes),
    },
    {
      key: 'missing-scopes',
      title: 'Missing Scopes',
      sortable: true,
      sortValue: (row) => row.missingScopes.length,
      className: 'min-w-[260px]',
      render: (row) => renderMissingScopes(row.missingScopes, criticalScopes),
    },
    {
      key: 'reauth',
      title: 'Reauth Hinweis',
      sortable: true,
      sortValue: (row) => (row.oauthNeedsReauth ? 1 : 0),
      render: (row) =>
        row.oauthNeedsReauth ? (
          <span className="stat-pill !border-amber-400/35 !bg-amber-500/12 !text-amber-100">Reauth nötig</span>
        ) : (
          '—'
        ),
    },
  ];

  return (
    <section className="space-y-6">
      {renderHeader(response, () => void scopeQuery.refetch(), scopeQuery.isFetching)}

      <div className="grid gap-5 md:grid-cols-3">
        <KpiCard title="Total Authorized" value={formatNumber(summary.totalAuthorized)} hint="OAuth-Verknüpfungen mit Scope-Daten" tone="primary" icon={ShieldCheck} />
        <div className="relative">
          <KpiCard
            title="Mit vollem Scope-Satz"
            value={formatNumber(summary.fullScopeCount)}
            hint={summary.fullScopeCount === summary.totalAuthorized ? 'Alle autorisierten Streamer sind vollständig.' : 'Nicht alle Streamer haben den Soll-Satz.'}
            tone="accent"
            icon={ShieldCheck}
          />
          <div className="pointer-events-none absolute right-5 top-5">
            <StatusBadge status={summary.fullScopeCount === summary.totalAuthorized ? 'ok' : 'warning'} />
          </div>
        </div>
        <div className="relative">
          <KpiCard
            title="Re-Auth benötigt"
            value={formatNumber(summary.missingScopeCount)}
            hint={
              summary.missingScopeCount === 0
                ? 'Kein Scope-Diff offen.'
                : summary.missingScopeCount <= 3
                  ? 'Ein kleiner Reauth-Backlog ist offen.'
                  : 'Mehrere Streamer brauchen eine erneute Autorisierung.'
            }
            tone="neutral"
            icon={ShieldAlert}
          />
          <div className="pointer-events-none absolute right-5 top-5">
            <StatusBadge status={reauthTone} />
          </div>
        </div>
      </div>

      <Section title="Required vs Critical" hint="Erwarteter Scope-Satz">
        <div className="grid gap-5 lg:grid-cols-2">
          <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-5">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-sm font-semibold uppercase tracking-[0.2em] text-white">Required Scopes</h3>
              <span className="stat-pill">{formatNumber(response?.requiredScopes.length ?? 0)}</span>
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              {(response?.requiredScopes ?? []).length ? (
                (response?.requiredScopes ?? []).map((scope) => <InlineScopeChip key={scope} label={scope} />)
              ) : (
                <div className="flex items-center gap-2">
                  <span className="text-sm text-white">—</span>
                  <StatusBadge status="warning" />
                </div>
              )}
            </div>
          </article>

          <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-5">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-sm font-semibold uppercase tracking-[0.2em] text-white">Critical Scopes</h3>
              <span className="stat-pill !border-amber-400/30 !bg-amber-500/10 !text-amber-100">
                {formatNumber(response?.criticalScopes.length ?? 0)}
              </span>
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              {(response?.criticalScopes ?? []).length ? (
                (response?.criticalScopes ?? []).map((scope) => (
                  <InlineScopeChip key={scope} label={scope} tone="critical" />
                ))
              ) : (
                <div className="flex items-center gap-2">
                  <span className="text-sm text-white">—</span>
                  <StatusBadge status="warning" />
                </div>
              )}
            </div>
          </article>
        </div>
      </Section>

      <Section
        title="Streamer-Diff"
        hint="Welcher Streamer hat welche Scopes"
        action={
          <div className="w-full max-w-sm">
            <SearchInput
              placeholder="Nach Login oder Display Name suchen …"
              defaultValue={search}
              onDebouncedChange={setSearch}
            />
          </div>
        }
      >
        <DataTable
          columns={columns}
          rows={filteredItems}
          rowKey={(row) => row.login}
          emptyLabel="Keine Streamer mit OAuth-Status gefunden."
        />
      </Section>
    </section>
  );
}

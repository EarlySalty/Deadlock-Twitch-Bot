import { useState } from 'react';
import { ChevronRight, Coins, FileText, RefreshCw, SearchX, ShieldCheck, Users } from 'lucide-react';
import type { AffiliateListItem } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { AffiliateDetailPanel } from '@/pages/billing/AffiliateDetailPanel';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import {
  useAffiliateStats,
  useAffiliatesList,
  useGenerateGutschriften,
  useSetAffiliateCommissionRate,
  useToggleAffiliateActive,
} from '@/hooks/useAdmin';
import { parseCommissionRate } from '@/utils/commissionRate';
import { formatCurrencyEuro, formatDateTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  message: string;
  tone: 'success' | 'error';
};

function summarizeGenerateResult(result: { results?: Array<{ ok?: boolean; action?: string; status?: string }> }) {
  const entries = result.results ?? [];
  if (!entries.length) {
    return 'Keine faelligen Gutschriften gefunden.';
  }

  const createdCount = entries.filter((entry) => entry.ok && entry.action !== 'existing').length;
  const existingCount = entries.filter((entry) => entry.action === 'existing').length;
  const blockedCount = entries.filter((entry) => entry.status === 'blocked').length;
  const skippedCount = entries.filter((entry) => entry.status === 'no_commissions').length;

  const parts = [
    createdCount > 0 ? `${createdCount} erstellt oder versendet` : '',
    existingCount > 0 ? `${existingCount} bereits vorhanden` : '',
    blockedCount > 0 ? `${blockedCount} blockiert` : '',
    skippedCount > 0 ? `${skippedCount} ohne Provisionen` : '',
  ].filter(Boolean);

  return parts.join(', ') || 'Generierung abgeschlossen.';
}

export function Affiliates() {
  const [search, setSearch] = useState('');
  const [selectedLogin, setSelectedLogin] = useState<string | null>(null);
  const [toggleTarget, setToggleTarget] = useState<AffiliateListItem | null>(null);
  const [rateDrafts, setRateDrafts] = useState<Record<string, string>>({});
  const [confirmGenerateAll, setConfirmGenerateAll] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, message: '', tone: 'success' });

  const statsQuery = useAffiliateStats();
  const affiliatesQuery = useAffiliatesList();
  const toggleMutation = useToggleAffiliateActive();
  const rateMutation = useSetAffiliateCommissionRate();
  const generateMutation = useGenerateGutschriften();

  const rows = affiliatesQuery.data ?? [];
  const stats = statsQuery.data;
  const normalizedSearch = search.trim().toLowerCase();
  const filteredRows = rows.filter((row) => {
    if (!normalizedSearch) {
      return true;
    }
    return [row.login, row.displayName ?? ''].some((value) => value.toLowerCase().includes(normalizedSearch));
  });

  const columns: TableColumn<AffiliateListItem>[] = [
    {
      key: 'affiliate',
      title: 'Affiliate',
      sortable: true,
      sortValue: (row) => row.displayName ?? row.login,
      render: (row) => (
        <button
          type="button"
          onClick={() => setSelectedLogin((current) => (current === row.login ? null : row.login))}
          className="group text-left"
        >
          <div className="font-medium text-white transition group-hover:text-primary">{row.displayName || row.login}</div>
          <div className="mt-1 text-xs uppercase tracking-[0.18em] text-text-secondary">{row.login}</div>
        </button>
      ),
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => `${row.active ? '1' : '0'}-${row.status ?? ''}`,
      render: (row) => (
        <div className="flex flex-wrap gap-2">
          <StatusBadge status={row.active ? 'active' : 'inactive'} />
          {row.stripeConnectStatus ? <StatusBadge status={row.stripeConnectStatus} /> : null}
        </div>
      ),
    },
    {
      key: 'claims',
      title: 'Claims',
      sortable: true,
      sortValue: (row) => row.totalClaims,
      render: (row) => <span className="font-medium text-white">{row.totalClaims}</span>,
    },
    {
      key: 'provision',
      title: 'Provision',
      sortable: true,
      sortValue: (row) => row.totalProvisionEuro,
      render: (row) => formatCurrencyEuro(row.totalProvisionEuro),
    },
    {
      key: 'commissionRate',
      title: 'Provision (%)',
      render: (row) => (
        <div className="flex min-w-48 items-center gap-2">
          <input
            type="number"
            min="0"
            max="100"
            step="1"
            aria-label={`Provision (%) für ${row.login}`}
            className="admin-input w-20 px-3 py-2 text-sm"
            value={rateDrafts[row.login] ?? String(row.commissionRatePct)}
            onChange={(event) =>
              setRateDrafts((current) => ({ ...current, [row.login]: event.target.value }))
            }
          />
          <button
            type="button"
            className="admin-button admin-button-secondary whitespace-nowrap px-3 py-2 text-xs"
            disabled={rateMutation.isPending}
            onClick={() => {
              void handleSaveRate(row);
            }}
          >
            Satz speichern
          </button>
        </div>
      ),
    },
    {
      key: 'lastClaim',
      title: 'Letzter Claim',
      sortable: true,
      sortValue: (row) => row.lastClaimAt ?? '',
      render: (row) => formatDateTime(row.lastClaimAt),
    },
    {
      key: 'createdAt',
      title: 'Mitglied seit',
      sortable: true,
      sortValue: (row) => row.createdAt ?? '',
      render: (row) => formatDateTime(row.createdAt),
    },
    {
      key: 'actions',
      title: 'Aktionen',
      render: (row) => (
        <div className="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="admin-button admin-button-secondary px-3 py-2 text-xs"
            onClick={() => setToggleTarget(row)}
            disabled={toggleMutation.isPending}
          >
            {row.active ? 'Deaktivieren' : 'Aktivieren'}
          </button>
          <button
            type="button"
            className="admin-button admin-button-secondary px-3 py-2 text-xs"
            onClick={() => setSelectedLogin(row.login)}
          >
            Details
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      ),
    },
  ];

  async function handleConfirmToggle() {
    if (!toggleTarget) {
      return;
    }

    try {
      const result = await toggleMutation.mutateAsync(toggleTarget.login);
      setToast({
        open: true,
        tone: 'success',
        message: `${result.login} ist jetzt ${result.active ? 'aktiv' : 'inaktiv'}.`,
      });
      setToggleTarget(null);
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Affiliate-Status konnte nicht aktualisiert werden.',
      });
    }
  }

  async function handleSaveRate(row: AffiliateListItem) {
    const commissionRatePct = parseCommissionRate(rateDrafts[row.login] ?? String(row.commissionRatePct));
    if (commissionRatePct === null) {
      setToast({ open: true, tone: 'error', message: 'Satz muss zwischen 0 und 100 liegen' });
      return;
    }

    try {
      await rateMutation.mutateAsync({ login: row.login, commissionRatePct });
      setRateDrafts((current) => ({ ...current, [row.login]: String(commissionRatePct) }));
      setToast({ open: true, tone: 'success', message: 'Provisionssatz gespeichert' });
    } catch {
      setToast({ open: true, tone: 'error', message: 'Speichern fehlgeschlagen' });
    }
  }

  async function handleGenerateAll() {
    try {
      const result = await generateMutation.mutateAsync({});
      setToast({
        open: true,
        tone: 'success',
        message: summarizeGenerateResult(result),
      });
      setConfirmGenerateAll(false);
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Gutschriften konnten nicht generiert werden.',
      });
    }
  }

  return (
    <>
      <section className="space-y-6">
        <PageHeader
          title="Affiliate-Verwaltung und Gutschriften"
          description="Aktivität, Provisionen und Gutschrift-Bereitschaft auf einen Blick. Leere oder fehlende Backend-Daten werden defensiv behandelt, damit die Oberfläche bedienbar bleibt."
          primaryAction={
            <div className="flex flex-col gap-3 sm:flex-row">
              <div className="min-w-[18rem] flex-1">
                <SearchInput placeholder="Nach Login oder Namen suchen" onDebouncedChange={setSearch} />
              </div>
              <button
                type="button"
                className="admin-button admin-button-primary"
                onClick={() => setConfirmGenerateAll(true)}
                disabled={generateMutation.isPending}
              >
                <RefreshCw className={`h-4 w-4 ${generateMutation.isPending ? 'animate-spin' : ''}`} />
                Gutschriften generieren
              </button>
            </div>
          }
        />

        {affiliatesQuery.isError ? (
          <div className="rounded-2xl border border-red-400/20 bg-red-500/10 px-4 py-3 text-sm text-red-100">
            {affiliatesQuery.error instanceof Error
              ? affiliatesQuery.error.message
              : 'Affiliate-Liste konnte nicht geladen werden.'}
          </div>
        ) : null}

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <KpiCard
            title="Affiliates"
            value={String(stats?.totalAffiliates ?? rows.length)}
            hint={`${stats?.activeAffiliates ?? rows.filter((row) => row.active).length} aktiv`}
            tone="primary"
            icon={Users}
          />
          <KpiCard
            title="Claims Gesamt"
            value={String(stats?.totalClaims ?? rows.reduce((sum, row) => sum + row.totalClaims, 0))}
            hint={statsQuery.isFetching ? 'Wird aktualisiert …' : 'Historische Claims'}
            tone="neutral"
            icon={ShieldCheck}
          />
          <KpiCard
            title="Provision Gesamt"
            value={formatCurrencyEuro(stats?.totalProvisionEuro ?? rows.reduce((sum, row) => sum + row.totalProvisionEuro, 0))}
            hint="Aus bestätigten Provisionen"
            tone="accent"
            icon={Coins}
          />
          <KpiCard
            title="Dieser Monat"
            value={String(stats?.thisMonthClaims ?? 0)}
            hint={formatCurrencyEuro(stats?.thisMonthProvisionEuro ?? 0)}
            tone="neutral"
            icon={FileText}
          />
        </div>

        <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]">
          <article className="panel-card rounded-[1.8rem] p-6">
            <div className="mb-4 flex items-center justify-between gap-4">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Affiliates</p>
                <p className="mt-2 text-sm text-text-secondary">
                  {filteredRows.length} von {rows.length} Einträgen sichtbar
                </p>
              </div>
            </div>
            <DataTable
              columns={columns}
              rows={filteredRows}
              rowKey={(row) => row.login}
              emptyState={
                <EmptyState
                  icon={SearchX}
                  title={affiliatesQuery.isLoading ? 'Affiliates werden geladen' : 'Keine Affiliates gefunden'}
                  description={
                    affiliatesQuery.isLoading
                      ? 'Die Affiliate-Liste wird gerade geladen.'
                      : normalizedSearch
                        ? 'Für die aktuelle Suche wurden keine Affiliate-Profile gefunden.'
                        : 'Es sind aktuell keine Affiliate-Profile vorhanden.'
                  }
                />
              }
            />
          </article>

          {selectedLogin ? (
            <AffiliateDetailPanel login={selectedLogin} onClose={() => setSelectedLogin(null)} />
          ) : (
            <aside className="panel-card flex min-h-[22rem] items-center justify-center rounded-[1.8rem] p-6">
              <EmptyState
                icon={FileText}
                title="Affiliate auswählen"
                description="Öffne einen Datensatz aus der Tabelle, um Stats, Readiness, letzte Claims und vorhandene Gutschriften zu prüfen."
                className="w-full border-transparent bg-transparent"
              />
            </aside>
          )}
        </div>
      </section>

      <ConfirmDialog
        open={Boolean(toggleTarget)}
        title={toggleTarget?.active ? 'Affiliate deaktivieren?' : 'Affiliate aktivieren?'}
        description={
          toggleTarget
            ? `Der Status von ${toggleTarget.displayName || toggleTarget.login} wird umgeschaltet.`
            : ''
        }
        confirmLabel={toggleTarget?.active ? 'Deaktivieren' : 'Aktivieren'}
        busy={toggleMutation.isPending}
        onConfirm={() => {
          void handleConfirmToggle();
        }}
        onCancel={() => setToggleTarget(null)}
      />

      <ConfirmDialog
        open={confirmGenerateAll}
        title="Alle faelligen Gutschriften generieren?"
        description="Es werden alle aktuell fälligen Gutschriften erzeugt und, falls möglich, direkt versendet."
        confirmLabel="Jetzt generieren"
        busy={generateMutation.isPending}
        onConfirm={() => {
          void handleGenerateAll();
        }}
        onCancel={() => setConfirmGenerateAll(false)}
      />

      <Toast
        open={toast.open}
        message={toast.message}
        tone={toast.tone}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </>
  );
}

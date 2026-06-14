import { useState } from 'react';
import { Coins, Download, FileText, Mail, RefreshCw, SearchX, TriangleAlert } from 'lucide-react';
import type { GutschriftDocument } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import { useAffiliatesList, useAllGutschriften, useGenerateGutschriften } from '@/hooks/useAdmin';
import { formatCurrency, formatDateTime } from '@/utils/formatters';

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

export function Gutschriften() {
  const [search, setSearch] = useState('');
  const [confirmGenerateAll, setConfirmGenerateAll] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, message: '', tone: 'success' });

  const documentsQuery = useAllGutschriften();
  const affiliatesQuery = useAffiliatesList();
  const generateMutation = useGenerateGutschriften();

  const displayNameByLogin = new Map<string, string>();
  for (const affiliate of affiliatesQuery.data ?? []) {
    if (affiliate.login) {
      displayNameByLogin.set(affiliate.login, affiliate.displayName || affiliate.login);
    }
  }

  const allDocuments = (documentsQuery.data ?? []).map((document) => ({
    ...document,
    affiliateDisplayName:
      document.affiliateDisplayName ||
      (document.affiliateLogin ? displayNameByLogin.get(document.affiliateLogin) : undefined),
  }));

  const normalizedSearch = search.trim().toLowerCase();
  const filteredDocuments = allDocuments.filter((document) => {
    if (!normalizedSearch) {
      return true;
    }
    return [
      document.affiliateLogin ?? '',
      document.affiliateDisplayName ?? '',
      document.gutschriftNumber ?? '',
      document.periodLabel ?? '',
      document.status ?? '',
    ].some((value) => value.toLowerCase().includes(normalizedSearch));
  });

  const totalGrossCents = allDocuments.reduce((sum, document) => sum + document.grossAmountCents, 0);
  const emailedCount = allDocuments.filter((document) => document.status === 'emailed').length;
  const openCount = allDocuments.filter((document) => document.status === 'generated').length;
  const issueCount = allDocuments.filter(
    (document) => document.status === 'blocked' || document.status === 'email_failed',
  ).length;

  const columns: TableColumn<GutschriftDocument>[] = [
    {
      key: 'affiliate',
      title: 'Affiliate',
      sortable: true,
      sortValue: (row) => row.affiliateDisplayName ?? row.affiliateLogin ?? '',
      render: (row) => (
        <div>
          <div className="font-medium text-white">{row.affiliateDisplayName || row.affiliateLogin || '—'}</div>
          <div className="mt-1 text-xs uppercase tracking-[0.16em] text-text-secondary">
            {row.affiliateLogin || 'kein Login'}
          </div>
        </div>
      ),
    },
    {
      key: 'period',
      title: 'Zeitraum',
      sortable: true,
      sortValue: (row) => `${row.periodYear ?? 0}-${row.periodMonth ?? 0}`,
      render: (row) => row.periodLabel || 'Offen',
    },
    {
      key: 'number',
      title: 'Nummer',
      sortable: true,
      sortValue: (row) => row.gutschriftNumber ?? '',
      render: (row) => <span className="font-medium text-white">{row.gutschriftNumber || 'Noch offen'}</span>,
    },
    {
      key: 'status',
      title: 'Status',
      sortable: true,
      sortValue: (row) => row.status ?? '',
      render: (row) => <StatusBadge status={row.status} />,
    },
    {
      key: 'gross',
      title: 'Brutto',
      sortable: true,
      sortValue: (row) => row.grossAmountCents,
      render: (row) => formatCurrency(row.grossAmountCents),
    },
    {
      key: 'sent',
      title: 'Aktualisiert',
      sortable: true,
      sortValue: (row) => row.emailedAt ?? row.generatedAt ?? row.createdAt ?? '',
      render: (row) => formatDateTime(row.emailedAt || row.generatedAt || row.createdAt),
    },
    {
      key: 'pdf',
      title: 'PDF',
      render: (row) =>
        row.hasPdf && row.downloadPath ? (
          <a
            href={row.downloadPath}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 text-sm font-medium text-primary transition hover:text-primary-hover"
          >
            <Download className="h-4 w-4" />
            Download
          </a>
        ) : (
          <span className="text-xs uppercase tracking-[0.16em] text-text-secondary">Kein PDF</span>
        ),
    },
  ];

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
          title="Globale Gutschriften-Übersicht"
          description="Zentrale Sicht auf generierte, versendete und blockierte Dokumente inklusive PDF-Download."
          primaryAction={
            <div className="flex flex-col gap-3 sm:flex-row">
              <div className="min-w-[18rem] flex-1">
                <SearchInput
                  placeholder="Nach Affiliate, Zeitraum oder Nummer suchen"
                  onDebouncedChange={setSearch}
                />
              </div>
              <button
                type="button"
                className="admin-button admin-button-primary"
                onClick={() => setConfirmGenerateAll(true)}
                disabled={generateMutation.isPending}
              >
                <RefreshCw className={`h-4 w-4 ${generateMutation.isPending ? 'animate-spin' : ''}`} />
                Alle generieren
              </button>
            </div>
          }
        />

        {documentsQuery.isError ? (
          <div className="rounded-2xl border border-red-400/20 bg-red-500/10 px-4 py-3 text-sm text-red-100">
            {documentsQuery.error instanceof Error
              ? documentsQuery.error.message
              : 'Gutschriften konnten nicht geladen werden.'}
          </div>
        ) : null}

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <KpiCard
            title="Dokumente"
            value={String(allDocuments.length)}
            hint={`${filteredDocuments.length} aktuell sichtbar`}
            tone="primary"
            icon={FileText}
          />
          <KpiCard
            title="Versendet"
            value={String(emailedCount)}
            hint={`${openCount} offen generiert`}
            tone="accent"
            icon={Mail}
          />
          <KpiCard
            title="Brutto Gesamt"
            value={formatCurrency(totalGrossCents)}
            hint="Summe aller gelisteten Dokumente"
            tone="neutral"
            icon={Coins}
          />
          <KpiCard
            title="Probleme"
            value={String(issueCount)}
            hint="Blockiert oder E-Mail fehlgeschlagen"
            tone="neutral"
            icon={TriangleAlert}
          />
        </div>

        <article className="panel-card rounded-[1.8rem] p-6">
          <div className="mb-4 flex items-center justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Dokumente</p>
              <p className="mt-2 text-sm text-text-secondary">
                {filteredDocuments.length} von {allDocuments.length} Dokumenten sichtbar
              </p>
            </div>
          </div>
          <DataTable
            columns={columns}
            rows={filteredDocuments}
            rowKey={(row) => `${row.affiliateLogin ?? 'affiliate'}-${row.gutschriftNumber ?? row.id ?? row.periodLabel ?? 'doc'}`}
            emptyState={
              <EmptyState
                icon={SearchX}
                title={documentsQuery.isLoading ? 'Gutschriften werden geladen' : 'Keine Gutschriften gefunden'}
                description={
                  documentsQuery.isLoading
                    ? 'Die globale Gutschriftenliste wird gerade geladen.'
                    : normalizedSearch
                      ? 'Für die aktuelle Suche wurden keine Dokumente gefunden.'
                      : 'Es sind aktuell keine Gutschriften im System sichtbar.'
                }
              />
            }
          />
        </article>
      </section>

      <ConfirmDialog
        open={confirmGenerateAll}
        title="Alle faelligen Gutschriften generieren?"
        description="Es werden alle aktuell fälligen Gutschriften erzeugt und, falls möglich, per E-Mail versendet."
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

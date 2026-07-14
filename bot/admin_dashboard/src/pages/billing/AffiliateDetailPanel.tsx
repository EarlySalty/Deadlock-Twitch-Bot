import { useState } from 'react';
import { CalendarDays, Coins, Download, FileText, Mail, RefreshCw, TriangleAlert, X } from 'lucide-react';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import { useAffiliateDetail, useAffiliateGutschriften, useGenerateGutschriften } from '@/hooks/useAdmin';
import { formatCurrency, formatCurrencyEuro, formatDateTime } from '@/utils/formatters';

interface AffiliateDetailPanelProps {
  login: string;
  onClose: () => void;
}

type ToastState = {
  open: boolean;
  message: string;
  tone: 'success' | 'error';
};

function summarizeGenerateResult(result: { results?: Array<{ ok?: boolean; action?: string; status?: string }> }) {
  const entries = result.results ?? [];
  if (!entries.length) {
    return 'Keine fälligen Gutschriften für diesen Affiliate gefunden.';
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

function MetricTile({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-[1.35rem] border border-white/10 bg-bg/35 px-4 py-4">
      <p className="text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-text-secondary">{label}</p>
      <p className="mt-2 text-xl font-semibold text-white">{value}</p>
      {hint ? <p className="mt-2 text-xs leading-5 text-text-secondary">{hint}</p> : null}
    </div>
  );
}

export function AffiliateDetailPanel({ login, onClose }: AffiliateDetailPanelProps) {
  const [confirmGenerate, setConfirmGenerate] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, message: '', tone: 'success' });

  const detailQuery = useAffiliateDetail(login);
  const gutschriftenQuery = useAffiliateGutschriften(login);
  const generateMutation = useGenerateGutschriften();

  const detail = detailQuery.data;
  const readiness = detail?.readiness ?? {
    canGenerate: false,
    blockers: [],
    warnings: [],
    missingFields: [],
    ustStatus: 'unknown',
  };
  const documents = gutschriftenQuery.data?.length ? gutschriftenQuery.data : detail?.gutschriften ?? [];
  const claims = detail?.claims ?? [];
  const latestDocuments = documents.slice(0, 4);
  const latestClaims = claims.slice(0, 5);

  async function handleGenerate() {
    try {
      const result = await generateMutation.mutateAsync({ affiliateLogin: login });
      setToast({
        open: true,
        tone: 'success',
        message: summarizeGenerateResult(result),
      });
      setConfirmGenerate(false);
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
      <aside className="panel-card rounded-[1.8rem] p-6">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.24em] text-text-secondary">Affiliate Detail</p>
            <h2 className="mt-3 text-2xl font-semibold text-white">{detail?.displayName || login}</h2>
            <p className="mt-2 text-sm uppercase tracking-[0.18em] text-text-secondary">{login}</p>
          </div>
          <button
            type="button"
            className="rounded-full border border-white/10 bg-white/5 p-2 text-white/70 transition hover:text-white"
            onClick={onClose}
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="mt-5 flex flex-wrap gap-2">
          <StatusBadge status={detail?.active ? 'active' : 'inactive'} />
          {detail?.stripeConnectStatus ? <StatusBadge status={detail.stripeConnectStatus} /> : null}
          <StatusBadge status={detail?.ustStatus || readiness.ustStatus || 'unknown'} />
          <StatusBadge status={readiness.canGenerate ? 'generated' : 'blocked'} />
        </div>

        <div className="mt-5">
          <button
            type="button"
            className="admin-button admin-button-primary w-full"
            onClick={() => setConfirmGenerate(true)}
            disabled={generateMutation.isPending}
          >
            <RefreshCw className={`h-4 w-4 ${generateMutation.isPending ? 'animate-spin' : ''}`} />
            Gutschrift generieren
          </button>
        </div>

        {detailQuery.isError ? (
          <div className="mt-5 rounded-2xl border border-danger/20 bg-danger/10 px-4 py-3 text-sm text-danger">
            {detailQuery.error instanceof Error ? detailQuery.error.message : 'Affiliate-Details konnten nicht geladen werden.'}
          </div>
        ) : null}

        <div className="mt-6 grid gap-3 sm:grid-cols-2">
          <MetricTile
            label="Claims"
            value={String(detail?.stats.totalClaims ?? 0)}
            hint={`${latestClaims.length} zuletzt sichtbar`}
          />
          <MetricTile
            label="Provision"
            value={formatCurrencyEuro(detail?.stats.totalProvisionEuro ?? 0)}
            hint="Gesamt aus bestätigten Umsätzen"
          />
          <MetricTile
            label="Ø pro Claim"
            value={formatCurrencyEuro(detail?.stats.avgProvisionEuro ?? 0)}
            hint="Durchschnittliche Provision"
          />
          <MetricTile
            label="Aktive Kunden"
            value={String(detail?.stats.activeCustomers ?? 0)}
            hint="Mit bestätigten Provisionen"
          />
        </div>

        <section className="mt-6 space-y-3 rounded-[1.5rem] border border-white/10 bg-bg/30 p-4">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Readiness</p>
              <p className="mt-2 text-sm leading-6 text-text-secondary">
                {readiness.canGenerate
                  ? 'Das Profil ist für die automatische Gutschrift-Erstellung vorbereitet.'
                  : 'Das Profil blockiert aktuell die automatische Gutschrift-Erstellung.'}
              </p>
            </div>
            <StatusBadge status={readiness.canGenerate ? 'generated' : 'blocked'} />
          </div>

          {readiness.blockers.length ? (
            <div className="space-y-2 rounded-2xl border border-warning/20 bg-warning/10 p-4">
              {readiness.blockers.map((blocker) => (
                <div key={blocker} className="flex items-start gap-2 text-sm text-warning">
                  <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
                  <span>{blocker}</span>
                </div>
              ))}
            </div>
          ) : null}

          {readiness.warnings.length ? (
            <div className="space-y-2">
              {readiness.warnings.map((warning) => (
                <div
                  key={warning}
                  className="rounded-2xl border border-white/10 bg-white/5 px-4 py-3 text-sm leading-6 text-text-secondary"
                >
                  {warning}
                </div>
              ))}
            </div>
          ) : null}

          {readiness.missingFields.length ? (
            <div className="flex flex-wrap gap-2">
              {readiness.missingFields.map((field) => (
                <span
                  key={field}
                  className="rounded-full border border-white/10 bg-white/5 px-3 py-1 text-[0.68rem] font-semibold uppercase tracking-[0.16em] text-text-secondary"
                >
                  {field.replace(/_/g, ' ')}
                </span>
              ))}
            </div>
          ) : null}
        </section>

        <section className="mt-6 rounded-[1.5rem] border border-white/10 bg-bg/30 p-4">
          <div className="flex items-center gap-2 text-white">
            <FileText className="h-4 w-4 text-primary" />
            <h3 className="text-sm font-semibold uppercase tracking-[0.2em] text-text-secondary">Gutschriften</h3>
          </div>

          <div className="mt-4 overflow-hidden rounded-[1.2rem] border border-white/10 bg-bg/35">
            <table className="min-w-full text-sm">
              <thead className="bg-white/5 text-[0.68rem] uppercase tracking-[0.18em] text-text-secondary">
                <tr>
                  <th className="px-3 py-3 text-left font-semibold">Zeitraum</th>
                  <th className="px-3 py-3 text-left font-semibold">Status</th>
                  <th className="px-3 py-3 text-right font-semibold">Brutto</th>
                  <th className="px-3 py-3 text-right font-semibold">PDF</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/6">
                {latestDocuments.length ? (
                  latestDocuments.map((document) => (
                    <tr key={`${document.gutschriftNumber ?? document.id ?? document.periodLabel ?? 'doc'}`} className="hover:bg-white/[0.03]">
                      <td className="px-3 py-3 text-white">
                        <div className="font-medium">{document.periodLabel || 'Offen'}</div>
                        <div className="mt-1 text-xs uppercase tracking-[0.16em] text-text-secondary">
                          {document.gutschriftNumber || 'Noch keine Nummer'}
                        </div>
                      </td>
                      <td className="px-3 py-3">
                        <StatusBadge status={document.status} />
                      </td>
                      <td className="px-3 py-3 text-right text-white">
                        {formatCurrency(document.grossAmountCents)}
                      </td>
                      <td className="px-3 py-3 text-right">
                        {document.hasPdf && document.downloadPath ? (
                          <a
                            href={document.downloadPath}
                            target="_blank"
                            rel="noreferrer"
                            className="inline-flex items-center gap-1 text-sm font-medium text-primary transition hover:text-primary-hover"
                          >
                            <Download className="h-4 w-4" />
                            PDF
                          </a>
                        ) : (
                          <span className="text-xs uppercase tracking-[0.16em] text-text-secondary">Fehlt</span>
                        )}
                      </td>
                    </tr>
                  ))
                ) : (
                  <tr>
                    <td colSpan={4} className="px-3 py-8 text-center text-sm text-text-secondary">
                      {gutschriftenQuery.isLoading ? 'Gutschriften werden geladen …' : 'Noch keine Gutschriften vorhanden.'}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </section>

        <section className="mt-6 rounded-[1.5rem] border border-white/10 bg-bg/30 p-4">
          <div className="flex items-center gap-2 text-white">
            <Coins className="h-4 w-4 text-accent" />
            <h3 className="text-sm font-semibold uppercase tracking-[0.2em] text-text-secondary">Letzte Claims</h3>
          </div>

          <div className="mt-4 space-y-3">
            {latestClaims.length ? (
              latestClaims.map((claim) => (
                <div key={`${claim.id ?? claim.customerLogin}-${claim.claimedAt ?? 'claim'}`} className="rounded-[1.2rem] border border-white/10 bg-white/5 px-4 py-3">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <div className="font-medium text-white">{claim.customerLogin}</div>
                      <div className="mt-1 text-xs uppercase tracking-[0.16em] text-text-secondary">
                        {formatDateTime(claim.claimedAt)}
                      </div>
                    </div>
                    <div className="text-right">
                      <div className="font-medium text-white">{formatCurrency(claim.commissionCents)}</div>
                      <div className="mt-1 text-xs uppercase tracking-[0.16em] text-text-secondary">
                        {claim.commissionCount} Provisionen
                      </div>
                    </div>
                  </div>
                </div>
              ))
            ) : (
              <div className="rounded-[1.2rem] border border-dashed border-white/10 px-4 py-6 text-center text-sm text-text-secondary">
                Noch keine Claims sichtbar.
              </div>
            )}
          </div>
        </section>

        <section className="mt-6 grid gap-3 rounded-[1.5rem] border border-white/10 bg-bg/30 p-4">
          <div className="flex items-center gap-2 text-text-secondary">
            <Mail className="h-4 w-4" />
            <span className="text-sm text-white">{detail?.email || 'Keine E-Mail hinterlegt'}</span>
          </div>
          <div className="flex items-center gap-2 text-text-secondary">
            <CalendarDays className="h-4 w-4" />
            <span className="text-sm text-white">Mitglied seit {formatDateTime(detail?.createdAt)}</span>
          </div>
          <div className="flex items-center gap-2 text-text-secondary">
            <RefreshCw className="h-4 w-4" />
            <span className="text-sm text-white">Account aktualisiert {formatDateTime(detail?.updatedAt)}</span>
          </div>
          <div className="flex items-center gap-2 text-text-secondary">
            <FileText className="h-4 w-4" />
            <span className="text-sm text-white">Profil aktualisiert {formatDateTime(detail?.profileUpdatedAt)}</span>
          </div>
        </section>
      </aside>

      <ConfirmDialog
        open={confirmGenerate}
        title="Gutschrift für diesen Affiliate generieren?"
        description="Es werden alle fälligen Gutschriften für diesen Affiliate erzeugt und, falls möglich, direkt versendet."
        confirmLabel="Jetzt generieren"
        busy={generateMutation.isPending}
        onConfirm={() => {
          void handleGenerate();
        }}
        onCancel={() => setConfirmGenerate(false)}
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

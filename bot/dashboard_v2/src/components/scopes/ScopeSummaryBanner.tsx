import { useT } from '../../context/LanguageContext';
import { AlertTriangle, CheckCircle2, ChevronRight, ShieldQuestion } from 'lucide-react';
import { useScopes } from '@/hooks/useScopes';

interface ScopeSummaryBannerProps {
  onOpenTab?: () => void;
}

export function ScopeSummaryBanner({ onOpenTab }: ScopeSummaryBannerProps) {
  const t = useT();
  const { summary } = useScopes();
  const preview = summary.missing.slice(0, 3);

  return (
    <div className="mb-6 rounded-xl border border-border bg-gradient-to-r from-card to-background p-4 shadow-lg">
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="flex items-start gap-3">
          <div className="rounded-lg bg-accent/20 p-2 text-accent">
            <ShieldQuestion className="h-5 w-5" />
          </div>
          <div className="space-y-1">
            <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-text-secondary">
              <span>OAuth Scope Check</span>
              {summary.criticalMissing.length > 0 && (
                <span className="rounded-full bg-danger/10 px-2 py-0.5 text-[10px] font-semibold text-danger">
                  {t("kritisch")}
                </span>
              )}
            </div>
            <h3 className="text-lg font-semibold text-white">{t("Autorisation im Blick")}</h3>
            <p className="text-sm text-text-secondary">
              {t("Schnelltest für Partner: Welche Scopes fehlen dem aktuellen Token?")}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 rounded-lg border border-border bg-black/30 px-3 py-2">
            <div className="text-lg font-bold text-white">{summary.granted}</div>
            <div className="text-xs leading-tight text-text-secondary">
              <div>{t('von {total}', { total: summary.total })}</div>
              <div>{t("Scopes aktiv")}</div>
            </div>
          </div>

          <button
            type="button"
            onClick={onOpenTab}
            className="flex items-center gap-1 rounded-lg bg-accent px-3 py-2 text-sm font-semibold text-bg transition hover:bg-accent/80"
          >
            Details
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
        {summary.missing.length === 0 ? (
          <span className="flex items-center gap-1 rounded-full bg-success/10 px-3 py-1 text-success">
            <CheckCircle2 className="h-4 w-4" />
            {t("Alle Scopes sind autorisiert.")}
          </span>
        ) : (
          <>
            <span className="flex items-center gap-1 rounded-full bg-warning/10 px-3 py-1 text-warning">
              <AlertTriangle className="h-4 w-4" />
              {t('{count} fehlen', { count: summary.missing.length })}
            </span>
            {preview.map(scope => (
              <span
                key={scope.id}
                className="rounded-full bg-white/5 px-3 py-1 text-text-secondary"
                title={scope.id}
              >
                {t(scope.label)}
              </span>
            ))}
            {summary.missing.length > preview.length && (
              <span className="text-text-secondary">…</span>
            )}
          </>
        )}
      </div>
    </div>
  );
}

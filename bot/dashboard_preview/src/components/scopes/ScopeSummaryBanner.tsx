import { AlertTriangle, CheckCircle2, ChevronRight, ShieldQuestion } from 'lucide-react';
import { useScopes } from '@/hooks/useScopes';

interface ScopeSummaryBannerProps {
  onOpenTab?: () => void;
}

export function ScopeSummaryBanner({ onOpenTab }: ScopeSummaryBannerProps) {
  const { summary } = useScopes();
  const preview = summary.missing.slice(0, 3);

  return (
    <div className="mb-6 rounded-xl border border-border bg-gradient-to-r from-[#1b1f2b] to-[#131722] p-4 shadow-lg">
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
                  kritisch
                </span>
              )}
            </div>
            <h3 className="text-lg font-semibold text-white">Autorisation im Blick</h3>
            <p className="text-sm text-text-secondary">
              Schnelltest für Partner: Welche Scopes fehlen dem aktuellen Token?
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 rounded-lg border border-border bg-black/30 px-3 py-2">
            <div className="text-lg font-bold text-white">{summary.granted}</div>
            <div className="text-xs leading-tight text-text-secondary">
              <div>von {summary.total}</div>
              <div>Scopes aktiv</div>
            </div>
          </div>

          <button
            type="button"
            onClick={onOpenTab}
            className="flex items-center gap-1 rounded-lg bg-accent px-3 py-2 text-sm font-semibold text-white transition hover:bg-accent/80"
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
            Alle Scopes sind autorisiert.
          </span>
        ) : (
          <>
            <span className="flex items-center gap-1 rounded-full bg-warning/10 px-3 py-1 text-warning">
              <AlertTriangle className="h-4 w-4" />
              {summary.missing.length} fehlen
            </span>
            {preview.map(scope => (
              <span
                key={scope.id}
                className="rounded-full bg-white/5 px-3 py-1 text-text-secondary"
                title={scope.id}
              >
                {scope.label}
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

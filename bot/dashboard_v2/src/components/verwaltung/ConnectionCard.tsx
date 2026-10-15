import { useLanguage, useT } from '../../context/LanguageContext';
import { type ReactNode } from 'react';

export type ConnectionTone = 'success' | 'warning' | 'error' | 'muted';

const TONE_CLASS: Record<ConnectionTone, string> = {
  success: 'text-success',
  warning: 'text-warning',
  error: 'text-error',
  muted: 'text-text-secondary',
};

export interface ConnectionCardProps {
  icon: ReactNode;
  title: string;
  status: string;
  tone: ConnectionTone;
  sentence: string;
  footer: ReactNode;
  children?: ReactNode;
  tourId?: string;
}

export function ConnectionCard({ icon, title, status, tone, sentence, footer, children, tourId }: ConnectionCardProps) {
  return (
    <section
      tabIndex={tourId ? -1 : undefined}
      data-tour-id={tourId}
      data-tour-ready={tourId ? 'true' : undefined}
      className="panel-card flex h-full flex-col rounded-2xl p-5"
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="flex min-w-0 items-center gap-2 text-base font-bold text-white">
          {icon}
          {title}
        </h2>
        <span className={`shrink-0 text-xs font-semibold ${TONE_CLASS[tone]}`}>● {status}</span>
      </div>
      <p className="mt-2 text-sm text-text-secondary">{sentence}</p>
      {children}
      <div className="mt-auto pt-4">{footer}</div>
    </section>
  );
}

function ScopeChips({ scopes, tone }: { scopes: string[]; tone: 'success' | 'error' }) {
  const chip = tone === 'success' ? 'border-success/30 bg-success/10 text-success' : 'border-error/40 bg-error/10 text-error';
  return (
    <div className="flex flex-wrap gap-1.5">
      {scopes.map(scope => (
        <span key={scope} className={`rounded-full border px-2.5 py-0.5 text-[11px] font-medium ${chip}`}>{scope}</span>
      ))}
    </div>
  );
}

export function ScopeDisclosure({ granted, missing }: { granted: string[]; missing: string[] }) {
  const t = useT();
  const { locale } = useLanguage();
  if (!granted.length && !missing.length) return null;
  return (
    <details className="group mt-3">
      <summary className="cursor-pointer list-none text-xs font-semibold text-text-secondary hover:text-white">
        {t('{count} Berechtigungen aktiv', { count: granted.length.toLocaleString(locale) })} <span className="inline-block transition-transform group-open:rotate-180">▾</span>
      </summary>
      <div className="mt-2 space-y-2">
        {granted.length > 0 && <ScopeChips scopes={granted} tone="success" />}
        {missing.length > 0 && (
          <div>
            <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-text-secondary">{t('Fehlende Scopes ({count})', { count: missing.length.toLocaleString(locale) })}</p>
            <ScopeChips scopes={missing} tone="error" />
          </div>
        )}
      </div>
    </details>
  );
}

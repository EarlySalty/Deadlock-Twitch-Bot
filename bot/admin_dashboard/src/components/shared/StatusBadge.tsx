interface StatusBadgeProps {
  status?: string | boolean | null;
}

const STATUS_STYLES: Record<string, string> = {
  live: 'border-red-400/35 bg-red-500/15 text-red-100',
  active: 'border-emerald-400/35 bg-emerald-500/15 text-emerald-100',
  verified: 'border-sky-400/35 bg-sky-500/15 text-sky-100',
  archived: 'border-slate-400/35 bg-slate-500/15 text-slate-100',
  departnered: 'border-rose-400/35 bg-rose-500/15 text-rose-100',
  non_partner: 'border-amber-400/35 bg-amber-500/15 text-amber-100',
  offline: 'border-slate-500/40 bg-slate-700/30 text-slate-200',
  inactive: 'border-slate-500/40 bg-slate-800/45 text-slate-200',
  warning: 'border-amber-400/35 bg-amber-500/15 text-amber-100',
  error: 'border-red-400/35 bg-red-500/15 text-red-100',
  connected: 'border-emerald-400/35 bg-emerald-500/15 text-emerald-100',
  partial: 'border-amber-400/35 bg-amber-500/15 text-amber-100',
  reauth: 'border-rose-400/35 bg-rose-500/15 text-rose-100',
  trialing: 'border-violet-400/35 bg-violet-500/15 text-violet-100',
  past_due: 'border-amber-400/35 bg-amber-500/15 text-amber-100',
  emailed: 'border-emerald-400/35 bg-emerald-500/15 text-emerald-100',
  generated: 'border-cyan-400/35 bg-cyan-500/15 text-cyan-100',
  blocked: 'border-amber-400/35 bg-amber-500/15 text-amber-100',
  email_failed: 'border-rose-400/35 bg-rose-500/15 text-rose-100',
  kleinunternehmer: 'border-teal-400/35 bg-teal-500/15 text-teal-100',
  regelbesteuert: 'border-orange-400/35 bg-orange-500/15 text-orange-100',
  unknown: 'border-slate-500/40 bg-slate-700/30 text-slate-200',
};

export function StatusBadge({ status }: StatusBadgeProps) {
  const normalized =
    typeof status === 'boolean'
      ? status
        ? 'verified'
        : 'offline'
      : String(status || 'offline').trim().toLowerCase();

  return (
    <span
      className={[
        'inline-flex items-center rounded-full border px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em]',
        STATUS_STYLES[normalized] || 'border-white/10 bg-white/5 text-white/70',
      ].join(' ')}
    >
      {normalized.replace('_', ' ')}
    </span>
  );
}

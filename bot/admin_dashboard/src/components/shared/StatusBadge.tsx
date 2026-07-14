interface StatusBadgeProps {
  status?: string | boolean | null;
}

/*
 * Industrial Gold: Status ist Material, nicht Regenbogen.
 * Vier Zustaende, vier Toene — Plasma leuchtet nur, wenn wirklich etwas laeuft.
 *   live/verbunden → Plasma-Gruen (das Leuchten IST der Zustand)
 *   aktiv/geprueft → Plasma-Blau
 *   Achtung       → Schmiedefeuer (Ember)
 *   Defekt        → heisses Eisen
 *   Ruhe/unbekannt→ kaltes Gusseisen
 */
const TONE = {
  plasma: 'border-success/40 bg-success/12 text-success shadow-[0_0_14px_-4px_rgba(0,255,136,0.55)]',
  blue: 'border-accent/40 bg-accent/12 text-accent',
  ember: 'border-warning/40 bg-warning/12 text-warning',
  iron: 'border-danger/40 bg-danger/12 text-danger',
  cold: 'border-border bg-card-hover/60 text-secondary',
  gold: 'border-primary/40 bg-primary/12 text-primary',
} as const;

const STATUS_STYLES: Record<string, string> = {
  // laeuft gerade — das Plasma leuchtet
  ok: TONE.plasma,
  active: TONE.plasma,
  connected: TONE.plasma,
  emailed: TONE.plasma,
  live: TONE.plasma,

  // bestaetigt / geprueft
  verified: TONE.blue,
  generated: TONE.blue,
  kleinunternehmer: TONE.blue,

  // Aufmerksamkeit noetig
  warning: TONE.ember,
  partial: TONE.ember,
  non_partner: TONE.ember,
  token_error: TONE.ember,
  past_due: TONE.ember,
  regelbesteuert: TONE.ember,
  'reauth-needed': TONE.ember,
  reauth_needed: TONE.ember,

  // kaputt / abgewiesen
  error: TONE.iron,
  departnered: TONE.iron,
  reauth: TONE.iron,
  email_failed: TONE.iron,
  blocked: TONE.iron,

  // zahlend, aber auf Probe
  trialing: TONE.gold,

  // kalt
  idle: TONE.cold,
  offline: TONE.cold,
  inactive: TONE.cold,
  archived: TONE.cold,
  unknown: TONE.cold,
};

/* Nur echte Laufzustaende pulsieren — sonst waere der Puls Deko statt Signal. */
const PULSING = new Set(['live', 'connected', 'active', 'ok']);

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
        'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em]',
        STATUS_STYLES[normalized] || TONE.cold,
      ].join(' ')}
    >
      <span
        className={['plasma-dot', PULSING.has(normalized) ? 'plasma-dot-live' : ''].join(' ')}
        style={{ background: 'currentColor', boxShadow: '0 0 10px currentColor' }}
        aria-hidden="true"
      />
      {normalized.replace(/[_-]+/g, ' ')}
    </span>
  );
}

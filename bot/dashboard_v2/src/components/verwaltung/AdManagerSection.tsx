import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import {
  AlertTriangle,
  Clock3,
  ExternalLink,
  FlaskConical,
  Gamepad2,
  Loader2,
  PauseCircle,
  Play,
  Radio,
  RefreshCw,
  Save,
  ShieldCheck,
  Timer,
  Zap,
} from 'lucide-react';
import {
  AD_DURATION_OPTIONS,
  adManagerReauthUrl,
  adManagerSettingsInput,
  fetchAdManager,
  normalizeAdManagerSettings,
  queueAdManagerAction,
  saveAdManagerSettings,
  type AdManagerResponse,
  type AdManagerSettingsInput,
  type AdManagerSteamStatus,
  type AdManagerStrategy,
} from '@/api/adManager';
import { ApiHttpError } from '@/api/httpError';

interface AdManagerSectionProps {
  reconnectUrl: string;
}

const STRATEGIES: Array<{
  id: AdManagerStrategy;
  label: string;
  description: string;
  badge?: string;
  emphasis?: 'passive' | 'recommended';
  points?: string[];
}> = [
  {
    id: 'monitor',
    label: 'Nur überwachen',
    description: 'Der Bot liest den Twitch-Werbeplan, greift aber nicht ein.',
    points: ['Keine Twitch-Pausen', 'Startet keine Werbung'],
  },
  {
    id: 'snooze',
    label: 'Nur Twitch-Pausen nutzen',
    badge: 'Passiv',
    emphasis: 'passive',
    description: 'Wenn Werbung fällig wird, nutzt der Bot verfügbare Twitch-Snoozes.',
    points: ['Verschiebt fällige Werbung', 'Startet selbst keine Werbung'],
  },
  {
    id: 'smart',
    label: 'Match schützen & Queue nutzen',
    badge: 'Empfohlen',
    emphasis: 'recommended',
    description: 'Der Bot sucht aktiv den ruhigsten Moment für fällige Werbung.',
    points: [
      'Im Match → Werbung verschieben',
      'Queue oder Menü → Werbung starten',
      'Ohne Steam-Status → ruhige Chat-Phase nutzen',
    ],
  },
];

function steamStatusView(steam: AdManagerSteamStatus): {
  value: string;
  hint: string;
  tone: string;
} {
  if (!steam.linked) {
    return {
      value: 'Nicht verbunden',
      hint: 'Hinterlege deine SteamID64 im Tab Bot & Schutz bei der KI-Engagement.',
      tone: 'text-text-secondary',
    };
  }
  switch (steam.state) {
    case 'in_match':
      return {
        value: 'Im Match',
        hint: steam.hero
          ? `${steam.hero}. Werbung wird so lange wie möglich verschoben.`
          : 'Werbung wird so lange wie möglich verschoben.',
        tone: 'text-warning',
      };
    case 'in_queue':
      return {
        value: 'In der Queue',
        hint: 'Gutes Werbefenster: Fällt Werbung an, startet sie jetzt.',
        tone: 'text-success',
      };
    case 'out_of_game':
      return {
        value: 'Nicht im Spiel',
        hint: 'Werbung ist gerade unkritisch.',
        tone: 'text-text-secondary',
      };
    case 'stale':
      return {
        value: 'Status zu alt',
        hint: steam.observedAt
          ? `Letzter Stand: ${formatDateTime(steam.observedAt)}. Der Bot nutzt vorerst die Chat-Ruhe.`
          : 'Der Bot nutzt vorerst die Chat-Ruhe.',
        tone: 'text-warning',
      };
    default:
      return {
        value: 'Warten auf Status',
        hint: 'Sobald Steam-Daten ankommen, erscheint hier dein Match-Status.',
        tone: 'text-text-secondary',
      };
  }
}

function settingsEqual(a: AdManagerSettingsInput | null, b: AdManagerSettingsInput | null): boolean {
  return a !== null && b !== null && JSON.stringify(a) === JSON.stringify(b);
}

function formatDateTime(iso: string | null): string {
  if (!iso) return '–';
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString('de-DE', {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatRemaining(seconds: number | null): string {
  if (seconds === null) return '–';
  if (seconds < 60) return `${Math.max(0, seconds)} Sek.`;
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours > 0 ? `${hours} Std. ${rest} Min.` : `${minutes} Min.`;
}

function actionKindLabel(kind: string): string {
  if (kind === 'snooze') return 'Werbung pausiert';
  if (kind === 'commercial') return 'Werbung gestartet';
  return 'Werbeaktion';
}

function actionOutcomeLabel(outcome: string): string {
  if (outcome === 'succeeded') return 'Ausgeführt';
  if (outcome === 'failed') return 'Fehlgeschlagen';
  if (outcome === 'cancelled') return 'Abgebrochen';
  if (outcome === 'unknown') return 'Ausgang noch unklar';
  if (outcome === 'unresolved') return 'Nicht eindeutig geklärt';
  return 'Status unbekannt';
}

function formatNextAd(iso: string | null): { primary: string; secondary: string | null } {
  if (!iso) return { primary: 'Nicht geplant', secondary: null };
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return { primary: iso, secondary: null };
  const diffMinutes = Math.ceil((date.getTime() - Date.now()) / 60_000);
  return {
    primary: diffMinutes > 0 ? `in ${diffMinutes} Min.` : 'steht an',
    secondary: formatDateTime(iso),
  };
}

function StatusCard({
  icon,
  label,
  value,
  hint,
  tone = 'text-primary',
  className = '',
}: {
  icon: ReactNode;
  label: string;
  value: string;
  hint?: string | null;
  tone?: string;
  className?: string;
}) {
  return (
    <div className={`min-w-0 bg-background/60 px-3.5 py-3 ${className}`}>
      <div className={`mb-1.5 flex items-center gap-1.5 ${tone}`}>
        {icon}
        <span className="truncate text-[11px] font-semibold uppercase tracking-wider text-text-secondary">
          {label}
        </span>
      </div>
      <p className={`truncate text-base font-bold ${tone}`}>{value}</p>
      {hint ? <p className="mt-0.5 truncate text-[11px] text-text-secondary" title={hint}>{hint}</p> : null}
    </div>
  );
}

function NumberSetting({
  label,
  description,
  value,
  min,
  max,
  unit,
  disabled,
  onChange,
}: {
  label: string;
  description: string;
  value: number;
  min: number;
  max: number;
  unit: string;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  const inputId = useId();
  const descriptionId = `${inputId}-beschreibung`;
  const unitId = `${inputId}-einheit`;

  return (
    <div
      className={`rounded-lg border border-border bg-background/50 px-3 py-2.5 ${disabled ? 'opacity-50' : ''}`}
      title={description}
    >
      <label htmlFor={inputId} className="block text-[13px] font-semibold text-white">{label}</label>
      <p id={descriptionId} className="mt-0.5 truncate text-[10px] leading-4 text-text-secondary">{description}</p>
      <span className="mt-1.5 flex items-center gap-1.5">
        <input
          id={inputId}
          type="number"
          min={min}
          max={max}
          step={1}
          value={value}
          disabled={disabled}
          aria-describedby={`${descriptionId} ${unitId}`}
          onChange={(event) => onChange(Number(event.target.value))}
          className="min-h-10 min-w-0 flex-1 rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-sm font-semibold text-white outline-none transition-colors focus:border-primary disabled:cursor-not-allowed"
        />
        <span id={unitId} className="w-9 text-[11px] text-text-secondary">{unit}</span>
      </span>
    </div>
  );
}

export function AdManagerSection({ reconnectUrl }: AdManagerSectionProps) {
  const [data, setData] = useState<AdManagerResponse | null>(null);
  const [draft, setDraft] = useState<AdManagerSettingsInput | null>(null);
  const [baseline, setBaseline] = useState<AdManagerSettingsInput | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [actionPending, setActionPending] = useState<'snooze' | 'commercial' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const actionRetryKeys = useRef<Partial<Record<'snooze' | 'commercial', string>>>({});

  useEffect(() => {
    let active = true;
    let refreshing = false;
    let controller: AbortController | null = null;

    const load = async (initial: boolean) => {
      if (refreshing) return;
      refreshing = true;
      const requestController = new AbortController();
      controller = requestController;
      let timedOut = false;
      const timeout = window.setTimeout(() => {
        timedOut = true;
        requestController.abort();
      }, 10_000);
      if (initial) {
        setLoading(true);
        setError(null);
      }
      try {
        const loaded = await fetchAdManager(requestController.signal);
        if (!active) return;
        setData(loaded);
        setRefreshError(null);
        if (initial) {
          const settings = adManagerSettingsInput(loaded.settings);
          setDraft(settings);
          setBaseline(settings);
        }
      } catch (loadError) {
        if (!active) return;
        const message = timedOut
          ? 'Die Anfrage hat nach 10 Sekunden nicht geantwortet.'
          : loadError instanceof Error
            ? loadError.message
            : 'Werbemanager konnte nicht geladen werden.';
        if (initial) {
          setError(message);
        } else {
          setRefreshError(`Live-Status konnte nicht aktualisiert werden: ${message}`);
        }
      } finally {
        window.clearTimeout(timeout);
        refreshing = false;
        if (controller === requestController) controller = null;
        if (active) setLoading(false);
      }
    };

    void load(true);
    const refresh = window.setInterval(() => {
      void load(false);
    }, 30_000);

    return () => {
      active = false;
      controller?.abort();
      window.clearInterval(refresh);
    };
  }, []);

  const needsInitialSave = data?.settings.updatedAt === null;
  const dirty = useMemo(
    () => needsInitialSave || !settingsEqual(draft, baseline),
    [draft, baseline, needsInitialSave],
  );
  const patch = (next: Partial<AdManagerSettingsInput>) => {
    setNotice(null);
    setDraft((current) => current ? { ...current, ...next } : current);
  };

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const normalized = normalizeAdManagerSettings(draft);
      const saved = await saveAdManagerSettings(normalized);
      const settings = adManagerSettingsInput(saved.settings);
      setData(saved);
      setDraft(settings);
      setBaseline(settings);
      setNotice('Einstellungen gespeichert.');
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : 'Speichern fehlgeschlagen.');
    } finally {
      setSaving(false);
    }
  };

  const queueAction = async (action: 'snooze' | 'commercial') => {
    if (!draft) return;
    setActionPending(action);
    setError(null);
    setNotice(null);
    const idempotencyKey = actionRetryKeys.current[action] ?? crypto.randomUUID();
    actionRetryKeys.current[action] = idempotencyKey;
    try {
      const result = await queueAdManagerAction(
        action === 'snooze'
          ? { action: 'snooze' }
          : { action: 'commercial', durationSeconds: draft.adDurationSeconds },
        idempotencyKey,
      );
      if (!result.queued) {
        delete actionRetryKeys.current[action];
        throw new Error('Twitch-Aktion wurde nicht eingeplant.');
      }
      delete actionRetryKeys.current[action];
      setNotice(
        action === 'snooze'
          ? 'Anfrage zum Pausieren eingereiht. Der Bot prüft vor der Ausführung Streamstatus und Twitch-Zugriff.'
          : `Anfrage für ${draft.adDurationSeconds} Sekunden Werbung eingereiht. Der Bot prüft vor der Ausführung Streamstatus und Twitch-Zugriff.`,
      );
    } catch (actionError) {
      if (actionError instanceof ApiHttpError) {
        delete actionRetryKeys.current[action];
      }
      setError(actionError instanceof Error ? actionError.message : 'Twitch-Aktion fehlgeschlagen.');
    } finally {
      setActionPending(null);
    }
  };

  if (loading) {
    return (
      <section className="panel-card rounded-2xl p-6">
        <div className="flex items-center gap-3 text-sm text-text-secondary">
          <Loader2 className="h-5 w-5 animate-spin text-primary" />
          Werbemanager wird geladen ...
        </div>
      </section>
    );
  }

  if (!data || !draft) {
    const reauthUrl = adManagerReauthUrl(reconnectUrl);
    return (
      <section className="panel-card rounded-2xl p-6">
        <div role="alert" className="rounded-xl border border-danger/40 bg-danger/10 p-4 text-sm text-danger">
          <p>{error ?? 'Der Werbemanager ist gerade nicht verfügbar.'}</p>
          <a
            href={reauthUrl}
            className="mt-3 inline-flex items-center gap-2 rounded-lg border border-danger/40 px-4 py-2 font-semibold transition-colors hover:bg-danger/10"
          >
            Twitch neu verbinden <ExternalLink className="h-4 w-4" />
          </a>
        </div>
      </section>
    );
  }

  const { status } = data;
  const nextAd = formatNextAd(status.nextAdAt);
  const missingScopeLabels = [
    !status.scopes.read ? 'Werbeplan lesen' : null,
    !status.scopes.snooze ? 'Werbung pausieren' : null,
    !status.scopes.commercial ? 'Werbung starten' : null,
  ].filter((label): label is string => label !== null);
  const needsReauth = missingScopeLabels.length > 0;
  const canSnooze = status.isLive && status.scopes.snooze && (status.snoozeCount ?? 0) > 0;
  const canRunCommercial = status.isLive && status.scopes.commercial;
  const smartFieldsDisabled = draft.strategy !== 'smart';
  const reauthUrl = adManagerReauthUrl(reconnectUrl);

  return (
    <motion.section
      tabIndex={-1}
      data-tour-id="onboarding-advertising"
      data-tour-ready="true"
      data-unsaved={!settingsEqual(draft, baseline)}
      className="panel-card rounded-2xl p-4 md:p-5"
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.32 }}
    >
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="mb-1 flex items-center gap-2 text-sm font-medium uppercase tracking-wider text-primary">
            <FlaskConical className="h-4 w-4" /> Experimentell
          </p>
          <h2 className="display-font text-2xl font-bold text-white">Intelligenter Werbemanager</h2>
          <p className="mt-1 max-w-2xl text-sm text-text-secondary">
            Du entscheidest, wie stark der Bot eingreift: nur beobachten, nur Twitch-Snoozes nutzen
            oder Matches schützen und Queue oder Menü aktiv als Werbefenster verwenden.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <span className={`rounded-full border px-2.5 py-1 text-[11px] font-semibold ${
            draft.enabled && status.workerHealthy
              ? 'border-success/40 bg-success/10 text-success'
              : draft.enabled
                ? 'border-danger/40 bg-danger/10 text-danger'
                : 'border-border bg-background/50 text-text-secondary'
          }`}>
            {draft.enabled
              ? status.workerHealthy ? 'Bot läuft' : 'Bot nicht erreichbar'
              : 'Bot aus'}
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={draft.enabled}
            aria-label="Werbemanager aktiv"
            title={draft.enabled ? 'Werbemanager ausschalten' : 'Werbemanager einschalten'}
            onClick={() => patch({ enabled: !draft.enabled })}
            className={`relative inline-flex h-8 w-11 shrink-0 items-center rounded-full transition-colors ${
              draft.enabled ? 'bg-primary' : 'bg-border'
            }`}
          >
            <span className={`inline-block h-4 w-4 rounded-full bg-white transition-transform ${
              draft.enabled ? 'translate-x-6' : 'translate-x-1'
            }`} />
          </button>
        </div>
      </div>

      {data.settings.enabled && !status.workerHealthy ? (
        <div role="alert" className="mb-4 rounded-xl border border-danger/40 bg-danger/10 p-3 text-sm text-danger">
          <p className="font-semibold">Die Automatik meldet sich gerade nicht.</p>
          <p className="mt-1 text-xs text-text-secondary">
            Bis der Bot wieder arbeitet, werden keine Werbepausen oder intelligenten Werbungen ausgelöst.
            {status.workerHeartbeatAt ? ` Letztes Lebenszeichen: ${formatDateTime(status.workerHeartbeatAt)}.` : ''}
          </p>
        </div>
      ) : null}

      <div className="mb-3 flex items-center gap-2 rounded-lg border border-warning/35 bg-warning/10 px-3 py-2 text-xs text-text-secondary">
        <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />
        <p>
          <span className="font-semibold text-warning">Twitch kann Werbefreiheit nicht garantieren.</span>{' '}
          Snoozes verschieben die nächste Werbung nur um fünf Minuten und sind begrenzt verfügbar.
        </p>
      </div>

      {error ? (
        <div role="alert" className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      ) : null}
      {refreshError ? (
        <div role="alert" className="mb-4 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning">
          {refreshError}
        </div>
      ) : null}
      {notice ? (
        <div role="status" className="mb-4 rounded-lg border border-accent/40 bg-accent/10 px-3 py-2 text-sm text-accent">
          {notice}
        </div>
      ) : null}

      {needsReauth ? (
        <div className="mb-4 rounded-xl border border-warning/40 bg-warning/10 p-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="flex items-center gap-2 text-sm font-semibold text-warning">
                <ShieldCheck className="h-4 w-4" /> Twitch-Berechtigungen fehlen
              </p>
              <p className="mt-1 text-xs text-text-secondary">
                Fehlend: {missingScopeLabels.join(', ')}. Verbinde Twitch erneut, damit Status, Automatik und Handaktionen vollständig funktionieren.
              </p>
              <p className="mt-1 text-xs text-text-secondary">
                Twitch bestätigt dabei beide Werbe-Schreibrechte: Pausen verschieben und Werbung starten. Der Bot nutzt nur die Aktionen, die deine gewählte Strategie erlaubt.
              </p>
            </div>
            <a
              href={reauthUrl}
              className="inline-flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 px-4 py-2 text-sm font-semibold text-warning transition-colors hover:bg-warning/20"
            >
              Twitch neu verbinden <ExternalLink className="h-4 w-4" />
            </a>
          </div>
        </div>
      ) : null}

      <div className="mb-3">
        <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
          {status.isLive ? 'Stream läuft.' : 'Stream ist offline.'}{' '}
          {status.nextAdAt ? `Nächste Werbung: ${formatDateTime(status.nextAdAt)}.` : 'Keine nächste Werbung gemeldet.'}{' '}
          {status.snoozeCount === null ? 'Verfügbare Pausen unbekannt.' : `${status.snoozeCount} Pausen verfügbar.`}
        </p>
        <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-baseline gap-2">
            <h3 className="text-base font-bold text-white">Live-Status</h3>
            <span className="text-[11px] text-text-secondary">Twitch: {formatDateTime(status.observedAt)}</span>
          </div>
          <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold ${
            status.isLive ? 'bg-error/10 text-error' : 'bg-background/60 text-text-secondary'
          }`}>
            <span className={`h-1.5 w-1.5 rounded-full ${status.isLive ? 'animate-pulse bg-error' : 'bg-text-secondary'}`} />
            {status.isLive ? 'Stream läuft' : 'Offline'}
          </span>
        </div>
        {(() => {
          const steamView = steamStatusView(status.steam);
          return (
            <div className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-border bg-border lg:grid-cols-5">
              <StatusCard
                icon={<Clock3 className="h-3.5 w-3.5" />}
                label="Nächste Werbung"
                value={nextAd.primary}
                hint={nextAd.secondary}
              />
              <StatusCard
                icon={<PauseCircle className="h-3.5 w-3.5" />}
                label="Pausen"
                value={status.snoozeCount === null ? '–' : `${status.snoozeCount}`}
                hint={status.snoozeRefreshAt ? `Neue ab ${formatDateTime(status.snoozeRefreshAt)}` : null}
                tone={(status.snoozeCount ?? 0) > 0 ? 'text-success' : 'text-warning'}
              />
              <StatusCard
                icon={<ShieldCheck className="h-3.5 w-3.5" />}
                label="Preroll-frei"
                value={formatRemaining(status.prerollFreeSeconds)}
                hint="verbleibend"
                tone="text-success"
              />
              <StatusCard
                icon={<Timer className="h-3.5 w-3.5" />}
                label="Letzte Werbung"
                value={formatDateTime(status.lastAdAt)}
                hint={status.durationSeconds === null ? null : `${status.durationSeconds} Sekunden`}
                tone="text-text-secondary"
              />
              <StatusCard
                icon={<Gamepad2 className="h-3.5 w-3.5" />}
                label="Match-Status (Steam)"
                value={steamView.value}
                hint={steamView.hint}
                tone={steamView.tone}
                className="col-span-2 lg:col-span-1"
              />
            </div>
          );
        })()}
      </div>

      {status.lastAction ? (
        <div className="mb-4 rounded-xl border border-border bg-background/50 px-3 py-2.5 text-sm">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="font-semibold text-white">Letzte Bot-Aktion: {actionKindLabel(status.lastAction.kind)}</span>
            <span className="text-xs text-text-secondary">{formatDateTime(status.lastAction.at)}</span>
          </div>
          <p className="mt-1 text-xs text-text-secondary">
            {actionOutcomeLabel(status.lastAction.outcome)}{status.lastAction.detail ? ` · ${status.lastAction.detail}` : ''}
          </p>
        </div>
      ) : null}

      <div className="mb-4">
        <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
          <h3 className="text-base font-bold text-white">Strategie</h3>
          <p className="text-[11px] text-text-secondary">Empfohlen: Matches schützen, Queue als Werbefenster nutzen.</p>
        </div>
        <div className="grid gap-2 md:grid-cols-3">
          {STRATEGIES.map((strategy) => {
            const selected = strategy.id === draft.strategy;
            const recommended = strategy.emphasis === 'recommended';
            return (
              <button
                key={strategy.id}
                type="button"
                aria-pressed={selected}
                title={strategy.description}
                onClick={() => patch({ strategy: strategy.id })}
                className={`rounded-xl border px-3.5 py-3 text-left transition-colors ${
                  selected
                    ? 'border-primary bg-primary/15 shadow-[inset_0_0_0_1px_rgba(197,160,89,0.18)]'
                    : recommended
                      ? 'border-primary/45 bg-primary/[0.06] hover:border-primary/70'
                      : 'border-border bg-background/50 hover:border-border-hover'
                }`}
              >
                <span className="flex items-center gap-2">
                  <span className={selected || recommended ? 'text-primary' : 'text-text-secondary'}>
                    {strategy.id === 'monitor' ? <Radio className="h-4 w-4" /> : strategy.id === 'snooze' ? <PauseCircle className="h-4 w-4" /> : <Zap className="h-4 w-4" />}
                  </span>
                  <span className={`text-sm font-bold ${selected || recommended ? 'text-primary' : 'text-white'}`}>
                    {strategy.label}
                  </span>
                  {strategy.badge ? (
                    <span className={`ml-auto rounded-full border px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider ${
                      recommended
                        ? 'border-primary/40 bg-primary/10 text-primary'
                        : 'border-border bg-card text-text-secondary'
                    }`}>
                      {strategy.badge}
                    </span>
                  ) : null}
                </span>
                <span className="mt-1 block text-[11px] leading-4 text-text-secondary">{strategy.description}</span>
                {strategy.points ? (
                  <span className="mt-2 flex flex-wrap gap-x-3 gap-y-1 border-t border-border/70 pt-2 text-[11px] leading-4 text-text-secondary">
                    {strategy.points.map((point) => (
                      <span key={point} className="inline-flex items-center gap-1.5">
                        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${recommended ? 'bg-primary' : 'bg-text-secondary/60'}`} />
                        {point}
                      </span>
                    ))}
                  </span>
                ) : null}
              </button>
            );
          })}
        </div>
      </div>

      <div className="mb-4">
        <h3 className="mb-2 text-base font-bold text-white">Vorgaben</h3>
        <div className="rounded-xl border border-border bg-background/50 px-3 py-2.5">
          <div className="flex items-center justify-between gap-2">
            <p className="text-[13px] font-semibold text-white">Werbedauer</p>
            <span className="text-[10px] text-text-secondary">Für Smart-Steuerung und manuelle Werbung</span>
          </div>
          <div className="mt-2 grid grid-cols-3 gap-1.5 sm:grid-cols-6">
            {AD_DURATION_OPTIONS.map((seconds) => (
              <button
                key={seconds}
                type="button"
                aria-pressed={draft.adDurationSeconds === seconds}
                onClick={() => patch({ adDurationSeconds: seconds })}
                className={`min-h-9 rounded-md border px-2 py-1.5 text-xs font-semibold transition-colors ${
                  draft.adDurationSeconds === seconds
                    ? 'border-primary bg-primary/15 text-primary'
                    : 'border-border bg-card text-text-secondary hover:text-white'
                }`}
              >
                {seconds} Sek.
              </button>
            ))}
          </div>
        </div>
        <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
          <NumberSetting
            label="Mindestabstand"
            description="So viele Minuten liegen mindestens zwischen zwei Werbungen – auch wenn Twitch die vorige automatisch gestartet hat."
            value={draft.minIntervalMinutes}
            min={8}
            max={180}
            unit="Min."
            disabled={smartFieldsDisabled}
            onChange={(value) => patch({ minIntervalMinutes: value })}
          />
          <NumberSetting
            label="Startschutz"
            description="Nach Streamstart startet der Bot in diesem Zeitraum keine Werbung."
            value={draft.startupDelayMinutes}
            min={0}
            max={180}
            unit="Min."
            disabled={smartFieldsDisabled}
            onChange={(value) => patch({ startupDelayMinutes: value })}
          />
          <NumberSetting
            label="Chat-Ruhe"
            description="So lange muss ungefähr keine neue Chat-Nachricht kommen. Gilt, wenn dein Steam-Status gerade nicht frisch ist."
            value={draft.quietWindowMinutes}
            min={0}
            max={60}
            unit="Min."
            disabled={smartFieldsDisabled}
            onChange={(value) => patch({ quietWindowMinutes: value })}
          />
          <NumberSetting
            label="Vorlauf"
            description="So früh vor der nächsten geplanten Werbung entscheidet der Bot über Pausieren oder Starten."
            value={draft.actionLeadSeconds}
            min={10}
            max={300}
            unit="Sek."
            disabled={draft.strategy === 'monitor'}
            onChange={(value) => patch({ actionLeadSeconds: value })}
          />
        </div>
        {smartFieldsDisabled ? (
          <p className="mt-1.5 text-[11px] text-text-secondary">
            Abstand, Startschutz und Chat-Ruhe gelten nur für „Match schützen & Queue nutzen“.
          </p>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-3.5">
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={!dirty || saving}
            onClick={() => void save()}
            className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-3.5 py-2 text-sm font-semibold text-primary transition-colors hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
            Speichern
          </button>
          {dirty && !saving ? (
            <span className="text-[11px] text-text-secondary">
              {needsInitialSave ? 'Noch nicht eingerichtet.' : 'Ungespeicherte Änderungen.'}
            </span>
          ) : null}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span className="mr-1 text-[11px] font-semibold uppercase tracking-wider text-text-secondary">Manuell</span>
          <button
            type="button"
            disabled={!canSnooze || actionPending !== null}
            onClick={() => void queueAction('snooze')}
            className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-3 py-2 text-sm font-semibold text-accent transition-colors hover:bg-accent/20 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {actionPending === 'snooze' ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
            5 Min. pausieren
          </button>
          <button
            type="button"
            disabled={!canRunCommercial || actionPending !== null}
            onClick={() => void queueAction('commercial')}
            className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-3 py-2 text-sm font-semibold text-primary transition-colors hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {actionPending === 'commercial' ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
            {draft.adDurationSeconds} Sek. Werbung
          </button>
        </div>
        {!status.isLive ? <p className="basis-full text-[11px] text-text-secondary">Manuelle Aktionen sind verfügbar, sobald Twitch deinen Stream als live meldet.</p> : null}
        {status.isLive && !status.workerHealthy ? (
          <p className="basis-full text-[11px] text-warning">Der Bot ist gerade nicht erreichbar; Anfragen warten auf seine Rückkehr.</p>
        ) : null}
      </div>
    </motion.section>
  );
}

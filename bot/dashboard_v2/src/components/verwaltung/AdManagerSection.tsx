import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import {
  AlertTriangle,
  Check,
  Clock3,
  ExternalLink,
  Gamepad2,
  History,
  Info,
  Loader2,
  PauseCircle,
  Play,
  RefreshCw,
  Save,
  ShieldCheck,
  SlidersHorizontal,
  Zap,
} from 'lucide-react';
import {
  AD_DURATION_OPTIONS,
  BUDGET_MINUTES_MAX,
  BUDGET_MINUTES_MIN,
  adManagerReauthUrl,
  adManagerSettingsInput,
  fetchAdManager,
  fetchAdManagerHistory,
  normalizeAdManagerSettings,
  queueAdManagerAction,
  saveAdManagerSettings,
  type AdManagerHistory,
  type AdManagerResponse,
  type AdManagerSettingsInput,
  type AdManagerSteamStatus,
  type AdManagerStrategy,
} from '@/api/adManager';
import { ApiHttpError } from '@/api/httpError';
import { beschreibeEintrag, budgetVorschau, statusSatz } from './adManagerTexte';

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
    id: 'snooze',
    label: 'Nur Twitch-Pausen nutzen',
    badge: 'Passiv',
    emphasis: 'passive',
    description: 'Verschiebt fällige Werbung, wenn Twitch eine Pause anbietet.',
    points: ['Twitch-Pausen nutzen', 'Keine Werbung durch den Bot'],
  },
  {
    id: 'smart',
    label: 'Match schützen & Queue nutzen',
    badge: 'Empfohlen',
    emphasis: 'recommended',
    description: 'Schützt Matches und nutzt Queue oder Menü als Werbefenster.',
    points: [
      'Im Match: Werbung verschieben',
      'Queue oder Menü: Werbung starten',
      'Ohne Steam: ruhige Chat-Phase nutzen',
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
      hint: 'Hinterlege deine SteamID64 unter Bot & Schutz.',
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

function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
}

function formatRelativeMinutes(iso: string | null): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  const diff = Math.ceil((date.getTime() - Date.now()) / 60_000);
  if (diff <= 0) return 'jetzt';
  if (diff < 60) return `in ${diff} Min.`;
  const hours = Math.floor(diff / 60);
  const rest = diff % 60;
  return rest > 0 ? `in ${hours} Std. ${rest} Min.` : `in ${hours} Std.`;
}

function formatNextAd(iso: string | null): string {
  if (!iso) return 'Nicht geplant';
  return formatRelativeMinutes(iso) ?? formatDateTime(iso);
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

function InfoHint({ label, text }: { label: string; text: string }) {
  return (
    <span className="group relative inline-flex shrink-0">
      <button
        type="button"
        aria-label={label}
        className="inline-flex h-5 w-5 items-center justify-center rounded-full text-text-secondary transition-colors hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50"
      >
        <Info className="h-3.5 w-3.5" />
      </button>
      <span
        role="tooltip"
        className="pointer-events-none absolute left-1/2 top-full z-20 mt-2 hidden w-64 -translate-x-1/2 rounded-lg border border-border bg-card px-3 py-2 text-xs font-normal leading-5 text-text-secondary shadow-lg group-hover:block group-focus-within:block"
      >
        {text}
      </span>
    </span>
  );
}

function StatusMetric({
  icon,
  label,
  value,
  hint,
  tone = 'text-white',
}: {
  icon: ReactNode;
  label: string;
  value: string;
  hint?: string | null;
  tone?: string;
}) {
  return (
    <div className="min-w-0 rounded-xl border border-border bg-background/45 p-3" title={hint ?? undefined}>
      <div className="flex items-start justify-between gap-3">
        <span className={`shrink-0 ${tone}`}>{icon}</span>
        <span className={`min-w-0 text-right text-base font-bold leading-tight ${tone}`}>{value}</span>
      </div>
      <span className="mt-2 block text-[10px] font-semibold uppercase tracking-wider text-text-secondary">{label}</span>
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
    <div className={`min-w-0 ${disabled ? 'opacity-50' : ''}`}>
      <div className="flex items-center gap-1.5">
        <label htmlFor={inputId} className="text-[13px] font-semibold text-white">{label}</label>
        <InfoHint label={`${label} erklären`} text={description} />
      </div>
      <span id={descriptionId} className="sr-only">{description}</span>
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
  const [history, setHistory] = useState<AdManagerHistory | null>(null);
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
        try {
          const loadedHistory = await fetchAdManagerHistory(requestController.signal);
          if (active) setHistory(loadedHistory);
        } catch {
          if (active && initial) setHistory(null);
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
  const plan = status.plan ?? null;
  const budgetSource = plan?.source ?? 'own';
  const nextBlockLabel = formatRelativeMinutes(plan?.nextBlockAt ?? null);
  const steamView = steamStatusView(status.steam);
  const missingScopeLabels = [
    !status.scopes.read ? 'Werbeplan lesen' : null,
    !status.scopes.snooze ? 'Werbung pausieren' : null,
    !status.scopes.commercial ? 'Werbung starten' : null,
  ].filter((label): label is string => label !== null);
  const needsReauth = missingScopeLabels.length > 0;
  const canSnooze = status.isLive && status.scopes.snooze && (status.snoozeCount ?? 0) > 0;
  const canRunCommercial = status.isLive && status.scopes.commercial;
  const smartFieldsDisabled = draft.strategy !== 'smart';
  const chatNoticeOn = draft.chatNoticeBeforeAd !== false;
  const reauthUrl = adManagerReauthUrl(reconnectUrl);
  const laufText = statusSatz({
    enabled: draft.enabled,
    isLive: status.isLive,
    currentReason: status.currentReason,
    nextBlockLabel,
    fit: plan?.fit ?? null,
  });
  const historyEntries = history?.entries ?? [];
  const summary = history?.summary ?? null;

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
      <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="display-font text-2xl font-bold text-white">Intelligenter Werbemanager</h2>
        <div className="flex flex-wrap items-center gap-2">
          <span
            className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold ${
              draft.enabled ? 'bg-primary/10 text-primary' : 'bg-background/60 text-text-secondary'
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                draft.enabled && status.workerHealthy ? 'animate-pulse bg-primary' : 'bg-text-secondary'
              }`}
            />
            {draft.enabled ? 'Aktiv' : 'Aus'}
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={draft.enabled}
            aria-label="Werbemanager aktiv"
            title={draft.enabled ? 'Werbemanager ausschalten' : 'Werbemanager einschalten'}
            onClick={() => patch({ enabled: !draft.enabled })}
            className={`relative inline-flex h-8 w-14 shrink-0 items-center rounded-full border-2 transition-colors ${
              draft.enabled ? 'border-primary bg-primary/25' : 'border-border bg-background'
            }`}
          >
            <span
              className={`inline-block h-5 w-5 rounded-full transition-transform ${
                draft.enabled ? 'translate-x-7 bg-primary' : 'translate-x-1 bg-text-secondary'
              }`}
            />
          </button>
        </div>
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
                Fehlend: {missingScopeLabels.join(', ')}.
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

      <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {laufText}
      </p>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,13fr)_minmax(20rem,7fr)]">
        <div className="min-w-0 rounded-xl border border-border bg-background/35 p-4 md:p-5">
          <h3 className="text-lg font-bold text-white">Konfiguration</h3>

          <section className="mt-4">
            <h4 className="text-sm font-semibold text-white">Strategie</h4>
            <div className="mt-2 grid gap-2 sm:grid-cols-2" role="radiogroup" aria-label="Werbestrategie">
              {STRATEGIES.map((strategy) => {
                const selected = strategy.id === draft.strategy;
                const recommended = strategy.emphasis === 'recommended';
                const tooltip = strategy.points?.length
                  ? `${strategy.description} ${strategy.points.join('. ')}.`
                  : strategy.description;
                return (
                  <button
                    key={strategy.id}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    onClick={() => patch({ strategy: strategy.id })}
                    className={`min-w-0 rounded-xl border px-3 py-3 text-left transition-colors ${
                      selected
                        ? 'border-primary bg-primary/15'
                        : 'border-border bg-background/45 hover:border-border-hover'
                    }`}
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <span className={selected || recommended ? 'text-primary' : 'text-text-secondary'}>
                        {strategy.id === 'snooze' ? <PauseCircle className="h-4 w-4" /> : <Zap className="h-4 w-4" />}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-sm font-semibold text-white">{strategy.label}</span>
                      <span title={tooltip} aria-label={tooltip} className="shrink-0 text-text-secondary">
                        <Info className="h-3.5 w-3.5" />
                      </span>
                      {selected ? <Check className="h-4 w-4 shrink-0 text-primary" aria-hidden="true" /> : null}
                    </span>
                    {strategy.badge ? (
                      <span
                        className={`mt-2 inline-flex rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider ${
                          recommended
                            ? 'border-primary/40 bg-primary/10 text-primary'
                            : 'border-border bg-card text-text-secondary'
                        }`}
                      >
                        {strategy.badge}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </section>

          <section className="mt-5 border-t border-border pt-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex min-w-0 items-center gap-1.5">
                <h4 className="text-sm font-semibold text-white">Chat-Hinweis</h4>
                <InfoHint
                  label="Chat-Hinweis erklären"
                  text="Der Bot schreibt kurz vor einer Werbung eine Zeile in den Chat, wenn der Werbemanager aktiv ist."
                />
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={chatNoticeOn}
                aria-label="Chat vor Werbung informieren"
                title={chatNoticeOn ? 'Hinweis im Chat ausschalten' : 'Hinweis im Chat einschalten'}
                onClick={() => patch({ chatNoticeBeforeAd: !chatNoticeOn })}
                className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full border-2 transition-colors ${
                  chatNoticeOn ? 'border-primary bg-primary/25' : 'border-border bg-background'
                }`}
              >
                <span
                  className={`inline-block h-4 w-4 rounded-full transition-transform ${
                    chatNoticeOn ? 'translate-x-6 bg-primary' : 'translate-x-1 bg-text-secondary'
                  }`}
                />
              </button>
            </div>
          </section>

          <section className="mt-5 border-t border-border pt-4">
            <div className="flex items-center gap-1.5">
              <h4 className="text-sm font-semibold text-white">Budget</h4>
              <InfoHint
                label="Budget erklären"
                text="Der Werbemanager nutzt den Twitch-Werbeplan, wenn Twitch einen Plan liefert. Sonst gilt dein eigenes Stundenbudget."
              />
            </div>
            {budgetSource === 'twitch' ? (
              <div className="mt-2">
                <p className="text-sm font-semibold text-white">Twitch-Werbungs-Manager</p>
                <p className="mt-1 text-xs text-text-secondary">{budgetVorschau(draft.budgetMinutesPerHour, plan)}</p>
              </div>
            ) : (
              <div className="mt-2 flex flex-wrap items-end gap-3">
                <label htmlFor="budget-minutes" className="block">
                  <span className="block text-xs font-medium text-text-secondary">Minuten pro Stunde</span>
                  <input
                    id="budget-minutes"
                    type="number"
                    min={BUDGET_MINUTES_MIN}
                    max={BUDGET_MINUTES_MAX}
                    step={1}
                    value={draft.budgetMinutesPerHour}
                    onChange={(event) => patch({ budgetMinutesPerHour: Number(event.target.value) })}
                    className="mt-1 min-h-10 w-24 rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-sm font-semibold text-white outline-none transition-colors focus:border-primary"
                  />
                </label>
                <p className="pb-2 text-xs text-text-secondary">{budgetVorschau(draft.budgetMinutesPerHour, plan)}</p>
              </div>
            )}
          </section>

          <details className="group mt-5 border-t border-border pt-4">
            <summary className="flex cursor-pointer items-center gap-2 text-sm font-semibold text-primary">
              <SlidersHorizontal className="h-4 w-4" /> Feineinstellungen
            </summary>
            <div className="mt-4 grid gap-x-4 gap-y-4 sm:grid-cols-2">
              <NumberSetting
                label="Mindestabstand"
                description="Gilt für Twitch-Pausen und für Werbung, die du selbst startest."
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
                onChange={(value) => patch({ actionLeadSeconds: value })}
              />
            </div>
            {smartFieldsDisabled ? (
              <p className="mt-3 text-[11px] text-text-secondary">
                Abstand, Startschutz und Chat-Ruhe gelten bei „Match schützen & Queue nutzen“.
              </p>
            ) : null}
          </details>

          <div className="mt-5 flex flex-wrap items-center gap-2 border-t border-border pt-4">
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
        </div>

        <div className="min-w-0 space-y-4">
          <section className="rounded-xl border border-border bg-background/35 p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h3 className="text-lg font-bold text-white">Live-Status</h3>
                <p className="mt-1 text-xs leading-5 text-text-secondary">{laufText}</p>
              </div>
              <span
                className={`inline-flex shrink-0 items-center gap-1.5 rounded-full px-2 py-1 text-[10px] font-semibold ${
                  status.isLive ? 'bg-error/10 text-error' : 'bg-background/60 text-text-secondary'
                }`}
              >
                <span className={`h-1.5 w-1.5 rounded-full ${status.isLive ? 'animate-pulse bg-error' : 'bg-text-secondary'}`} />
                {status.isLive ? 'Stream läuft' : 'Offline'}
              </span>
            </div>

            {draft.enabled && !status.workerHealthy ? (
              <div className="mt-3 flex items-center gap-2 text-xs text-danger">
                <AlertTriangle className="h-4 w-4 shrink-0" />
                <span>
                  Bot nicht erreichbar
                  {status.workerHeartbeatAt ? ` · ${formatDateTime(status.workerHeartbeatAt)}` : ''}
                </span>
              </div>
            ) : null}

            <div className="mt-4 grid grid-cols-2 gap-2">
              <StatusMetric
                icon={<Clock3 className="h-4 w-4" />}
                label="Nächste Werbung"
                value={formatNextAd(status.nextAdAt)}
                hint={status.nextAdAt ? formatDateTime(status.nextAdAt) : null}
              />
              <StatusMetric
                icon={<PauseCircle className="h-4 w-4" />}
                label="Pausen"
                value={status.snoozeCount === null ? '–' : `${status.snoozeCount}`}
                hint={status.snoozeRefreshAt ? `Neue ab ${formatDateTime(status.snoozeRefreshAt)}` : null}
                tone={(status.snoozeCount ?? 0) > 0 ? 'text-success' : 'text-warning'}
              />
              <StatusMetric
                icon={<Gamepad2 className="h-4 w-4" />}
                label="Match-Status"
                value={steamView.value}
                hint={steamView.hint}
                tone={steamView.tone}
              />
              <StatusMetric
                icon={<Clock3 className="h-4 w-4" />}
                label="Letzte Bot-Aktion"
                value={status.lastAction ? actionKindLabel(status.lastAction.kind) : 'Noch keine'}
                hint={status.lastAction ? `${actionOutcomeLabel(status.lastAction.outcome)} · ${formatDateTime(status.lastAction.at)}` : null}
                tone={status.lastAction ? 'text-text-secondary' : 'text-white'}
              />
            </div>

            <div className="mt-4 flex items-center gap-2 border-t border-border pt-3 text-xs text-text-secondary">
              <InfoHint
                label="Twitch-Steuerung erklären"
                text="Twitch entscheidet, wann Werbung fällig wird. Der Bot kann innerhalb der Twitch-Funktionen pausieren oder einen Werbeblock starten."
              />
              <span>Twitch legt Fälligkeit und Pausen fest.</span>
            </div>

            <div className="mt-4 border-t border-border pt-4">
              <div className="flex items-center justify-between gap-3">
                <h4 className="text-sm font-semibold text-white">Von Hand</h4>
                {!status.isLive ? <span className="text-[10px] text-text-secondary">Im Stream verfügbar</span> : null}
              </div>
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  type="button"
                  disabled={!canSnooze || actionPending !== null}
                  onClick={() => void queueAction('snooze')}
                  className="inline-flex min-h-9 items-center gap-2 rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-sm font-semibold text-accent transition-colors hover:bg-accent/20 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {actionPending === 'snooze' ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
                  5 Min. pausieren
                </button>
                <button
                  type="button"
                  disabled={!canRunCommercial || actionPending !== null}
                  onClick={() => void queueAction('commercial')}
                  className="inline-flex min-h-9 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-3 py-1.5 text-sm font-semibold text-primary transition-colors hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {actionPending === 'commercial' ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                  {draft.adDurationSeconds} Sek. Werbung
                </button>
              </div>
              <div className="mt-2 inline-flex max-w-full flex-wrap gap-1 rounded-lg border border-border bg-background/45 p-1">
                {AD_DURATION_OPTIONS.map((seconds) => (
                  <button
                    key={seconds}
                    type="button"
                    aria-pressed={draft.adDurationSeconds === seconds}
                    onClick={() => patch({ adDurationSeconds: seconds })}
                    className={`min-h-7 rounded-md px-2 py-1 text-[11px] font-semibold transition-colors ${
                      draft.adDurationSeconds === seconds
                        ? 'bg-primary/15 text-primary'
                        : 'text-text-secondary hover:text-white'
                    }`}
                  >
                    {seconds}s
                  </button>
                ))}
              </div>
            </div>
          </section>

          <section className="rounded-xl border border-border bg-background/35 p-4">
            <div className="flex items-center gap-2">
              <History className="h-4 w-4 text-primary" />
              <h3 className="text-sm font-semibold text-white">Verlauf</h3>
            </div>

            {summary ? (
              <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-3">
                <div>
                  <p className="text-base font-bold text-white">{summary.blocksRun}</p>
                  <p className="text-[10px] uppercase tracking-wider text-text-secondary">Blöcke</p>
                </div>
                <div>
                  <p className="text-base font-bold text-success">{summary.blocksInWindow}</p>
                  <p className="text-[10px] uppercase tracking-wider text-text-secondary">Im Fenster</p>
                </div>
                <div>
                  <p className="text-base font-bold text-white">{summary.budgetSecondsUsed} / {summary.budgetSecondsPlanned} Sek.</p>
                  <p className="text-[10px] uppercase tracking-wider text-text-secondary">Budget</p>
                </div>
                <div>
                  <p className="text-base font-bold text-text-secondary">{summary.postponed}</p>
                  <p className="text-[10px] uppercase tracking-wider text-text-secondary">Verschoben</p>
                </div>
              </div>
            ) : null}

            {historyEntries.length > 0 ? (
              <ul className="mt-3 max-h-56 divide-y divide-border/60 overflow-y-auto border-t border-border">
                {historyEntries.map((entry, index) => (
                  <li key={`${entry.at}-${index}`} className="flex items-baseline gap-3 py-2 text-xs">
                    <span className="shrink-0 tabular-nums font-semibold text-text-secondary">{formatTime(entry.at)}</span>
                    <span className={entry.decision === 'commercial' ? 'text-white' : 'text-text-secondary'}>{beschreibeEintrag(entry)}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-3 border-t border-border pt-3 text-xs text-text-secondary">Noch keine Entscheidungen in dieser Session.</p>
            )}
          </section>
        </div>
      </div>
    </motion.section>
  );
}

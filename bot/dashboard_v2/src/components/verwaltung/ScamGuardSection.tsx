import { useT } from '../../context/LanguageContext';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2, ShieldCheck } from 'lucide-react';
import { CommandRow, SectionCard } from './ui';
import { useSerialSave, type SaveOutcome } from './useSerialSave';
import {
  fetchScamSettings,
  saveScamSettings,
  type ScamGuardMode,
  type ScamGuardSettings,
} from '@/api/scamGuard';

const MODE_OPTIONS: Array<{ key: ScamGuardMode; label: string; desc: string }> = [
  {
    key: 'auto_ban',
    label: 'Auto-Bann',
    desc: 'Bei sehr hoher Sicherheit wird der Account automatisch im Kanal gebannt.',
  },
  {
    key: 'timeout',
    label: 'Auto-Timeout',
    desc: 'Bei sehr hoher Sicherheit wird der Account automatisch getimeoutet statt gebannt.',
  },
  {
    key: 'alert_only',
    label: 'Nur melden',
    desc: 'Verdächtige Fälle werden gespeichert. Der Bot führt keine automatische Aktion aus.',
  },
];

function asPercent(value: number): number {
  return Math.round(value * 100);
}

function useScamSettings() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [unsaved, setUnsaved] = useState(false);
  const [draft, setDraft] = useState<ScamGuardSettings | null>(null);
  const latest = useRef<ScamGuardSettings | null>(null);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    fetchScamSettings()
      .then((data) => {
        if (!active) return;
        latest.current = data;
        setDraft(data);
      })
      .catch((e) => active && setError(e instanceof Error ? e.message : 'Unbekannter Fehler'))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  const request = useSerialSave(saveScamSettings, (sent: ScamGuardSettings, outcome: SaveOutcome<ScamGuardSettings>) => {
    if (latest.current !== sent) return;
    if (!outcome.ok) {
      setError(outcome.error instanceof Error ? outcome.error.message : 'Speichern fehlgeschlagen');
      return;
    }
    latest.current = outcome.result;
    setDraft(outcome.result);
    setUnsaved(false);
    setSavedAt(Date.now());
  });

  const persist = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    const next = latest.current;
    if (!next) return;
    setError(null);
    setSavedAt(null);
    request(next);
  }, [request]);

  useEffect(() => () => {
    if (timer.current !== null) persist();
  }, [persist]);

  const update = (next: Partial<ScamGuardSettings>, debounce: boolean) => {
    if (!latest.current) return;
    latest.current = { ...latest.current, ...next };
    setDraft(latest.current);
    setUnsaved(true);
    if (!debounce) {
      persist();
      return;
    }
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(persist, 600);
  };

  return { loading, error, savedAt, unsaved, draft, update };
}

function ModeSelector({ mode, onChange }: { mode: ScamGuardMode; onChange: (mode: ScamGuardMode) => void }) {
  const t = useT();
  const activeMode = MODE_OPTIONS.find((m) => m.key === mode) ?? MODE_OPTIONS[0];
  return (
    <div>
      <p className="text-sm font-bold text-white">{t("Verhalten bei hoher Sicherheit")}</p>
      <p className="mt-0.5 mb-3 text-xs text-text-secondary">{t(activeMode.desc)}</p>
      <div role="group" aria-label={t("Verhalten bei hoher Sicherheit")} className="grid grid-cols-3 gap-1 rounded-lg border border-border p-1">
        {MODE_OPTIONS.map((option) => {
          const selected = mode === option.key;
          return (
            <button
              key={option.key}
              type="button"
              aria-pressed={selected}
              onClick={() => onChange(option.key)}
              className={`rounded-md px-2 py-1.5 text-sm font-semibold transition-colors ${
                selected ? 'bg-primary/15 text-primary' : 'text-text-secondary hover:text-white'
              }`}
            >
              {t(option.label)}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ThresholdSlider({ label, hint, value, max, accent, onChange }: { label: string; hint: string; value: number; max: number; accent: 'primary' | 'accent'; onChange: (value: number) => void }) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <p className="text-sm font-semibold text-white">{label}</p>
        <span className={`text-sm font-bold tabular-nums ${accent === 'primary' ? 'text-primary' : 'text-accent'}`}>{asPercent(value)} %</span>
      </div>
      <p className="mb-2 text-xs text-text-secondary">{hint}</p>
      <input
        type="range"
        min={50}
        max={max}
        step={1}
        value={asPercent(value)}
        onChange={(e) => onChange(Number(e.target.value) / 100)}
        className={`w-full ${accent === 'primary' ? 'accent-primary' : 'accent-accent'}`}
        aria-label={label}
      />
    </div>
  );
}

function SettingsBlock() {
  const t = useT();
  const { loading, error, savedAt, unsaved, draft, update } = useScamSettings();

  if (!draft) {
    if (!loading && error) return <p role="alert" className="text-sm text-danger">{t(error)}</p>;
    return (
      <div className="flex items-center gap-3 text-text-secondary text-sm">
        <Loader2 className="h-4 w-4 animate-spin text-primary" />
        {t("Einstellungen werden geladen ...")}
      </div>
    );
  }

  return (
    <div data-unsaved={unsaved}>
      {error && <p role="alert" className="mb-2 text-sm text-danger">{t(error)}</p>}
      <CommandRow
        title={t("Scam-Schutz aktiv")}
        description={t("Prüft Erstschreiber in deinem Chat auf aufgesetzte Betrugsmaschen.")}
        inputSide="end"
        savedAt={savedAt}
        toggle={{ checked: draft.enabled, onChange: () => update({ enabled: !draft.enabled }, false), label: t("Scam-Schutz aktiv") }}
      />
      <div className="grid gap-6 pt-4 md:grid-cols-2">
        <ModeSelector mode={draft.mode} onChange={(mode) => update({ mode }, false)} />
        <div className="space-y-5">
          <ThresholdSlider
            label={t("Schwelle für die automatische Aktion")}
            hint={`${t("Ab dieser Sicherheit greift")} ${draft.mode === 'alert_only' ? t("die Auto-Aktion (aktuell deaktiviert)") : t("der gewählte Modus")}.`}
            value={draft.threshold}
            max={100}
            accent="primary"
            onChange={(threshold) => update({ threshold, suggestion_floor: Math.min(draft.suggestion_floor, threshold) }, true)}
          />
          <ThresholdSlider
            label={t("Schwelle für Vorschläge")}
            hint={t("Ab dieser Sicherheit wird ein verdächtiger Fall gespeichert (höchstens so hoch wie die Auto-Schwelle).")}
            value={draft.suggestion_floor}
            max={asPercent(draft.threshold)}
            accent="accent"
            onChange={(suggestion_floor) => update({ suggestion_floor }, true)}
          />
        </div>
      </div>
    </div>
  );
}

export function ScamGuardSection() {
  const t = useT();
  return (
    <SectionCard
      eyebrow="Moderation"
      icon={<ShieldCheck className="h-4 w-4" />}
      title={t("Scam-Schutz")}
      description={t("Ein KI-Wächter prüft Erstschreiber auf aufgesetzte Betrugsmaschen (z. B. Beziehungs- oder Wachstums-Pitches), die einfache Wortfilter durchrutschen. Du steuerst hier das Verhalten.")}
    >
      <SettingsBlock />
    </SectionCard>
  );
}

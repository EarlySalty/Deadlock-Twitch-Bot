import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, ShieldCheck } from 'lucide-react';
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

function SettingsBlock() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [draft, setDraft] = useState<ScamGuardSettings | null>(null);
  const [baseline, setBaseline] = useState<ScamGuardSettings | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    fetchScamSettings()
      .then((data) => {
        if (!active) return;
        setDraft(data);
        setBaseline(data);
      })
      .catch((e) => active && setError(e instanceof Error ? e.message : 'Unbekannter Fehler'))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  const dirty =
    draft !== null &&
    baseline !== null &&
    (draft.enabled !== baseline.enabled ||
      draft.mode !== baseline.mode ||
      draft.threshold !== baseline.threshold ||
      draft.suggestion_floor !== baseline.suggestion_floor);

  const patch = (next: Partial<ScamGuardSettings>) => {
    setSaved(false);
    setDraft((prev) => (prev ? { ...prev, ...next } : prev));
  };

  const onSave = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const result = await saveScamSettings(draft);
      setDraft(result);
      setBaseline(result);
      setSaved(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setSaving(false);
    }
  };

  if (loading || !draft) {
    return (
      <div className="flex items-center gap-3 text-text-secondary text-sm">
        <Loader2 className="h-4 w-4 animate-spin text-primary" />
        Einstellungen werden geladen ...
      </div>
    );
  }

  const activeMode = MODE_OPTIONS.find((m) => m.key === draft.mode) ?? MODE_OPTIONS[0];
  // suggestion_floor darf nie über der Auto-Schwelle liegen (Backend erzwingt das).
  const floorMax = draft.threshold;

  return (
    <div className="space-y-5" data-unsaved={dirty}>
      {error && (
        <div className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {/* Aktiv-Schalter */}
      <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div className="min-w-0">
            <p className="text-base font-bold text-white">Scam-Schutz aktiv</p>
            <p className="text-xs text-text-secondary mt-0.5">
              Prüft Erstschreiber in deinem Chat auf aufgesetzte Betrugsmaschen.
            </p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={draft.enabled}
            aria-label="Scam-Schutz aktiv"
            onClick={() => patch({ enabled: !draft.enabled })}
            className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors ${
              draft.enabled ? 'bg-primary' : 'bg-border'
            }`}
          >
            <span
              className={`inline-block h-5 w-5 transform rounded-full bg-white transition-[transform,translate,scale] ${
                draft.enabled ? 'translate-x-6' : 'translate-x-1'
              }`}
            />
          </button>
        </div>
      </div>

      {/* Modus */}
      <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
        <p className="text-base font-bold text-white mb-1">Verhalten bei hoher Sicherheit</p>
        <p className="text-xs text-text-secondary mb-3">{activeMode.desc}</p>
        <div className="grid grid-cols-3 gap-2">
          {MODE_OPTIONS.map((option) => {
            const selected = draft.mode === option.key;
            return (
              <button
                key={option.key}
                type="button"
                onClick={() => patch({ mode: option.key })}
                className={`rounded-lg border px-3 py-2 text-sm font-semibold transition-colors ${
                  selected
                    ? 'border-primary bg-primary/15 text-primary'
                    : 'border-border bg-background/40 text-text-secondary hover:border-border-hover hover:text-white'
                }`}
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Schwellen */}
      <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 space-y-5">
        <div>
          <div className="flex items-center justify-between mb-1">
            <p className="text-sm font-semibold text-white">Schwelle für die automatische Aktion</p>
            <span className="text-sm font-bold text-primary tabular-nums">{asPercent(draft.threshold)} %</span>
          </div>
          <p className="text-xs text-text-secondary mb-2">
            Ab dieser Sicherheit greift {draft.mode === 'alert_only' ? 'die Auto-Aktion (aktuell deaktiviert)' : 'der gewählte Modus'}.
          </p>
          <input
            type="range"
            min={50}
            max={100}
            step={1}
            value={asPercent(draft.threshold)}
            onChange={(e) => {
              const t = Number(e.target.value) / 100;
              // Vorschlags-Schwelle nachziehen, falls sie sonst darüber läge.
              patch({ threshold: t, suggestion_floor: Math.min(draft.suggestion_floor, t) });
            }}
            className="w-full accent-primary"
            aria-label="Schwelle für die automatische Aktion"
          />
        </div>

        <div>
          <div className="flex items-center justify-between mb-1">
            <p className="text-sm font-semibold text-white">Schwelle für Vorschläge</p>
            <span className="text-sm font-bold text-accent tabular-nums">{asPercent(draft.suggestion_floor)} %</span>
          </div>
          <p className="text-xs text-text-secondary mb-2">
            Ab dieser Sicherheit wird ein verdächtiger Fall gespeichert (höchstens so hoch wie die Auto-Schwelle).
          </p>
          <input
            type="range"
            min={50}
            max={asPercent(floorMax)}
            step={1}
            value={asPercent(draft.suggestion_floor)}
            onChange={(e) => patch({ suggestion_floor: Number(e.target.value) / 100 })}
            className="w-full accent-accent"
            aria-label="Schwelle für Vorschläge"
          />
        </div>
      </div>

      <div className="flex items-center gap-3 flex-wrap">
        <button
          type="button"
          disabled={!dirty || saving}
          onClick={() => void onSave()}
          className="inline-flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
          Einstellungen speichern
        </button>
        {saved && !dirty && <span className="text-sm text-success">Gespeichert.</span>}
        {dirty && !saving && <span className="text-sm text-text-secondary">Ungespeicherte Änderungen.</span>}
      </div>
    </div>
  );
}

export function ScamGuardSection() {
  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.24 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1 flex items-center gap-2">
          <ShieldCheck className="h-4 w-4" /> Moderation
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">Scam-Schutz</h2>
        <p className="text-sm text-text-secondary">
          Ein KI-Wächter prüft Erstschreiber auf aufgesetzte Betrugsmaschen (z. B. Beziehungs- oder
          Wachstums-Pitches), die einfache Wortfilter durchrutschen. Du steuerst hier das Verhalten.
        </p>
      </div>

      <SettingsBlock />
    </motion.section>
  );
}

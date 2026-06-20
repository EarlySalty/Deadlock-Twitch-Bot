import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import {
  AlertTriangle,
  Ban,
  ChevronDown,
  ChevronUp,
  Loader2,
  RotateCcw,
  ShieldCheck,
  ShieldQuestion,
} from 'lucide-react';
import {
  banScamVerdict,
  fetchScamQueue,
  fetchScamSettings,
  fetchScamVerdict,
  ignoreScamVerdict,
  revokeScamVerdict,
  saveScamSettings,
  type ScamGuardMode,
  type ScamGuardSettings,
  type ScamQueueItem,
  type ScamVerdictDetail,
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
    desc: 'Es wird nichts automatisch ausgeführt — verdächtige Fälle landen nur in der Queue.',
  },
];

function asPercent(value: number): number {
  return Math.round(value * 100);
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString('de-DE', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

/** Farbcodierung der Sicherheit: hoch = rot (Auto-Schwelle), mittel = orange. */
function confidenceTone(confidence: number): string {
  if (confidence >= 0.9) return 'border-error/40 bg-error/10 text-error';
  if (confidence >= 0.8) return 'border-warning/40 bg-warning/10 text-warning';
  return 'border-border bg-background/60 text-text-secondary';
}

// ── Einstellungen ────────────────────────────────────────────────────────────

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
    <div className="space-y-5">
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
              className={`inline-block h-5 w-5 transform rounded-full bg-white transition-transform ${
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
            Ab dieser Sicherheit landet ein Fall als Vorschlag in der Queue (höchstens so hoch wie die Auto-Schwelle).
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

// ── Queue-Eintrag ────────────────────────────────────────────────────────────

type ItemState = 'open' | 'banned' | 'timed_out' | 'ignored' | 'revoked';

/** Karten-Anfangszustand aus dem bereits durchgeführten Aktionstyp ableiten. */
function initialItemState(action: string): ItemState {
  if (action === 'banned') return 'banned';
  if (action === 'timed_out') return 'timed_out';
  return 'open';
}

/** Badge für bereits automatisch durchgesetzte Fälle (Vorschlag = kein Badge). */
function actionBadge(action: string): { label: string; tone: string } | null {
  if (action === 'banned') return { label: 'Auto-gebannt', tone: 'border-error/40 bg-error/10 text-error' };
  if (action === 'timed_out') return { label: 'Auto-Timeout', tone: 'border-warning/40 bg-warning/10 text-warning' };
  return null;
}

function QueueItemCard({ item }: { item: ScamQueueItem }) {
  const [state, setState] = useState<ItemState>(() => initialItemState(item.action_taken));
  const [busy, setBusy] = useState<null | 'ban' | 'ignore' | 'revoke'>(null);
  const [note, setNote] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [detail, setDetail] = useState<ScamVerdictDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const badge = actionBadge(item.action_taken);

  const toggleDetail = async () => {
    const next = !expanded;
    setExpanded(next);
    if (next && !detail && !detailLoading) {
      setDetailLoading(true);
      setDetailError(null);
      try {
        setDetail(await fetchScamVerdict(item.id));
      } catch (e) {
        setDetailError(e instanceof Error ? e.message : 'Details nicht verfügbar');
      } finally {
        setDetailLoading(false);
      }
    }
  };

  const onBan = async () => {
    setBusy('ban');
    setNote(null);
    try {
      const result = await banScamVerdict(item.id);
      switch (result.status) {
        case 'enforced':
          setState('banned');
          setNote('Account wurde im Kanal gebannt.');
          break;
        case 'ban_failed_no_mod':
          setNote('Bann fehlgeschlagen — der Bot ist in deinem Kanal kein Moderator.');
          break;
        case 'not_eligible':
          setNote('Dieser Vorschlag ist nicht mehr offen.');
          break;
        default:
          setNote('Vorschlag nicht gefunden.');
      }
    } catch (e) {
      setNote(e instanceof Error ? e.message : 'Bann fehlgeschlagen');
    } finally {
      setBusy(null);
    }
  };

  const onIgnore = async () => {
    setBusy('ignore');
    setNote(null);
    try {
      await ignoreScamVerdict(item.id);
      setState('ignored');
      setNote('Als harmlos markiert.');
    } catch (e) {
      setNote(e instanceof Error ? e.message : 'Ignorieren fehlgeschlagen');
    } finally {
      setBusy(null);
    }
  };

  const onRevoke = async () => {
    const wasTimeout = state === 'timed_out';
    setBusy('revoke');
    setNote(null);
    try {
      const result = await revokeScamVerdict(item.id);
      if (result.status === 'revoked') {
        setState('revoked');
        setNote(
          wasTimeout
            ? 'Timeout aufgehoben, Account wieder entbannt.'
            : 'Bann zurückgenommen, Account wieder entbannt.',
        );
      } else {
        setNote('Urteil nicht gefunden.');
      }
    } catch (e) {
      setNote(e instanceof Error ? e.message : 'Rücknahme fehlgeschlagen');
    } finally {
      setBusy(null);
    }
  };

  const resolved = state === 'ignored' || state === 'revoked';

  return (
    <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
      <div className="flex items-start justify-between gap-3 flex-wrap">
        <div className="min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <p className="text-base font-bold text-white font-mono">@{item.chatter_login}</p>
            <span className={`rounded-full border px-2 py-0.5 text-[11px] font-semibold ${confidenceTone(item.confidence)}`}>
              {asPercent(item.confidence)} % Sicherheit
            </span>
            <span className="rounded-full border border-border bg-background/40 px-2 py-0.5 text-[11px] font-medium text-text-secondary">
              {item.category}
            </span>
            {badge && (
              <span className={`rounded-full border px-2 py-0.5 text-[11px] font-semibold ${badge.tone}`}>
                {badge.label}
              </span>
            )}
          </div>
          <p className="text-xs text-text-secondary mt-0.5">{formatDate(item.created_at)}</p>
        </div>
        <button
          type="button"
          onClick={() => void toggleDetail()}
          className="inline-flex items-center gap-1 text-xs font-semibold text-text-secondary transition-colors hover:text-white"
        >
          {expanded ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
          Details
        </button>
      </div>

      <p className="mt-2 text-sm text-text-secondary">{item.reasoning}</p>

      {expanded && (
        <div className="mt-3 rounded-lg border border-border/60 bg-background/40 p-3">
          {detailLoading ? (
            <div className="flex items-center gap-2 text-xs text-text-secondary">
              <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
              Transkript wird geladen ...
            </div>
          ) : detailError ? (
            <p className="text-xs text-danger">{detailError}</p>
          ) : detail ? (
            <div className="space-y-2">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-text-secondary">
                Chat-Auszug (Grundlage des Urteils)
              </p>
              <pre className="whitespace-pre-wrap break-words text-xs text-text-secondary font-mono">
                {detail.transcript_snapshot || '—'}
              </pre>
            </div>
          ) : null}
        </div>
      )}

      {note && (
        <div className="mt-3 flex items-start gap-2 rounded-lg border border-border/60 bg-background/40 px-3 py-2 text-xs text-text-secondary">
          <AlertTriangle className="h-3.5 w-3.5 mt-0.5 shrink-0 text-warning" />
          <span>{note}</span>
        </div>
      )}

      {/* Aktionen je nach Zustand */}
      {!resolved && (
        <div className="mt-3 flex items-center gap-2 flex-wrap">
          {state === 'open' ? (
            <>
              <button
                type="button"
                disabled={busy !== null}
                onClick={() => void onBan()}
                className="inline-flex items-center gap-2 rounded-lg border border-error/40 bg-error/10 px-4 py-2 text-sm font-semibold text-error transition-colors hover:border-error/60 hover:bg-error/20 disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {busy === 'ban' ? <Loader2 className="h-4 w-4 animate-spin" /> : <Ban className="h-4 w-4" />}
                Bannen
              </button>
              <button
                type="button"
                disabled={busy !== null}
                onClick={() => void onIgnore()}
                className="inline-flex items-center gap-2 rounded-lg border border-border bg-background/40 px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:border-border-hover hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {busy === 'ignore' ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldQuestion className="h-4 w-4" />}
                Harmlos
              </button>
            </>
          ) : (
            // state === 'banned' | 'timed_out' → Rücknahme anbieten (echter Twitch-Unban via Bot)
            <button
              type="button"
              disabled={busy !== null}
              onClick={() => void onRevoke()}
              className="inline-flex items-center gap-2 rounded-lg border border-border bg-background/40 px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:border-border-hover hover:text-white disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {busy === 'revoke' ? <Loader2 className="h-4 w-4 animate-spin" /> : <RotateCcw className="h-4 w-4" />}
              {state === 'timed_out' ? 'Timeout aufheben' : 'Bann zurücknehmen'}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ── Queue ────────────────────────────────────────────────────────────────────

function QueueBlock() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<ScamQueueItem[]>([]);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setItems(await fetchScamQueue());
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Queue nicht verfügbar');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  return (
    <div>
      <div className="flex items-center justify-between gap-3 mb-1 flex-wrap">
        <h3 className="text-lg font-bold text-white">Gemeldete Fälle</h3>
        <button
          type="button"
          onClick={() => void load()}
          disabled={loading}
          className="inline-flex items-center gap-1.5 text-xs font-semibold text-text-secondary transition-colors hover:text-white disabled:opacity-40"
        >
          {loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RotateCcw className="h-3.5 w-3.5" />}
          Aktualisieren
        </button>
      </div>
      <p className="text-xs text-text-secondary mb-3">
        Vorgeschlagene Fälle arbeitest du hier ab; bereits automatisch gebannte oder getimeoutete Fälle
        kannst du mit einem Klick wieder zurücknehmen — das hebt den Bann auch im Kanal auf.
      </p>

      {error && (
        <div className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          Vorschläge werden geladen ...
        </div>
      ) : items.length === 0 ? (
        <div className="rounded-xl border border-border/60 bg-background/40 px-4 py-6 text-center text-sm text-text-secondary">
          Keine offenen Fälle. Der Scam-Schutz meldet sich, sobald ein verdächtiger Erstschreiber auftaucht.
        </div>
      ) : (
        <div className="space-y-3">
          {items.map((item) => (
            <QueueItemCard key={item.id} item={item} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Section ──────────────────────────────────────────────────────────────────

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
          Wachstums-Pitches), die einfache Wortfilter durchrutschen. Du steuerst hier das Verhalten und
          arbeitest gemeldete Fälle ab.
        </p>
      </div>

      <div className="space-y-6">
        <SettingsBlock />
        <div className="h-px bg-border/60" />
        <QueueBlock />
      </div>
    </motion.section>
  );
}

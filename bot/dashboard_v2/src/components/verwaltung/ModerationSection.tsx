import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, ShieldCheck } from 'lucide-react';
import {
  fetchModerationSettings,
  saveModerationSettings,
  type ModerationSettings,
} from '@/api/moderation';

type ModerationKey = keyof ModerationSettings;

const TASKS: Array<{ key: ModerationKey; label: string; desc: string }> = [
  {
    key: 'global_ban_enabled',
    label: 'Bekannte Betrüger fernhalten',
    desc: 'Accounts, die in der Community schon als Betrüger aufgefallen sind, kommen in deinem Chat gar nicht erst zum Zug.',
  },
  {
    key: 'scam_pitch_enabled',
    label: 'Vor Abzock-Maschen warnen',
    desc: 'Der Bot erkennt typische Anwerb- und Abzock-Nachrichten und warnt frühzeitig davor.',
  },
  {
    key: 'spam_autoban_enabled',
    label: 'Werbe-Spam automatisch stoppen',
    desc: 'Offensichtliche Werbung und Viewer-Kauf-Nachrichten fliegen automatisch aus deinem Chat.',
  },
  {
    key: 'sus_invite_enabled',
    label: 'Verdächtige Discord-Einladungen bremsen',
    desc: 'Fremde Discord-Einladungen von ganz neuen Zuschauern werden kurz ausgebremst, damit niemand in eine Falle gelockt wird.',
  },
];

export function ModerationSection() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [draft, setDraft] = useState<ModerationSettings | null>(null);
  const [baseline, setBaseline] = useState<ModerationSettings | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    fetchModerationSettings()
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
    TASKS.some((task) => draft[task.key] !== baseline[task.key]);

  const toggle = (key: ModerationKey) => {
    setSaved(false);
    setDraft((prev) => (prev ? { ...prev, [key]: !prev[key] } : prev));
  };

  const onSave = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const result = await saveModerationSettings(draft);
      setDraft(result);
      setBaseline(result);
      setSaved(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setSaving(false);
    }
  };

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.2 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1 flex items-center gap-2">
          <ShieldCheck className="h-4 w-4" /> Moderation
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">Was der Bot übernimmt</h2>
        <p className="text-sm text-text-secondary">
          Lege fest, welche Aufgaben der Bot in deinem Chat automatisch übernimmt. Alles ist von
          Haus aus an. Schaltest du etwas aus, lässt der Bot diesen Bereich in Ruhe.
        </p>
      </div>

      {loading || !draft ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          Einstellungen werden geladen ...
        </div>
      ) : (
        <div className="space-y-5">
          {error && (
            <div className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
              {error}
            </div>
          )}

          <div className="space-y-3">
            {TASKS.map((task) => {
              const on = draft[task.key];
              return (
                <div
                  key={task.key}
                  className="soft-elevate rounded-xl border border-border bg-background/60 p-4"
                >
                  <div className="flex items-center justify-between gap-4 flex-wrap">
                    <div className="min-w-0">
                      <p className="text-base font-bold text-white">{task.label}</p>
                      <p className="text-xs text-text-secondary mt-0.5">{task.desc}</p>
                    </div>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={on}
                      aria-label={task.label}
                      onClick={() => toggle(task.key)}
                      className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors ${
                        on ? 'bg-primary' : 'bg-border'
                      }`}
                    >
                      <span
                        className={`inline-block h-5 w-5 transform rounded-full bg-white transition-[transform,translate,scale] ${
                          on ? 'translate-x-6' : 'translate-x-1'
                        }`}
                      />
                    </button>
                  </div>
                </div>
              );
            })}
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
            {dirty && !saving && (
              <span className="text-sm text-text-secondary">Ungespeicherte Änderungen.</span>
            )}
          </div>
        </div>
      )}
    </motion.section>
  );
}

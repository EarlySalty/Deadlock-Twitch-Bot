import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { BellOff, Loader2, MessageSquare, ShieldAlert } from 'lucide-react';
import {
  fetchSilentSettings,
  saveSilentSettings,
  type SilentSettings,
} from '@/api/silentNotifications';

type FlagKey = keyof SilentSettings;

const ROWS: Array<{
  key: FlagKey;
  icon: typeof ShieldAlert;
  title: string;
  desc: string;
}> = [
  {
    key: 'silent_ban',
    icon: ShieldAlert,
    title: 'Auto-Ban-Hinweise stummschalten',
    desc: 'Wenn aktiv, postet der Bot keine Chat-Notiz mehr, sobald er jemanden automatisch bannt.',
  },
  {
    key: 'silent_raid',
    icon: MessageSquare,
    title: 'Raid-Hinweise stummschalten',
    desc: 'Wenn aktiv, postet der Bot keine Chat-Notiz mehr zu automatischen Raids.',
  },
];

export function SilentNotificationsSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<SilentSettings>({
    silent_ban: false,
    silent_raid: false,
  });
  const [pendingKey, setPendingKey] = useState<FlagKey | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchSilentSettings();
      setSettings({
        silent_ban: Boolean(data.silent_ban),
        silent_raid: Boolean(data.silent_raid),
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Unbekannter Fehler');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onToggle = async (key: FlagKey) => {
    const next = { ...settings, [key]: !settings[key] };
    setPendingKey(key);
    setError(null);
    // Optimistisch umschalten, bei Fehler zurückrollen.
    setSettings(next);
    try {
      const saved = await saveSilentSettings(next);
      setSettings({
        silent_ban: Boolean(saved.silent_ban),
        silent_raid: Boolean(saved.silent_raid),
      });
    } catch (e) {
      setSettings(settings);
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setPendingKey(null);
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
          <BellOff className="h-4 w-4" /> Chat-Benachrichtigungen
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">Stille Hinweise</h2>
        <p className="text-sm text-text-secondary">
          Steuere, ob der Bot Chat-Notizen zu Auto-Bans und Raids postet. Identisch zu den
          Chat-Befehlen <code className="text-primary">!silentban</code> /{' '}
          <code className="text-primary">!silentraid</code> — beide Wege bleiben synchron.
        </p>
      </div>

      {error && (
        <div className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          Einstellungen werden geladen ...
        </div>
      ) : (
        <div className="space-y-3">
          {ROWS.map(({ key, icon: Icon, title, desc }) => {
            const on = settings[key];
            const pending = pendingKey === key;
            return (
              <div
                key={key}
                className="soft-elevate rounded-xl border border-border bg-background/60 p-4"
              >
                <div className="flex items-center justify-between gap-4 flex-wrap">
                  <div className="min-w-0 flex items-start gap-3">
                    <Icon
                      className={`h-5 w-5 mt-0.5 ${on ? 'text-primary' : 'text-text-secondary'}`}
                    />
                    <div className="min-w-0">
                      <p className="text-base font-bold text-white">{title}</p>
                      <p className="text-xs text-text-secondary mt-0.5">{desc}</p>
                    </div>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={on}
                    aria-label={title}
                    disabled={pending}
                    onClick={() => void onToggle(key)}
                    className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors ${
                      on ? 'bg-primary' : 'bg-border'
                    } disabled:opacity-50 disabled:cursor-not-allowed`}
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white transition-[transform,translate,scale] ${
                        on ? 'translate-x-6' : 'translate-x-1'
                      }`}
                    />
                    {pending && (
                      <Loader2 className="absolute left-1/2 -translate-x-1/2 h-3.5 w-3.5 animate-spin text-white" />
                    )}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </motion.section>
  );
}

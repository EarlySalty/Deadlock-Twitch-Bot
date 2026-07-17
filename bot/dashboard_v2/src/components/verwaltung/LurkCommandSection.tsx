import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, Power, PowerOff } from 'lucide-react';
import {
  fetchLurkCommandSettings,
  toggleLurkCommand,
  type LurkCommandSettingsResponse,
} from '@/api/lurkCommand';

export function LurkCommandSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [settings, setSettings] = useState<LurkCommandSettingsResponse | null>(null);
  const [pending, setPending] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      setSettings(await fetchLurkCommandSettings());
    } catch {
      setError('PLATZHALTER: !lurk-Command-Status konnte nicht geladen werden.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const onToggle = async () => {
    if (!settings) return;
    const want = !settings.lurk_command_enabled;
    setPending(true);
    setError(null);
    setSuccess(null);
    try {
      const data = await toggleLurkCommand(want);
      setSettings({ lurk_command_enabled: data.lurk_command_enabled });
      setSuccess('PLATZHALTER: gespeichert.');
    } catch {
      setError('PLATZHALTER: Speichern fehlgeschlagen.');
    } finally {
      setPending(false);
    }
  };

  const enabled = Boolean(settings?.lurk_command_enabled);

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.17 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">
          Chat-Befehl
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">
          !lurk-Command
        </h2>
        <p className="text-sm text-text-secondary">
          PLATZHALTER: kurze Erklärung zum !lurk-Command.
        </p>
      </div>

      {error && (
        <div className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {success && (
        <div className="mb-4 rounded-lg border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {success}
        </div>
      )}

      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          PLATZHALTER: Status wird geladen.
        </div>
      ) : !settings ? (
        <div className="rounded-xl border border-border bg-background/40 px-4 py-6 text-sm text-text-secondary text-center">
          PLATZHALTER: Einstellung nicht verfügbar.
        </div>
      ) : (
        <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
          <div className="flex items-center justify-between gap-4 flex-wrap">
            <div className="min-w-0">
              <p
                className={`text-base font-bold ${
                  enabled ? 'text-success' : 'text-text-secondary'
                }`}
              >
                {enabled
                  ? 'PLATZHALTER: !lurk-Command ist aktiv'
                  : 'PLATZHALTER: !lurk-Command ist aus'}
              </p>
              <p className="text-xs text-text-secondary mt-0.5">
                PLATZHALTER: kurze Status-Erklärung.
              </p>
            </div>
            <button
              type="button"
              disabled={pending}
              onClick={() => void onToggle()}
              className={`inline-flex items-center gap-2 rounded-lg px-5 py-2.5 text-sm font-semibold transition-colors ${
                enabled
                  ? 'border border-danger/40 bg-danger/10 text-danger hover:bg-danger/20'
                  : 'border border-primary/40 bg-primary/10 text-primary hover:bg-primary/20'
              } disabled:opacity-50 disabled:cursor-not-allowed`}
            >
              {pending ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : enabled ? (
                <PowerOff className="h-4 w-4" />
              ) : (
                <Power className="h-4 w-4" />
              )}
              {enabled
                ? 'PLATZHALTER: !lurk deaktivieren'
                : 'PLATZHALTER: !lurk aktivieren'}
            </button>
          </div>
        </div>
      )}
    </motion.section>
  );
}

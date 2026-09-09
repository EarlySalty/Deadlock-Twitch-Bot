import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { CheckCircle2, CircleAlert, Gift, Loader2, Power, PowerOff } from 'lucide-react';
import {
  fetchLurkerTaxSettings,
  toggleLurkerTax,
  type LurkerTaxSettingsResponse,
} from '@/api/lurkerTax';

export function LurkerTaxSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [settings, setSettings] = useState<LurkerTaxSettingsResponse | null>(null);
  const [pending, setPending] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchLurkerTaxSettings();
      setSettings(data);
    } catch {
      setError('Lurker-Steuer-Status konnte nicht geladen werden.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onToggle = async () => {
    if (!settings) return;
    const want = !settings.lurker_tax_enabled;
    setPending(true);
    setError(null);
    setSuccess(null);
    try {
      const data = await toggleLurkerTax(want);
      setSettings((current) => ({
        lurker_tax_enabled: data.lurker_tax_enabled,
        has_moderator_read_chatters:
          current?.has_moderator_read_chatters ?? settings.has_moderator_read_chatters,
        reward_present: current?.reward_present ?? settings.reward_present,
      }));
      setSuccess('Gespeichert.');
    } catch {
      setError('Speichern fehlgeschlagen. Bitte versuch es nochmal.');
    } finally {
      setPending(false);
    }
  };

  const enabled = Boolean(settings?.lurker_tax_enabled);
  const scopeReady = Boolean(settings?.has_moderator_read_chatters);
  const rewardPresent = Boolean(settings?.reward_present);

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.16 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">
          Stammzuschauer
        </p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">
          Lurker-Steuer
        </h2>
        <p className="text-sm text-text-secondary">
          Erinnert deine ruhigsten Stamm-Lurker ab und zu mit einem freundlichen @-Hinweis
          daran, mal wieder Hallo zu sagen. Höchstens zwei pro Erinnerung und nur bei
          langjährigen Zuschauern. Standardmäßig aus.
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
          Status wird geladen ...
        </div>
      ) : !settings ? (
        <div className="rounded-xl border border-border bg-background/40 px-4 py-6 text-sm text-text-secondary text-center">
          Die Einstellung ist gerade nicht verfügbar. Bitte lade die Seite neu.
        </div>
      ) : (
        <>
          <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-5">
            <div className="flex items-center justify-between gap-4 flex-wrap">
              <div className="min-w-0">
                <p
                  className={`text-base font-bold ${
                    enabled ? 'text-success' : 'text-text-secondary'
                  }`}
                >
                  {enabled ? 'Lurker-Steuer ist AKTIV' : 'Lurker-Steuer ist aus'}
                </p>
                <p className="text-xs text-text-secondary mt-0.5">
                  {enabled
                    ? 'Holt sehr sparsam deine ruhigsten Stamm-Lurker zurück in den Chat.'
                    : 'Aktiviere sie, um stille Stammzuschauer sanft zurück in den Chat zu holen.'}
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
                {enabled ? 'Lurker-Steuer deaktivieren' : 'Lurker-Steuer aktivieren'}
              </button>
            </div>
          </div>

          <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4 mb-5">
            <div className="flex items-center justify-between gap-3 flex-wrap mb-3">
              <div className="flex items-center gap-2 min-w-0">
                <Gift className="h-4 w-4 text-primary shrink-0" />
                <p className="text-base font-bold text-white">Belohnung zum Einlösen</p>
              </div>
              {rewardPresent ? (
                <span className="inline-flex items-center gap-1.5 rounded-lg border border-success/40 bg-success/10 px-2.5 py-1 text-xs font-semibold text-success">
                  <CheckCircle2 className="h-3.5 w-3.5" />
                  Belohnung gefunden
                </span>
              ) : (
                <span className="inline-flex items-center gap-1.5 rounded-lg border border-warning/40 bg-warning/10 px-2.5 py-1 text-xs font-semibold text-warning">
                  <CircleAlert className="h-3.5 w-3.5" />
                  Belohnung fehlt
                </span>
              )}
            </div>
            <p className="text-sm text-text-secondary mb-3">
              Damit deine Lurker etwas zum Antippen haben, legst du in Twitch eine
              Kanalpunkte-Belohnung mit genau diesem Namen an. Den Preis bestimmst du selbst,
              zum Beispiel 10 Punkte.
            </p>
            <div className="rounded-lg border border-border bg-background/40 px-3 py-2 mb-3">
              <p className="text-xs text-text-secondary mb-0.5">Name der Belohnung</p>
              <p className="text-sm font-semibold text-white">Lurker Steuer</p>
            </div>
            <p className="text-xs font-semibold text-text-secondary mb-1.5">
              So legst du sie in Twitch an:
            </p>
            <ol className="list-decimal pl-5 space-y-1 text-sm text-text-secondary">
              <li>Öffne dein Twitch Creator-Dashboard.</li>
              <li>Geh zu Zuschauerbelohnungen.</li>
              <li>Öffne Punkte und Belohnungen.</li>
              <li>
                Wähle Individuelle Belohnung hinzufügen und nenn sie genau
                {' '}
                <span className="font-semibold text-white">Lurker Steuer</span>.
              </li>
            </ol>
          </div>

          {enabled && !scopeReady && (
            <div className="rounded-xl border border-warning/40 bg-warning/10 px-4 py-3 text-sm text-warning">
              Der Schalter ist an, aber der Community-Bot braucht dafür noch die Leseberechtigung
              für deine Zuschauerliste. Darum kümmern wir uns, du musst nichts tun.
            </div>
          )}
        </>
      )}
    </motion.section>
  );
}

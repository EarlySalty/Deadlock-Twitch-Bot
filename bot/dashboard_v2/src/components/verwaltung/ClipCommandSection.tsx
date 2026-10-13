import { EditableCommandName } from './CommandNameSection';
import { useEffect, useId, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, Power, PowerOff } from 'lucide-react';
import {
  fetchClipCommandSettings,
  setClipCommandDuration,
  toggleClipCommand,
  type ClipCommandSettingsResponse,
} from '@/api/clipCommand';

export function ClipCommandSection() {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [settings, setSettings] = useState<ClipCommandSettingsResponse | null>(null);
  const [pending, setPending] = useState(false);
  const [duration, setDuration] = useState('60');
  const durationId = useId();

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchClipCommandSettings();
      setSettings(data);
      setDuration(String(data.clip_command_duration_seconds));
    } catch {
      setError('Status konnte nicht geladen werden. Bitte Seite neu laden.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const onToggle = async () => {
    if (!settings) return;
    const want = !settings.clip_command_enabled;
    setPending(true);
    setError(null);
    setSuccess(null);
    try {
      const data = await toggleClipCommand(want);
      setSettings(data);
      setSuccess('Gespeichert.');
    } catch {
      setError('Speichern fehlgeschlagen. Bitte nochmal versuchen.');
    } finally {
      setPending(false);
    }
  };

  const onSaveDuration = async () => {
    const seconds = Number(duration);
    if (!duration || !Number.isFinite(seconds) || seconds < 5 || seconds > 60
      || Math.abs(seconds * 10 - Math.round(seconds * 10)) >= 1e-7) {
      setSuccess(null);
      setError('Bitte 5 bis 60 Sekunden eingeben, in Schritten von 0,1 Sekunden.');
      return;
    }
    setPending(true);
    setError(null);
    setSuccess(null);
    try {
      const data = await setClipCommandDuration(seconds);
      setSettings(data);
      setDuration(String(data.clip_command_duration_seconds));
      setSuccess('Clip-Dauer gespeichert.');
    } catch {
      setError('Clip-Dauer konnte nicht gespeichert werden. Bitte nochmal versuchen.');
    } finally {
      setPending(false);
    }
  };

  const enabled = Boolean(settings?.clip_command_enabled);

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
          Clip-Befehl
        </h2>
        <p className="text-sm text-text-secondary">
          Steuert, ob dein Chat per Befehl einen Twitch-Clip erstellen kann. Aus heißt: der
          Bot legt keinen Clip mehr an und antwortet auch nicht darauf.
        </p>
      </div>

      {error && (
        <div role="alert" className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </div>
      )}

      {success && (
        <div role="status" className="mb-4 rounded-lg border border-success/40 bg-success/10 px-3 py-2 text-sm text-success">
          {success}
        </div>
      )}

      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          Status wird geladen …
        </div>
      ) : !settings ? (
        <div className="rounded-xl border border-border bg-background/40 px-4 py-6 text-sm text-text-secondary text-center">
          Einstellung gerade nicht verfügbar.
        </div>
      ) : (
        <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
          <div className="flex items-center justify-between gap-4 flex-wrap">
            <div className="min-w-0">
              <EditableCommandName command="clip" className="mb-2" />
              <p
                className={`text-base font-bold ${
                  enabled ? 'text-success' : 'text-text-secondary'
                }`}
              >
                {enabled ? 'Befehl ist aktiv' : 'Befehl ist aus'}
              </p>
              <p className="text-xs text-text-secondary mt-0.5">
                {enabled
                  ? 'Dein Chat kann per Befehl einen Clip erstellen.'
                  : 'Der Bot erstellt damit nichts mehr und bleibt stumm.'}
              </p>
            </div>
            <button
              type="button"
              aria-label={`Clip-Befehl ${enabled ? 'deaktivieren' : 'aktivieren'}`}
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
              {enabled ? 'Deaktivieren' : 'Aktivieren'}
            </button>
          </div>
          <form
            className="mt-5 border-t border-border pt-5"
            onSubmit={(event) => { event.preventDefault(); void onSaveDuration(); }}
          >
            <label htmlFor={durationId} className="block text-sm font-semibold text-white mb-2">
              Clip-Dauer in Sekunden
            </label>
            <div className="flex flex-wrap items-center gap-3">
              <input
                id={durationId}
                type="number"
                min="5"
                max="60"
                step="0.1"
                required
                value={duration}
                disabled={pending}
                aria-describedby={`${durationId}-hint`}
                onChange={(event) => { setDuration(event.target.value); setSuccess(null); }}
                className="w-28 rounded-lg border border-border bg-background px-3 py-2.5 text-base text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50"
              />
              <button
                type="submit"
                disabled={pending || Number(duration) === settings.clip_command_duration_seconds}
                className="rounded-lg border border-primary/40 bg-primary/10 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/20 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Dauer speichern
              </button>
            </div>
            <p id={`${durationId}-hint`} className="mt-2 text-sm text-text-secondary">
              5 bis 60 Sekunden, Standard: 60 Sekunden. Am Streamanfang oder Streamende kann Twitch einen kürzeren Clip liefern.
            </p>
            <p className="mt-2 text-sm text-text-secondary">
              Eigener Titel: Schreibe den gewünschten Titel hinter den Clip-Befehl, zum Beispiel „Krasser Headshot“.
            </p>
          </form>
        </div>
      )}
    </motion.section>
  );
}

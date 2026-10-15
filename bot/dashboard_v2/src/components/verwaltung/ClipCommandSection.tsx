import { useT } from '../../context/LanguageContext';
import { useCallback, useEffect, useId, useRef, useState } from 'react';
import {
  fetchClipCommandSettings,
  setClipCommandDuration,
  toggleClipCommand,
  type ClipCommandSettingsResponse,
} from '@/api/clipCommand';
import { CommandRow } from './ui';
import { SavedIndicator } from './SavedIndicator';
import { useSerialSave, type SaveOutcome } from './useSerialSave';

function validDuration(value: string): boolean {
  const seconds = Number(value);
  return Boolean(value) && Number.isFinite(seconds) && seconds >= 5 && seconds <= 60
    && Math.abs(seconds * 10 - Math.round(seconds * 10)) < 1e-7;
}

function ClipDurationField({ settings, onSaved }: { settings: ClipCommandSettingsResponse; onSaved: (data: ClipCommandSettingsResponse) => void }) {
  const t = useT();
  const durationId = useId();
  const [duration, setDuration] = useState(String(settings.clip_command_duration_seconds));
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const savedValue = useRef(settings.clip_command_duration_seconds);
  const requested = useRef(settings.clip_command_duration_seconds);
  const latest = useRef(duration);

  const request = useSerialSave(
    (value: string) => setClipCommandDuration(Number(value)),
    (sent: string, outcome: SaveOutcome<ClipCommandSettingsResponse>) => {
      if (outcome.ok) {
        savedValue.current = outcome.result.clip_command_duration_seconds;
        onSaved(outcome.result);
      }
      if (latest.current !== sent) return;
      if (!outcome.ok) {
        requested.current = savedValue.current;
        setError('Clip-Dauer konnte nicht gespeichert werden. Bitte nochmal versuchen.');
        return;
      }
      requested.current = outcome.result.clip_command_duration_seconds;
      setDuration(String(outcome.result.clip_command_duration_seconds));
      setSavedAt(Date.now());
    },
  );

  const save = useCallback((value: string, showInvalid: boolean) => {
    if (!validDuration(value)) {
      if (showInvalid) setError('Bitte 5 bis 60 Sekunden eingeben, in Schritten von 0,1 Sekunden.');
      return;
    }
    const seconds = Number(value);
    setError(null);
    if (seconds === requested.current) return;
    requested.current = seconds;
    setSavedAt(null);
    request(value);
  }, [request]);

  const saveRef = useRef(save);
  useEffect(() => { saveRef.current = save; }, [save]);
  useEffect(() => () => { saveRef.current(latest.current, false); }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => save(duration, false), 600);
    return () => window.clearTimeout(timer);
  }, [duration, save]);

  return (
    <div>
      <div className="flex flex-wrap items-center gap-3">
        <label htmlFor={durationId} className="text-sm font-semibold text-white">
          {t("Clip-Dauer in Sekunden")}
        </label>
        <input
          id={durationId}
          type="number"
          min="5"
          max="60"
          step="0.1"
          required
          value={duration}
          aria-describedby={`${durationId}-hint`}
          aria-invalid={error !== null}
          onChange={(event) => { latest.current = event.target.value; setDuration(event.target.value); }}
          onBlur={() => save(latest.current, true)}
          className="w-24 rounded-lg border border-border bg-background px-3 py-2 text-sm text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        />
        {savedAt && <SavedIndicator key={savedAt} />}
      </div>
      {error && <p role="alert" className="mt-1 text-xs text-danger">{t(error)}</p>}
      <p id={`${durationId}-hint`} className="mt-1 text-xs text-text-secondary">
        {t("5 bis 60 Sekunden, Standard: 60 Sekunden. Am Streamanfang oder Streamende kann Twitch einen kürzeren Clip liefern.")}
      </p>
      <p className="mt-1 text-xs text-text-secondary">
        {t("Eigener Titel: Schreibe den gewünschten Titel hinter den Clip-Befehl, zum Beispiel „Krasser Headshot“.")}
      </p>
    </div>
  );
}

export function ClipCommandSection() {
  const t = useT();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [settings, setSettings] = useState<ClipCommandSettingsResponse | null>(null);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    let active = true;
    fetchClipCommandSettings()
      .then(data => { if (active) setSettings(data); })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const onToggle = async () => {
    if (!settings || pending) return;
    setPending(true);
    setError(null);
    setSavedAt(null);
    try {
      setSettings(await toggleClipCommand(!settings.clip_command_enabled));
      setSavedAt(Date.now());
    } catch {
      setError('Speichern fehlgeschlagen. Bitte nochmal versuchen.');
    } finally {
      setPending(false);
    }
  };

  const onDurationSaved = useCallback((data: ClipCommandSettingsResponse) => setSettings(previous => previous && { ...previous, clip_command_duration_seconds: data.clip_command_duration_seconds }), []);
  const enabled = Boolean(settings?.clip_command_enabled);
  const description = loading
    ? t("Status wird geladen …")
    : !settings
      ? t("Einstellung gerade nicht verfügbar.")
      : enabled
        ? t("Dein Chat kann per Befehl einen Clip erstellen.")
        : t("Der Bot erstellt damit nichts mehr und bleibt stumm.");

  return (
    <CommandRow
      command="clip"
      title={t("Clip-Befehl")}
      description={description}
      savedAt={savedAt}
      error={error}
      toggle={{ checked: enabled, onChange: () => void onToggle(), label: t("Clip-Befehl"), disabled: !settings, pending }}
    >
      {settings && enabled ? <ClipDurationField settings={settings} onSaved={onDurationSaved} /> : null}
    </CommandRow>
  );
}

import { useT } from '../../context/LanguageContext';
import { useEffect, useId, useState } from 'react';
import { countChars, fetchFollowHintSettings, followHintTooLong, normalizeFollowHint, previewFollowHint, saveFollowHintSettings, type FollowHintSettings } from '@/api/followHint';
import { CommandRow } from './ui';

export function FollowHintSection() {
  const t = useT();
  const messageId = useId();
  const [settings, setSettings] = useState<FollowHintSettings | null>(null);
  const [draft, setDraft] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    fetchFollowHintSettings()
      .then(value => { if (active) { setSettings(value); setDraft(value.message); } })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  async function persist(enabled: boolean, message: string) {
    if (busy) return;
    setBusy(true); setError(null); setSavedAt(null);
    try {
      const next = await saveFollowHintSettings(enabled, message);
      setSettings(next); setDraft(next.message); setSavedAt(Date.now());
    } catch (cause) {
      setError(cause instanceof Error && cause.message === 'message_too_long'
        ? 'Die Nachricht ist zu lang.'
        : 'Speichern fehlgeschlagen. Bitte nochmal versuchen.');
    } finally {
      setBusy(false);
    }
  }

  const maxChars = settings?.max_chars ?? 300;
  const normalizedDraft = normalizeFollowHint(draft);
  const tooLong = followHintTooLong(draft, maxChars);
  const dirty = settings !== null && normalizedDraft !== normalizeFollowHint(settings.message);
  const description = loading
    ? t('Status wird geladen …')
    : !settings
      ? t('Einstellung gerade nicht verfügbar.')
      : settings.enabled
        ? t('Schreibt jemand zum ersten Mal in deinen Chat und folgt dir noch nicht, bittet der Bot ihn einmal um einen Follow. Höchstens alle 90 Sekunden.')
        : t('Aus. Der Bot bittet neue Zuschauer nicht um einen Follow.');

  return (
    <CommandRow
      title={t('Follow-Hinweis für neue Zuschauer')}
      description={description}
      savedAt={savedAt}
      error={error}
      toggle={{
        checked: Boolean(settings?.enabled),
        onChange: () => { if (settings) void persist(!settings.enabled, draft); },
        label: t('Follow-Hinweis für neue Zuschauer'),
        disabled: !settings || tooLong,
        pending: busy,
      }}
    >
      {settings && (
        <div>
          <label htmlFor={messageId} className="text-sm font-semibold text-white">{t('Eigene Nachricht')}</label>
          <textarea
            id={messageId}
            rows={2}
            value={draft}
            placeholder={settings.default_message}
            aria-describedby={`${messageId}-hint`}
            aria-invalid={tooLong}
            disabled={busy}
            onChange={event => setDraft(event.target.value)}
            className="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-60"
          />
          <div className="mt-1 flex flex-wrap items-center gap-3">
            <button
              type="button"
              disabled={!dirty || tooLong || busy}
              onClick={() => void persist(settings.enabled, draft)}
              className="rounded-lg bg-primary px-3 py-1.5 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t('Nachricht speichern')}
            </button>
            <span className={`text-xs ${tooLong ? 'text-danger' : 'text-text-secondary'}`}>{countChars(normalizedDraft)} / {maxChars}</span>
            {tooLong && countChars(normalizedDraft) <= maxChars && (
              <span className="text-xs text-danger">{t('Zu lang für den Twitch-Chat, sobald die Namen eingesetzt sind.')}</span>
            )}
          </div>
          <p id={`${messageId}-hint`} className="mt-1 text-xs text-text-secondary">
            {t('{user} wird durch den Namen ersetzt. Ohne {user} setzt der Bot den Namen an den Anfang. Leer lassen für den Standardtext.')}
          </p>
          <p className="mt-1 text-xs text-text-secondary">
            {t('Vorschau:')} <span className="text-white">{previewFollowHint(draft, settings.default_message, 'neuer_zuschauer')}</span>
          </p>
        </div>
      )}
    </CommandRow>
  );
}

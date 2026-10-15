import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchPostingPlan, savePostingPlanSettings } from '@/api/socialMedia';
import { getBrowserTimezone } from '@/api/core';
import { useT } from '@/context/LanguageContext';
import { scheduleQueueApi } from '@/components/socialmedia/autopilot/schedule-queue';
import { importPoolApi } from '@/components/socialmedia/autopilot/import-pool/api';
import { SavedIndicator } from './SavedIndicator';

function supportedTimezones(): string[] {
  try { return Intl.supportedValuesOf('timeZone'); } catch { return []; }
}

const TIMEZONES = supportedTimezones();

function timezoneOptions(current: string): string[] {
  return current && !TIMEZONES.includes(current) ? [current, ...TIMEZONES] : TIMEZONES;
}

export function GlobalTimezone({ owner, channelScoped = false }: { owner: string; channelScoped?: boolean }) {
  return <TimezoneEditor key={owner} owner={owner} channelScoped={channelScoped} />;
}

function TimezoneEditor({ owner, channelScoped }: { owner: string; channelScoped: boolean }) {
  const t = useT();
  const client = useQueryClient();
  const [draft, setDraft] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const attempted = useRef<string | null>(null);
  const key = ['social-media', 'global-timezone', owner];
  const current = useQuery({ queryKey: key, queryFn: async () => {
    const [plan, schedule, preferences] = await Promise.all([fetchPostingPlan(owner), scheduleQueueApi.load(owner), importPoolApi.getPreferences(owner)]);
    return { plan, schedule, preferences };
  }, retry: false });
  const timezone = draft ?? current.data?.plan.timezone ?? '';
  let valid = false;
  try { if (timezone) { new Intl.DateTimeFormat('de-DE', { timeZone: timezone }).format(); valid = true; } } catch { valid = false; }
  const values = current.data ? [current.data.plan.timezone, current.data.schedule.settings.timezone, current.data.preferences.timezone] : [];
  const diverged = new Set(values).size > 1;
  const dirty = current.isSuccess && values.some(value => value !== timezone);
  const save = useMutation({ mutationFn: async () => {
    const preferences = await importPoolApi.getPreferences(owner);
    const results = await Promise.allSettled([
      savePostingPlanSettings(owner, { timezone }),
      scheduleQueueApi.save({ timezone }, owner),
      importPoolApi.savePreferences(owner, { ...preferences, timezone }),
    ]);
    if (results.some(result => result.status === 'rejected')) throw new Error('Zeitzone nicht überall gespeichert. Prüfe den aktuellen Stand und versuche es erneut.');
  }, onSuccess: () => { setDraft(null); setSavedAt(Date.now()); }, onSettled: async () => {
    await client.invalidateQueries({ queryKey: ['social-media'] });
    await client.invalidateQueries({ queryKey: ['social-posting-plan'] });
  } });
  const { mutate, isPending } = save;

  useEffect(() => {
    if (draft === null || draft === attempted.current || !valid || !dirty || isPending) return;
    attempted.current = draft;
    mutate();
  }, [draft, valid, dirty, isPending, mutate]);

  const locked = !current.isSuccess || isPending;
  const choose = (value: string) => { attempted.current = null; setSavedAt(null); setDraft(value); };

  return <section className="panel-card rounded-2xl p-5" data-unsaved={draft !== null && dirty ? 'true' : undefined} data-saving={isPending ? 'true' : undefined}>
    <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
      <div className="min-w-0 flex-1 basis-64">
        <h2 className="text-base font-bold text-white">{channelScoped ? t('Zeitzone für Kanal {channel}', { channel: owner }) : t('Zeitzone')}</h2>
        <p className="mt-0.5 text-xs text-text-secondary">{t('Gilt für neue Twitch-Clips, den Auto-Pilot und den Clip-Vorrat. Bestehende Termine bleiben unverändert.')}</p>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {savedAt && <SavedIndicator key={savedAt} />}
        <select aria-label={t('Zeitzone')} className="studio-input min-h-10 max-w-[16rem]" value={timezone} disabled={locked} required onChange={event => choose(event.target.value)}>
          {!timezone && <option value="">{t('Wird geladen')}</option>}
          {timezoneOptions(timezone).map(zone => <option key={zone} value={zone}>{zone}</option>)}
        </select>
        <button type="button" className="studio-button" disabled={locked} onClick={() => choose(getBrowserTimezone())}>{t('Browser-Zeitzone übernehmen')}</button>
      </div>
    </div>
    {current.isPending && <p role="status" className="mt-2 text-xs text-text-secondary">{t('Wird geladen')}</p>}
    {current.isError && <p role="alert" className="mt-2 text-xs text-danger">{t('Die Zeitzonen konnten nicht geladen werden.')}</p>}
    {diverged && <p role="status" className="mt-2 text-xs text-text-secondary">{t('Die gespeicherten Zeitzonen unterscheiden sich: {zones}. Speichern gleicht alle drei Bereiche ab.', { zones: values.join(' / ') })} <button type="button" className="underline" disabled={!valid || locked} onClick={() => mutate()}>{t('Jetzt abgleichen')}</button></p>}
    {save.isError && <p role="alert" className="mt-2 text-xs text-danger">{t('Zeitzone nicht überall gespeichert. Prüfe den aktuellen Stand und versuche es erneut.')} {!diverged && <button type="button" className="underline" disabled={!valid || locked} onClick={() => mutate()}>{t('Erneut versuchen')}</button>}</p>}
  </section>;
}

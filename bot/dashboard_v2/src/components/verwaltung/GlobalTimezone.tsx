import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchPostingPlan, savePostingPlanSettings } from '@/api/socialMedia';
import { getBrowserTimezone } from '@/api/core';
import { useT } from '@/context/LanguageContext';
import { scheduleQueueApi } from '@/components/socialmedia/autopilot/schedule-queue';
import { importPoolApi } from '@/components/socialmedia/autopilot/import-pool/api';

export function GlobalTimezone({ owner, channelScoped = false }: { owner: string; channelScoped?: boolean }) {
  return <TimezoneEditor key={owner} owner={owner} channelScoped={channelScoped} />;
}

function TimezoneEditor({ owner, channelScoped }: { owner: string; channelScoped: boolean }) {
  const t = useT();
  const client = useQueryClient();
  const [draft, setDraft] = useState<string | null>(null);
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
  }, onSuccess: () => setDraft(null), onSettled: async () => {
    await client.invalidateQueries({ queryKey: ['social-media'] });
    await client.invalidateQueries({ queryKey: ['social-posting-plan'] });
  } });
  return <section className="panel-card rounded-2xl p-5" data-unsaved={draft !== null && dirty ? 'true' : undefined} data-saving={save.isPending ? 'true' : undefined}>
    <h2 className="text-xl font-semibold">{channelScoped ? t('Zeitzone für Kanal {channel}', { channel: owner }) : t('Zeitzone')}</h2>
    <p>{t('Gilt für neue Twitch-Clips, den Auto-Pilot und den Clip-Vorrat. Bestehende Termine bleiben unverändert.')}</p>
    {current.isPending && <p role="status">{t('Wird geladen')}</p>}
    {current.isError && <p role="alert">{t('Die Zeitzonen konnten nicht geladen werden.')}</p>}
    {diverged && <p role="status">{t('Die gespeicherten Zeitzonen unterscheiden sich: {zones}. Speichern gleicht alle drei Bereiche ab.', { zones: values.join(' / ') })}</p>}
    <form onSubmit={event => { event.preventDefault(); if (valid && dirty && !save.isPending) save.mutate(); }}>
      <label>{t('Zeitzone')}<input className="studio-input" value={timezone} disabled={!current.isSuccess || save.isPending} required onChange={event => setDraft(event.target.value)} /></label>
      <button type="button" className="studio-button" disabled={!current.isSuccess || save.isPending} onClick={() => setDraft(getBrowserTimezone())}>{t('Browser-Zeitzone übernehmen')}</button>
      <button type="submit" className="studio-button" disabled={!current.isSuccess || !valid || !dirty || save.isPending}>{t('Zeitzone speichern')}</button>
    </form>
    {save.isError && <p role="alert">{t('Zeitzone nicht überall gespeichert. Prüfe den aktuellen Stand und versuche es erneut.')}</p>}
    {save.isSuccess && current.isSuccess && !current.isFetching && !save.isPending && !dirty && !diverged && <p role="status">{t('Zeitzone für alle drei Bereiche gespeichert.')}</p>}
  </section>;
}

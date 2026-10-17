import type { ReactNode } from 'react';
import { Plus, RefreshCw } from 'lucide-react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useT } from '@/context/LanguageContext';
import type { ScheduleQueueApi, ScheduleSettings } from './autopilot/schedule-queue';

type AutopilotPatch = Pick<ScheduleSettings, 'enabled'> | Pick<ScheduleSettings, 'paused'>;

export function StudioAutopilotSwitches({ owner, settings, failed, api, onSaved }: {
  owner: string;
  settings: ScheduleSettings | undefined;
  failed: boolean;
  api: ScheduleQueueApi;
  onSaved: () => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const save = useMutation({
    mutationFn: async (patch: AutopilotPatch) => {
      await api.load(owner);
      return api.save(patch, owner);
    },
    onSettled: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: ['social-media', 'autopilot-schedule'] }),
        client.invalidateQueries({ queryKey: ['social-media', 'autopilot-publish'] }),
        client.invalidateQueries({ queryKey: ['social-media', 'production-settings'] }),
      ]);
      onSaved();
    },
  });
  const locked = !settings || save.isPending;
  return <div className="studio-autopilot" data-saving={save.isPending ? 'true' : undefined} role="group" aria-label={t('Auto-Pilot')}>
    <label className="studio-switch"><input type="checkbox" role="switch" checked={settings?.enabled ?? false} disabled={locked} onChange={(event) => save.mutate({ enabled: event.target.checked })} />{t('Auto-Pilot aktiv')}</label>
    <label className="studio-switch"><input type="checkbox" role="switch" checked={settings?.paused ?? false} disabled={locked || !settings?.enabled} onChange={(event) => save.mutate({ paused: event.target.checked })} />{t('Produktion pausieren')}</label>
    {(failed || save.isError) && <p role="alert" className="studio-autopilot-error">{t('Speichern oder Laden fehlgeschlagen. Bitte erneut versuchen.')}</p>}
  </div>;
}

export function StudioHero({ streamer, onUpload, onRefresh, refreshing = false, autopilot }: {
  streamer: string;
  onUpload: () => void;
  onRefresh: () => void;
  refreshing?: boolean;
  autopilot?: ReactNode;
}) {
  const t = useT();
  return <header className="studio-header">
    <div className="min-w-0"><h1 className="studio-heading">{t('Social-Media-Studio')}</h1><p className="mt-2 text-sm text-text-secondary">{streamer}</p></div>
    <div className="studio-header-actions">
      {autopilot}
      <button type="button" className="studio-button" onClick={onUpload}><Plus className="h-4 w-4" aria-hidden />{t('Clip hinzufügen')}</button>
      <button type="button" className="studio-icon-button" onClick={onRefresh} aria-label={t('Alle Daten neu laden')} title={t('Alle Daten neu laden')} aria-busy={refreshing || undefined}><RefreshCw className={`h-4 w-4${refreshing ? ' animate-spin' : ''}`} aria-hidden /></button>
    </div>
  </header>;
}

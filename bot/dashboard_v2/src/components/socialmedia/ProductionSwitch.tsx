import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useT } from '@/context/LanguageContext';
import { scheduleQueueApi, type ScheduleQueueApi } from './autopilot/schedule-queue';

export function ProductionSwitch({ owner, disabled, api = scheduleQueueApi }: { owner: string; disabled: boolean; api?: ScheduleQueueApi }) {
  const t = useT();
  const client = useQueryClient();
  const key = ['social-media', 'production-settings', owner];
  const settings = useQuery({ queryKey: key, queryFn: ({ signal }) => api.load(owner, signal), retry: false });
  const save = useMutation({ mutationFn: (enabled: boolean) => api.save({ ai_editing_enabled: enabled }, owner), onSuccess: () => client.invalidateQueries({ queryKey: ['social-media'] }) });
  return <div data-saving={save.isPending ? 'true' : undefined}>
    <label className="studio-switch"><input type="checkbox" role="switch" checked={settings.data?.settings.ai_editing_enabled ?? false} disabled={disabled || !settings.isSuccess || save.isPending || settings.isFetching} onChange={event => save.mutate(event.target.checked)} />{t('KI-Bearbeitung verwenden')}</label>
    {(settings.isError || save.isError) && <p role="alert">{t('Speichern oder Laden fehlgeschlagen. Bitte erneut versuchen.')}</p>}
  </div>;
}

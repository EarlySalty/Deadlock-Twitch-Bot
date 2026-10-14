import { useQuery } from '@tanstack/react-query';
import { useT } from '@/context/LanguageContext';
import { fetchAiJobs } from './ai/api';
import { importPoolApi } from './autopilot/import-pool/api';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';

export function StudioActivity({ twitchUserId, data, onNavigate, onAi }: {
  twitchUserId?: string; data: StudioOverviewData; now: number;
  onNavigate: (view: SocialMediaViewTarget) => void; onAi: () => void;
}) {
  const t = useT();
  const jobs = useQuery({ queryKey: ['social-ai', 'jobs', twitchUserId], queryFn: ({ signal }) => fetchAiJobs(twitchUserId!, signal), enabled: Boolean(twitchUserId), retry: false, refetchInterval: 30000 });
  const imports = useQuery({ queryKey: ['social-media', 'activity-import', twitchUserId], queryFn: ({ signal }) => importPoolApi.getLatestImport(twitchUserId!, signal), enabled: Boolean(twitchUserId), retry: false, refetchInterval: 30000 });
  const running = jobs.isError ? 0 : (jobs.data ?? []).filter(job => !['review', 'failed', 'rejected', 'awaiting_confirmation'].includes(job.status)).length;
  const activeImport = !imports.isError && imports.data?.import && ['queued', 'running', 'retry'].includes(imports.data.import.status);
  const productionRunning = (!data.scheduleFailed && data.schedule?.jobs.some(job => job.status === 'running')) || (!data.clipsFailed && data.clips?.some(clip => clip.status === 'publishing')) || (!data.publishFailed && data.publish?.videos.some(video => video.event?.destinations.some(destination => ['preparing', 'effect_started', 'processing'].includes(destination.state))));
  return <section className="studio-activity">
    {(jobs.isError || imports.isError) && <p role="alert">{t('Aktivität nicht abrufbar')}</p>}
    {running > 0 && <button type="button" className="studio-button" onClick={onAi}>{t('Laufende KI-Aufträge: {count}', { count: running })}</button>}
    {activeImport && <button type="button" className="studio-button" onClick={() => onNavigate('planen')}>{t('Import läuft')}</button>}
    {!productionRunning && jobs.isSuccess && imports.isSuccess && running === 0 && !activeImport && <p>{t('Gerade läuft nichts.')}</p>}
  </section>;
}

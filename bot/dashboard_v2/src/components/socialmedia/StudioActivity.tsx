import { useQuery } from '@tanstack/react-query';
import { useT } from '@/context/LanguageContext';
import { fetchAiJobs } from './ai/api';
import { importPoolApi } from './autopilot/import-pool/api';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';

export function StudioActivity({ twitchUserId, data, now, onNavigate, onAi }: {
  twitchUserId?: string; data: StudioOverviewData; now: number;
  onNavigate: (view: SocialMediaViewTarget) => void; onAi: () => void;
}) {
  const t = useT();
  const jobs = useQuery({ queryKey: ['social-ai', 'jobs', twitchUserId], queryFn: ({ signal }) => fetchAiJobs(twitchUserId!, signal), enabled: Boolean(twitchUserId), retry: false, refetchInterval: 30000 });
  const imports = useQuery({ queryKey: ['social-media', 'activity-import', twitchUserId], queryFn: ({ signal }) => importPoolApi.getLatestImport(twitchUserId!, signal), enabled: Boolean(twitchUserId), retry: false, refetchInterval: 30000 });
  const running = jobs.isError ? 0 : (jobs.data ?? []).filter(job => !['review', 'failed', 'rejected', 'awaiting_confirmation'].includes(job.status)).length;
  const activeImport = !imports.isError && imports.data?.import && ['queued', 'running', 'retrying'].includes(imports.data.import.status);
  const expiring = data.platformsFailed ? [] : (data.platforms ?? []).filter(platform => {
    const expiry = platform.platform === 'instagram' || !platform.automatically_renewed ? platform.expires_at : platform.refresh_expires_at;
    return platform.connected && (platform.reauth_soon || (expiry && Date.parse(expiry) >= now && Date.parse(expiry) <= now + 7 * 86400000));
  });
  return <section className="studio-activity">
    {(jobs.isError || imports.isError) && <p role="alert">{t('Aktivität nicht abrufbar')}</p>}
    {running > 0 && <button type="button" className="studio-button" onClick={onAi}>{t('Laufende KI-Aufträge: {count}', { count: running })}</button>}
    {activeImport && <button type="button" className="studio-button" onClick={() => onNavigate('planen')}>{t('Import läuft')}</button>}
    {expiring.map(platform => <button key={platform.platform} type="button" className="studio-button" onClick={() => onNavigate('einstellungen')}>{t('Verbindung läuft bald ab: {platform}', { platform: platform.platform })}</button>)}
  </section>;
}

import { lazy, Suspense, useId, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowRight, Ban, Coins, Send } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import type { SocialClipMitPosting } from '@/api/socialMedia';
import type { ScheduleQueueSnapshot } from './autopilot/schedule-queue';
import type { PublishSnapshot } from './autopilot/publish-review/api';
import type { AiJobView } from './ai/api';
import { platformIconUrl } from './platformIcon';
import { DESTINATION_REASONS, HOLD_REASONS } from './reviewReasons';
import { WorkspaceDialog } from './WorkspaceDialog';
import type { SocialMediaViewTarget } from './labels';
import type { QueueStage } from './queuePresentation';

const AnalyticsTab = lazy(() => import('./AnalyticsTab').then(module => ({ default: module.AnalyticsTab })));

type Translate = (key: string, params?: Record<string, string | number>) => string;
type Navigate = (view: SocialMediaViewTarget, stage?: QueueStage) => void;

interface RecentItem { key: string; title: string; at: string | null; platforms: string[] }
interface DiscardedItem { key: string; title: string; reason: string }

function clipName(clips: SocialClipMitPosting[] | undefined, clipDbId: number, fallback: string): string {
  const clip = clips?.find(item => item.clip_db_id === clipDbId);
  return clip?.generated_title?.trim() || clip?.title || fallback;
}

function recentPublications(publish: PublishSnapshot | undefined, clips: SocialClipMitPosting[] | undefined, t: Translate): RecentItem[] {
  const produced = (publish?.videos ?? []).flatMap(video => {
    const platforms = video.event?.destinations.filter(destination => destination.state === 'published').map(destination => destination.platform) ?? [];
    return platforms.length === 0 ? [] : [{ key: `video-${video.id}`, title: clipName(clips, video.clip_id, t('Video {id}', { id: video.id })), at: video.event?.completed_at ?? video.event?.executed_at ?? null, platforms }];
  });
  const legacy = (clips ?? []).filter(clip => clip.status === 'published_all').map(clip => ({
    key: `clip-${clip.clip_db_id}`, title: clip.generated_title?.trim() || clip.title || clip.clip_id, at: null,
    platforms: Object.entries(clip.platform_status).filter(([, done]) => done).map(([platform]) => platform),
  }));
  return [...produced.sort((a, b) => Date.parse(b.at ?? '') - Date.parse(a.at ?? '')), ...legacy];
}

function reasonText(reasons: string[], labels: Record<string, string>, t: Translate): string | null {
  const known = reasons.map(reason => labels[reason]).filter((label): label is string => Boolean(label));
  return known.length > 0 ? known.map(label => t(label)).join(' ') : reasons[0] ?? null;
}

function discardedItems(schedule: ScheduleQueueSnapshot | undefined, publish: PublishSnapshot | undefined, clips: SocialClipMitPosting[] | undefined, locale: string, t: Translate): DiscardedItem[] {
  const jobs = (schedule?.jobs ?? []).filter(job => job.status === 'failed' || job.status === 'cancelled').map(job => ({
    key: `job-${job.id}`, title: job.clip_title || t('Clip {id}', { id: job.clip_id }),
    reason: reasonText(job.review_reasons, HOLD_REASONS, t) ?? (job.last_error ? t(job.last_error) : t(job.status === 'cancelled' ? 'Auftrag abgebrochen' : 'Produktion fehlgeschlagen')),
  }));
  const videos = (publish?.videos ?? []).flatMap(video => {
    const title = clipName(clips, video.clip_id, t('Video {id}', { id: video.id }));
    const own = ['rejected', 'failed'].includes(video.status) ? [{ key: `video-${video.id}`, title,
      reason: reasonText(video.review_reasons, HOLD_REASONS, t) ?? t(video.status === 'rejected' ? 'Video verworfen' : 'Video fehlgeschlagen') }] : [];
    const destinations = (video.event?.destinations ?? []).filter(destination => destination.state === 'discarded' || destination.state === 'failed').map(destination => ({
      key: `destination-${video.id}-${destination.platform}`, title,
      reason: destination.reason ? t(DESTINATION_REASONS[destination.reason] ?? destination.reason) : t(destination.state === 'failed' ? 'Veröffentlichung fehlgeschlagen' : 'Veröffentlichung verworfen'),
    }));
    return [...own, ...destinations];
  });
  const discarded = (clips ?? []).filter(clip => clip.discarded_at).map(clip => ({
    key: `clip-${clip.clip_db_id}`, title: clip.generated_title?.trim() || clip.title || clip.clip_id,
    reason: t('Aussortiert am {date}', { date: new Date(clip.discarded_at!).toLocaleDateString(locale) }),
  }));
  return [...jobs, ...videos, ...discarded];
}

function aiUsage(jobs: AiJobView[]) {
  return jobs.reduce((sum, job) => ({
    cost: sum.cost + (job.measured_provider_cost_usd ?? job.estimated_provider_cost_usd ?? 0),
    estimated: sum.estimated || (job.measured_provider_cost_usd === null && job.estimated_provider_cost_usd !== null),
    tokens: sum.tokens + (job.input_tokens ?? 0) + (job.output_tokens ?? 0),
    jobs: sum.jobs + 1,
  }), { cost: 0, estimated: false, tokens: 0, jobs: 0 });
}

function Tile({ icon: Icon, title, children, footer }: { icon: typeof Send; title: string; children: ReactNode; footer?: ReactNode }) {
  return <section className="studio-tile">
    <header className="studio-tile-head"><Icon className="h-4 w-4" aria-hidden /><h2>{title}</h2></header>
    <div className="studio-tile-body">{children}</div>
    {footer}
  </section>;
}

function RecentTile({ items, onNavigate }: { items: RecentItem[]; onNavigate: Navigate }) {
  const { t, locale } = useLanguage();
  return <Tile icon={Send} title={t('Zuletzt veröffentlicht')}
    footer={<button type="button" className="studio-overview-link" onClick={() => onNavigate('clips', 'posted')}>{t('Alle veröffentlichten Clips')}<ArrowRight className="h-4 w-4" aria-hidden /></button>}>
    {items.length === 0 ? <p className="studio-overview-empty">{t('Noch nichts veröffentlicht.')}</p>
      : <ul className="studio-tile-list">{items.slice(0, 3).map(item => <li key={item.key}>
        <span className="studio-tile-item-title" title={item.title}>{item.title}</span>
        <span className="studio-tile-item-meta">
          {item.platforms.map(platform => <img key={platform} className="studio-overview-platform" src={platformIconUrl(platform)} width="16" height="16" alt={platform} />)}
          {item.at && <span>{new Date(item.at).toLocaleDateString(locale, { day: '2-digit', month: '2-digit' })}</span>}
        </span>
      </li>)}</ul>}
  </Tile>;
}

function DiscardedTile({ items }: { items: DiscardedItem[] }) {
  const { t } = useLanguage();
  const id = useId();
  return <Tile icon={Ban} title={t('Von KI aussortiert')}>
    <p className="studio-tile-value">{items.length}</p>
    {items.length === 0 ? <p className="studio-overview-empty">{t('Nichts aussortiert.')}</p> : <>
      <button type="button" className="studio-overview-link" popoverTarget={id}>{t('Gründe ansehen')}<ArrowRight className="h-4 w-4" aria-hidden /></button>
      <div id={id} popover="auto" className="studio-import-popover studio-tile-popover">
        <h3>{t('Warum aussortiert wurde')}</h3>
        <ul className="studio-tile-reasons">{items.map(item => <li key={item.key}><strong title={item.title}>{item.title}</strong><span>{item.reason}</span></li>)}</ul>
      </div>
    </>}
  </Tile>;
}

function CostSummary({ owner }: { owner: string }) {
  const { t, locale } = useLanguage();
  const jobs = useQuery({ queryKey: ['social-ai', 'jobs', owner], queryFn: async ({ signal }) => (await import('./ai/api')).fetchAiJobs(owner, signal), retry: false, refetchInterval: 30000 });
  if (jobs.isError) return <p className="studio-overview-empty">{t('Nicht abrufbar')}</p>;
  if (!jobs.data) return <p className="studio-overview-empty">{t('Wird geladen')}</p>;
  const usage = aiUsage(jobs.data);
  return <>
    <p className="studio-tile-value">{new Intl.NumberFormat(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(usage.cost)}</p>
    <p className="studio-tile-detail">{t('{tokens} Tokens · {count} KI-Aufträge', { tokens: usage.tokens.toLocaleString(locale), count: usage.jobs })}</p>
    {usage.estimated && <p className="studio-tile-detail">{t('Teilweise geschätzt')}</p>}
  </>;
}

function CostTile({ owner, streamer, isAdmin, statisticsPlatforms }: { owner?: string; streamer: string; isAdmin: boolean; statisticsPlatforms: string[] }) {
  const { t } = useLanguage();
  const [open, setOpen] = useState(false);
  return <Tile icon={Coins} title={t('KI-Kosten & Nutzung')}
    footer={<button type="button" className="studio-overview-link" disabled={!streamer} onClick={() => setOpen(true)}>{t('Details')}<ArrowRight className="h-4 w-4" aria-hidden /></button>}>
    {owner ? <CostSummary owner={owner} /> : <p className="studio-overview-empty">{t('Bitte einen Kanal auswählen.')}</p>}
    {open && <WorkspaceDialog eyebrow={t('Performance')} title={t('Auswertung')} onClose={() => setOpen(false)}>
      <Suspense fallback={<p role="status">{t('Wird geladen')}</p>}>
        <AnalyticsTab streamer={streamer} twitchUserId={owner} isAdmin={isAdmin} statisticsPlatforms={statisticsPlatforms} />
      </Suspense>
    </WorkspaceDialog>}
  </Tile>;
}

export function StudioOverviewTiles({ schedule, publish, clips, owner, streamer, isAdmin, statisticsPlatforms, onNavigate }: {
  schedule: ScheduleQueueSnapshot | undefined;
  publish: PublishSnapshot | undefined;
  clips: SocialClipMitPosting[] | undefined;
  owner?: string;
  streamer: string;
  isAdmin: boolean;
  statisticsPlatforms: string[];
  onNavigate: Navigate;
}) {
  const { t, locale } = useLanguage();
  return <div className="studio-tiles">
    <RecentTile items={recentPublications(publish, clips, t)} onNavigate={onNavigate} />
    <DiscardedTile items={discardedItems(schedule, publish, clips, locale, t)} />
    <CostTile owner={owner} streamer={streamer} isAdmin={isAdmin} statisticsPlatforms={statisticsPlatforms} />
  </div>;
}

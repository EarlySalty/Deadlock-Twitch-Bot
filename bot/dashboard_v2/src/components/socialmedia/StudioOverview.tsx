import type { ComponentType, ReactNode } from 'react';
import { AlertTriangle, ArrowRight, CalendarClock, CircleDot, Clock3, ListOrdered, Plug, Power } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import type { PlatformStatus, SocialClipMitPosting } from '@/api/socialMedia';
import type { QueueJob, ScheduleQueueSnapshot } from './autopilot/schedule-queue';
import type { PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';
import { connectionFact } from './platformPresentation';
import { PLATFORM_LABELS, type SocialMediaView } from './labels';
import { formatTerminInZone, queueStage, queuedSlots, type QueueStage } from './queuePresentation';
import type { SocialPlatform } from '@/types/socialMedia';

type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'gold';
type IconComponent = ComponentType<{ className?: string; 'aria-hidden'?: boolean }>;

export interface StudioOverviewData {
  schedule: ScheduleQueueSnapshot | undefined;
  scheduleFailed: boolean;
  publish: PublishSnapshot | undefined;
  publishFailed: boolean;
  clips: SocialClipMitPosting[] | undefined;
  clipsFailed: boolean;
  platforms: PlatformStatus[] | undefined;
  platformsFailed: boolean;
  timezone: string;
}

const HOLDS = {
  empty_pool: 'Keine passenden Clips verfügbar',
  no_schedule: 'Wöchentliche Termine fehlen',
  review_backlog: 'Eine Prüfung ist offen',
  outside_lead: 'Nächster Termin außerhalb des Vorlaufs',
  paused: 'Neue Produktionen pausiert',
  disabled: 'Keine automatische Produktion',
};

function jobsWithStatus(schedule: ScheduleQueueSnapshot | undefined, status: QueueJob['status']): QueueJob[] {
  return (schedule?.jobs ?? []).filter(job => job.status === status);
}

function scheduledVideos(publish: PublishSnapshot | undefined): PublishVideo[] {
  return (publish?.videos ?? []).filter(video => video.event && !video.event.executed_at && !video.event.completed_at
    && video.artifact_state === 'present' && !['expired', 'rejected', 'failed'].includes(video.status)
    && (video.event.destinations.length === 0 || video.event.destinations.some(destination => ['pending', 'preparing', 'held'].includes(destination.state))))
    .sort((a, b) => Date.parse(a.event!.scheduled_at) - Date.parse(b.event!.scheduled_at));
}

function uploadPlatforms(platforms: PlatformStatus[] | undefined): PlatformStatus[] {
  return (platforms ?? []).filter(status => status.capabilities?.upload);
}

function platformsNeedingAttention(platforms: PlatformStatus[] | undefined): PlatformStatus[] {
  return uploadPlatforms(platforms).filter(status => connectionFact(status).state !== 'connected');
}

function countStage(clips: SocialClipMitPosting[] | undefined, stage: Exclude<QueueStage, 'all'>): number {
  return (clips ?? []).filter(clip => queueStage(clip) === stage).length;
}

function upcomingPosts(clips: SocialClipMitPosting[] | undefined, now: number) {
  return (clips ?? []).filter(clip => queuedSlots(clip) > 0).flatMap(clip =>
    Object.entries(clip.scheduled_at ?? {})
      .filter(([platform, at]) => Boolean(at) && !clip.platform_status[platform as SocialPlatform] && Date.parse(at as string) >= now)
      .map(([platform, at]) => ({ key: `${clip.clip_db_id}-${platform}`, title: clip.title || clip.clip_id, platform: platform as SocialPlatform, at: at as string })),
  ).sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

function StatusItem({ icon: Icon, label, value, detail, tone, onClick }: {
  icon: IconComponent;
  label: string;
  value: string;
  detail: string;
  tone: Tone;
  onClick: () => void;
}) {
  return <button type="button" className="studio-status-item" data-tone={tone} onClick={onClick}>
    <span className="studio-status-label-row"><Icon className="h-3.5 w-3.5" aria-hidden />{label}</span>
    <strong className="studio-status-value">{value}</strong>
    <span className="studio-status-detail" title={detail}>{detail}</span>
  </button>;
}

export function StudioStatusStrip({ data, onNavigate }: {
  data: StudioOverviewData;
  onNavigate: (view: SocialMediaView, stage?: QueueStage) => void;
}) {
  const { t, locale } = useLanguage();
  const schedule = data.scheduleFailed ? undefined : data.schedule;
  const platforms = data.platformsFailed ? undefined : data.platforms;
  const publish = data.publishFailed ? undefined : data.publish;
  const unavailable = (failed: boolean) => ({ value: failed ? '?' : '…', detail: t(failed ? 'Nicht abrufbar' : 'Wird geladen'), tone: 'neutral' as Tone });
  const settings = schedule?.settings;
  const hold = schedule?.production_hold;
  const autopilot = !settings ? unavailable(data.scheduleFailed)
    : !settings.enabled ? { value: t('Aus'), detail: t('Keine automatische Produktion'), tone: 'neutral' as Tone }
      : settings.paused ? { value: t('Pausiert'), detail: t('Laufende Aufträge werden fertig'), tone: 'warning' as Tone }
        : hold ? { value: t('An'), detail: t(HOLDS[hold]), tone: hold === 'outside_lead' ? 'neutral' as Tone : 'warning' as Tone }
          : { value: t('An'), detail: t('{count} Termine pro Woche', { count: settings.schedule_slots.length }), tone: 'success' as Tone };
  const running = jobsWithStatus(schedule, 'running').length;
  const queued = jobsWithStatus(schedule, 'queued').length;
  const productionReviews = schedule?.review_count ?? 0;
  const clipReviews = countStage(data.clips, 'review');
  const reviewsKnown = Boolean(schedule) && Boolean(data.clips) && !data.clipsFailed;
  const reviews = unavailable(data.scheduleFailed || data.clipsFailed);
  const terms = [schedule?.next_slot, ...jobsWithStatus(schedule, 'queued').map(job => job.scheduled_at),
    ...jobsWithStatus(schedule, 'running').map(job => job.scheduled_at), ...scheduledVideos(publish).map(video => video.event!.scheduled_at)]
    .filter((value): value is string => Boolean(value) && Number.isFinite(Date.parse(value!))).sort((a, b) => Date.parse(a) - Date.parse(b));
  const nextKnown = Boolean(schedule) && Boolean(publish);
  const nextUnavailable = unavailable(data.scheduleFailed || data.publishFailed);
  const uploads = uploadPlatforms(platforms);
  const attention = platformsNeedingAttention(platforms);
  return <section className="studio-status-strip" aria-label={t('Stand auf einen Blick')}>
    <StatusItem icon={Power} label={t('Auto-Pilot')} {...autopilot} onClick={() => onNavigate('plan')} />
    <StatusItem icon={CalendarClock} label={t('Nächster Termin')}
      value={!nextKnown ? nextUnavailable.value : terms[0] ? formatTerminInZone(terms[0], locale, schedule!.settings.timezone) : t('Keiner')}
      detail={!nextKnown ? nextUnavailable.detail : terms[0] ? schedule!.settings.timezone : t('Lege Termine im Zeitplan fest')}
      tone="neutral" onClick={() => onNavigate('plan')} />
    <StatusItem icon={ListOrdered} label={t('Warteschlange')}
      value={!schedule ? unavailable(data.scheduleFailed).value : String(running + queued)}
      detail={!schedule ? unavailable(data.scheduleFailed).detail : running > 0 ? t('{count} in Produktion', { count: running }) : t('Keine Produktion aktiv')}
      tone={running > 0 ? 'gold' : 'neutral'} onClick={() => onNavigate('plan')} />
    <StatusItem icon={AlertTriangle} label={t('Prüfungen offen')}
      value={reviewsKnown ? String(productionReviews + clipReviews) : reviews.value}
      detail={reviewsKnown ? t('{productions} Produktionen & Videos · {clips} Clip-Freigaben', { productions: productionReviews, clips: clipReviews }) : reviews.detail}
      tone={reviewsKnown && productionReviews + clipReviews > 0 ? 'warning' : 'neutral'}
      onClick={() => productionReviews > 0 || clipReviews === 0 ? onNavigate('plan') : onNavigate('pool', 'review')} />
    <StatusItem icon={Plug} label={t('Plattformen')}
      value={!platforms ? unavailable(data.platformsFailed).value : `${uploads.length - attention.length} / ${uploads.length}`}
      detail={!platforms ? unavailable(data.platformsFailed).detail : attention.length === 0 ? t('Alle verbunden') : t('{count} brauchen Aufmerksamkeit', { count: attention.length })}
      tone={!platforms ? 'neutral' : attention.length === 0 ? 'success' : 'warning'} onClick={() => onNavigate('konten')} />
  </section>;
}

interface OverviewRow {
  key: string;
  title: string;
  meta: string;
  tone?: Tone;
  onClick: () => void;
}

function OverviewColumn({ icon: Icon, title, count, rows, empty, footer }: {
  icon: IconComponent;
  title: string;
  count: number | null;
  rows: OverviewRow[];
  empty: string;
  footer?: ReactNode;
}) {
  return <section className="studio-overview-column">
    <header className="studio-overview-column-head"><Icon className="h-4 w-4" aria-hidden /><h2>{title}</h2>{count !== null && count > 0 && <span>{count}</span>}</header>
    {rows.length === 0 ? <p className="studio-overview-empty">{empty}</p> : <ul className="studio-overview-rows">{rows.map(row => <li key={row.key}>
      <button type="button" className="studio-overview-row" data-tone={row.tone ?? 'neutral'} onClick={row.onClick}>
        <span className="studio-overview-row-text"><strong title={row.title}>{row.title}</strong><span title={row.meta}>{row.meta}</span></span>
        <ArrowRight className="h-4 w-4" aria-hidden />
      </button>
    </li>)}</ul>}
    {footer}
  </section>;
}

export function PipelineOverview({ data, now, onNavigate }: {
  data: StudioOverviewData;
  now: number;
  onNavigate: (view: SocialMediaView, stage?: QueueStage) => void;
}) {
  const { t, locale } = useLanguage();
  const { schedule, publish, clips, platforms, timezone } = data;
  const zone = schedule?.settings.timezone ?? timezone;
  const when = (iso: string | null, timeZone = zone) => iso ? `${formatTerminInZone(iso, locale, timeZone)} · ${timeZone}` : t('Ohne Termin');
  const clipTitle = (job: QueueJob) => job.clip_title || t('Clip {id}', { id: job.clip_id });
  const videoTitle = (video: PublishVideo) => clips?.find(clip => clip.clip_db_id === video.clip_id)?.title || t('Video {id}', { id: video.id });
  const nowRows: OverviewRow[] = [
    ...jobsWithStatus(schedule, 'running').map(job => ({ key: `job-${job.id}`, title: clipTitle(job), meta: job.mode === 'ai' ? t('KI-Bearbeitung läuft') : t('Produktion läuft'), tone: 'gold' as Tone, onClick: () => onNavigate('plan') })),
    ...(clips ?? []).filter(clip => clip.status === 'publishing').map(clip => ({ key: `publish-${clip.clip_db_id}`, title: clip.title || clip.clip_id, meta: t('Wird veröffentlicht'), tone: 'gold' as Tone, onClick: () => onNavigate('pool', 'planned') })),
    ...(publish?.videos ?? []).filter(video => video.event?.destinations.some(destination => ['preparing', 'effect_started', 'processing'].includes(destination.state))).map(video => ({ key: `video-${video.id}`, title: videoTitle(video), meta: t('Wird veröffentlicht'), tone: 'gold' as Tone, onClick: () => onNavigate('plan') })),
  ];
  const productionReviews = schedule?.review_count ?? 0;
  const clipReviews = countStage(clips, 'review');
  const failedClips = countStage(clips, 'failed');
  const failedJobs = jobsWithStatus(schedule, 'failed').length;
  const actionRows: OverviewRow[] = [];
  if (schedule?.production_hold === 'empty_pool' || schedule?.production_hold === 'no_schedule')
    actionRows.push({ key: 'hold', title: t(HOLDS[schedule.production_hold]), meta: t('Der Auto-Pilot wartet auf deine Aktion'), tone: 'warning', onClick: () => onNavigate(schedule.production_hold === 'empty_pool' ? 'library' : 'plan') });
  if (productionReviews > 0)
    actionRows.push({ key: 'reviews', title: t('{count} Produktionen & Videos prüfen', { count: productionReviews }), meta: t('Auftrag oder Videofreigabe prüfen'), tone: 'warning', onClick: () => onNavigate('plan') });
  if (clipReviews > 0)
    actionRows.push({ key: 'clips', title: t('{count} Clips freigeben', { count: clipReviews }), meta: t('Freigabe aus der bisherigen Pipeline'), tone: 'warning', onClick: () => onNavigate('pool', 'review') });
  if (failedJobs > 0)
    actionRows.push({ key: 'jobs', title: t('{count} Produktionen fehlgeschlagen', { count: failedJobs }), meta: t('Auftrag prüfen oder entfernen'), tone: 'danger', onClick: () => onNavigate('plan') });
  if (failedClips > 0)
    actionRows.push({ key: 'failed', title: t('{count} Clips mit Fehler', { count: failedClips }), meta: t('Veröffentlichung erneut prüfen'), tone: 'danger', onClick: () => onNavigate('pool', 'failed') });
  for (const video of publish?.videos ?? [])
    if (video.event?.destinations.some(destination => ['retry', 'held', 'uncertain', 'failed'].includes(destination.state)))
      actionRows.push({ key: `publish-${video.id}`, title: videoTitle(video), meta: t('Veröffentlichung erneut prüfen'), tone: 'warning', onClick: () => onNavigate('plan') });
  for (const status of platformsNeedingAttention(platforms))
    actionRows.push({ key: `platform-${status.platform}`, title: PLATFORM_LABELS[status.platform as SocialPlatform] ?? status.platform, meta: t(connectionFact(status).label), tone: 'warning', onClick: () => onNavigate('konten') });

  const nextRows: OverviewRow[] = scheduledVideos(publish).slice(0, 4).map(video => ({ key: `video-${video.id}`, title: videoTitle(video), meta: t('Geplant für {time}', { time: when(video.event!.scheduled_at) }), tone: 'gold', onClick: () => onNavigate('plan') }));
  for (const job of jobsWithStatus(schedule, 'queued').slice(0, 3))
    nextRows.push({ key: `queued-${job.id}`, title: clipTitle(job), meta: t('Produktion für {time}', { time: when(job.scheduled_at) }), onClick: () => onNavigate('plan') });
  for (const post of upcomingPosts(clips, now).slice(0, 4))
    nextRows.push({ key: post.key, title: post.title, meta: `${PLATFORM_LABELS[post.platform]} · ${when(post.at, timezone)}`, onClick: () => onNavigate('pool', 'planned') });
  if (schedule?.settings.enabled && schedule.next_slot)
    nextRows.push({ key: 'slot', title: t('Nächster freier Auto-Pilot-Termin'), meta: when(schedule.next_slot), onClick: () => onNavigate('plan') });

  const failed = data.scheduleFailed || data.publishFailed || data.clipsFailed || data.platformsFailed;
  const loading = (!schedule || !publish || !clips || !platforms) && !failed;
  const unavailable = failed ? t('Nicht abrufbar') : loading ? t('Wird geladen') : null;
  return <div className="studio-overview" aria-busy={loading || undefined}>
    {failed && <p className="studio-overview-notice" role="status">{t('Die Übersicht ist nicht vollständig abrufbar. Bereits geladene Einträge bleiben sichtbar.')}</p>}
    <OverviewColumn icon={CircleDot} title={t('Jetzt')} count={nowRows.length} rows={nowRows} empty={unavailable ?? t('Gerade läuft nichts.')} />
    <OverviewColumn icon={AlertTriangle} title={t('Braucht dich')} count={actionRows.length} rows={actionRows} empty={unavailable ?? t('Nichts offen. Du bist auf dem aktuellen Stand.')} />
    <OverviewColumn icon={Clock3} title={t('Als Nächstes')} count={null} rows={nextRows} empty={unavailable ?? t('Noch nichts eingeplant.')}
      footer={<button type="button" className="studio-overview-link" onClick={() => onNavigate('plan')}>{t('Zeitplan öffnen')}<ArrowRight className="h-4 w-4" aria-hidden /></button>} />
  </div>;
}

import { studioActions } from './studioActions';
import { isValidElement, type ComponentType, type ReactNode } from 'react';
import { AlertTriangle, ArrowRight, CalendarClock, CircleDot, Clock3, ListOrdered, Plug, Power } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import type { PlatformStatus, SocialClipMitPosting } from '@/api/socialMedia';
import type { QueueJob, ScheduleQueueSnapshot } from './autopilot/schedule-queue';
import type { PublishReviewClient, PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';
import { ApprovalEntries } from './StudioApprovals';
import { approvalVideos } from './approvalVideos';
import { StudioOverviewTiles } from './StudioOverviewTiles';
import type { SocialPlatform } from '@/types/socialMedia';
import { upcomingTerms, upcomingTermView } from './upcomingTerms';
import { connectionFact } from './platformPresentation';
import { platformIconUrl } from './platformIcon';
import { PLATFORM_LABELS, type SocialMediaViewTarget } from './labels';
import { formatTerminInZone, queueStage, queuedPostings, type QueueStage } from './queuePresentation';

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
  return (clips ?? []).flatMap(clip => queuedPostings(clip)
    .filter(({ at }) => Date.parse(at) >= now)
    .map(({ platform, at }) => ({ key: `${clip.clip_db_id}-${platform}`, title: clip.title || clip.clip_id, platform, at, stage: queueStage(clip) })),
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
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
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
  const terms = [
    ...[schedule?.next_slot, ...jobsWithStatus(schedule, 'queued').map(job => job.scheduled_at),
      ...jobsWithStatus(schedule, 'running').map(job => job.scheduled_at)]
      .filter((value): value is string => Boolean(value))
      .map(at => ({ at, timezone: schedule?.settings.timezone ?? data.timezone, view: 'planen' as const })),
    ...scheduledVideos(publish).map(video => ({ at: video.event!.scheduled_at, timezone: schedule?.settings.timezone ?? data.timezone, view: 'clips' as const })),
    ...(data.clipsFailed ? [] : data.clips ?? []).flatMap(clip => queuedPostings(clip).map(({ at }) => ({ at, timezone: data.timezone, view: 'clips' as const }))),
  ].filter(term => Number.isFinite(Date.parse(term.at))).sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
  const nextKnown = Boolean(schedule) && Boolean(publish) && Boolean(data.clips) && !data.clipsFailed;
  const nextUnavailable = unavailable(data.scheduleFailed || data.publishFailed || data.clipsFailed);
  const uploads = uploadPlatforms(platforms);
  const attention = platformsNeedingAttention(platforms);
  return <section className="studio-status-strip" aria-label={t('Stand auf einen Blick')}>
    <StatusItem icon={Power} label={t('Auto-Pilot')} {...autopilot} onClick={() => onNavigate('planen')} />
    <StatusItem icon={CalendarClock} label={t('Nächster Termin')}
      value={!nextKnown ? nextUnavailable.value : terms[0] ? formatTerminInZone(terms[0].at, locale, terms[0].timezone) : t('Keiner')}
      detail={!nextKnown ? nextUnavailable.detail : terms[0] ? terms[0].timezone : t('Lege Termine im Zeitplan fest')}
      tone="neutral" onClick={() => onNavigate(terms[0]?.view ?? 'planen')} />
    <StatusItem icon={ListOrdered} label={t('Warteschlange')}
      value={!schedule ? unavailable(data.scheduleFailed).value : String(running + queued)}
      detail={!schedule ? unavailable(data.scheduleFailed).detail : running > 0 ? t('{count} in Produktion', { count: running }) : t('Keine Produktion aktiv')}
      tone={running > 0 ? 'gold' : 'neutral'} onClick={() => onNavigate('clips', 'preparing')} />
    <StatusItem icon={AlertTriangle} label={t('Prüfungen offen')}
      value={reviewsKnown ? String(productionReviews + clipReviews) : reviews.value}
      detail={reviewsKnown ? t('{productions} Produktionen & Videos · {clips} Clip-Freigaben', { productions: productionReviews, clips: clipReviews }) : reviews.detail}
      tone={reviewsKnown && productionReviews + clipReviews > 0 ? 'warning' : 'neutral'}
      onClick={() => onNavigate('clips', 'review')} />
    <StatusItem icon={Plug} label={t('Plattformen')}
      value={!platforms ? unavailable(data.platformsFailed).value : `${uploads.length - attention.length} / ${uploads.length}`}
      detail={!platforms ? unavailable(data.platformsFailed).detail : attention.length === 0 ? t('Alle verbunden') : t('{count} brauchen Aufmerksamkeit', { count: attention.length })}
      tone={!platforms ? 'neutral' : attention.length === 0 ? 'success' : 'warning'} onClick={() => onNavigate('einstellungen')} />
  </section>;
}

interface OverviewRow {
  key: string;
  title: string;
  meta: string;
  tone?: Tone;
  platform?: string;
  platforms?: string[];
  badge?: { label: string; mode: string };
  onClick: () => void;
}

function OverviewColumn({ icon: Icon, title, count, rows, empty, lead, footer, variant }: {
  icon: IconComponent;
  title: string;
  count: number | null;
  rows: OverviewRow[];
  empty: string;
  lead?: ReactNode;
  footer?: ReactNode;
  variant?: 'actions' | 'now' | 'next';
}) {
  return <section className="studio-overview-column" data-variant={variant}>
    <header className="studio-overview-column-head"><Icon className="h-4 w-4" aria-hidden /><h2>{title}</h2>{count !== null && count > 0 && <span>{count}</span>}</header>
    {lead}
    {rows.length === 0 ? (empty && !lead ? <p className="studio-overview-empty">{empty}</p> : null) : <ul className="studio-overview-rows" data-scroll={rows.length > 8 ? 'true' : undefined}>{rows.map(row => <li key={row.key}>
      <button type="button" className="studio-overview-row" data-tone={row.tone ?? 'neutral'} onClick={row.onClick}>
        {row.platform && <img className="studio-overview-platform" src={platformIconUrl(row.platform)} width="16" height="16" alt="" aria-hidden />}
        <span className="studio-overview-row-text"><strong title={row.title}>{row.title}</strong><span title={row.meta}>{row.meta}</span></span>
        {row.badge && <span className="studio-mode-badge" data-mode={row.badge.mode}>{row.badge.label}</span>}
        {row.platforms && row.platforms.length > 0 && <span className="studio-overview-platforms">{row.platforms.map(platform => <img key={platform} className="studio-overview-platform" src={platformIconUrl(platform)} width="16" height="16" alt={PLATFORM_LABELS[platform as SocialPlatform] ?? platform} />)}</span>}
        <ArrowRight className="h-4 w-4" aria-hidden />
      </button>
    </li>)}</ul>}
    {footer}
  </section>;
}

const PLATFORM_ICONS = new Set(['youtube', 'tiktok', 'instagram']);

function rowPlatform(key: string): string | undefined {
  const platform = key.startsWith('platform-') ? key.slice('platform-'.length) : undefined;
  return platform && PLATFORM_ICONS.has(platform) ? platform : undefined;
}

function HoldBanner({ row, onNavigate }: { row: OverviewRow & { hold: 'empty_pool' | 'no_schedule' }; onNavigate: (view: SocialMediaViewTarget) => void }) {
  const { t } = useLanguage();
  return <div className="studio-overview-banner" data-tone="warning" role="status">
    <AlertTriangle className="h-4 w-4" aria-hidden />
    <span className="studio-overview-banner-text"><strong>{row.title}</strong><span>{row.meta}</span></span>
    <button type="button" className="studio-button" onClick={() => onNavigate('planen')}>{t(row.hold === 'empty_pool' ? 'Clip-Vorrat öffnen' : 'Termin hinzufügen')}</button>
  </div>;
}

function LegacyClipsBanner({ count, onNavigate }: { count: number; onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void }) {
  const { t } = useLanguage();
  return <div className="studio-overview-banner" data-tone="warning" role="status">
    <AlertTriangle className="h-4 w-4" aria-hidden />
    <span className="studio-overview-banner-text"><strong>{t(count === 1 ? '{count} Clip aus der bisherigen Pipeline wartet auf Freigabe' : '{count} Clips aus der bisherigen Pipeline warten auf Freigabe', { count })}</strong></span>
    <button type="button" className="studio-button" onClick={() => onNavigate('clips', 'review')}>{t('Zu den Clips')}</button>
  </div>;
}

function termGroup(key: string, clipId: number | undefined, at: string): string {
  return clipId === undefined ? key : `${clipId}@${Date.parse(at)}`;
}

function ownerFromActivity(activity: ReactNode): string | undefined {
  return isValidElement<{ twitchUserId?: string }>(activity) ? activity.props.twitchUserId : undefined;
}

export function PipelineOverview({ data, now, onNavigate, activity, twitchUserId, streamer, isAdmin = false, statisticsPlatforms, publishClient }: {
  data: StudioOverviewData;
  now: number;
  activity?: ReactNode;
  onNavigate: (view: SocialMediaViewTarget, stage?: QueueStage) => void;
  twitchUserId?: string;
  streamer?: string;
  isAdmin?: boolean;
  statisticsPlatforms?: string[];
  publishClient?: PublishReviewClient;
}) {
  const { t, locale } = useLanguage();
  const { timezone } = data;
  const schedule = data.scheduleFailed ? undefined : data.schedule;
  const publish = data.publishFailed ? undefined : data.publish;
  const clips = data.clipsFailed ? undefined : data.clips;
  const platforms = data.platformsFailed ? undefined : data.platforms;
  const owner = twitchUserId ?? ownerFromActivity(activity);
  const zone = schedule?.settings.timezone ?? timezone;
  const when = (iso: string | null, timeZone = zone) => iso ? `${formatTerminInZone(iso, locale, timeZone)} · ${timeZone}` : t('Ohne Termin');
  const clipTitle = (job: QueueJob) => job.clip_title || t('Clip {id}', { id: job.clip_id });
  const videoTitle = (video: PublishVideo) => clips?.find(clip => clip.clip_db_id === video.clip_id)?.title || t('Video {id}', { id: video.id });
  const modeBadge = (mode: 'static' | 'ai' | null | undefined) => mode ? { label: t(mode === 'ai' ? 'KI-Schnitt' : 'Standard-Vorlage'), mode } : undefined;
  const nowRows: OverviewRow[] = [
    ...jobsWithStatus(schedule, 'running').map(job => ({ key: `job-${job.id}`, title: clipTitle(job), meta: job.mode === 'ai' ? t('KI-Bearbeitung läuft') : t('Produktion läuft'), tone: 'gold' as Tone, badge: modeBadge(job.mode ?? job.requested_mode), onClick: () => onNavigate('clips', 'preparing') })),
    ...(clips ?? []).filter(clip => clip.status === 'publishing').map(clip => ({ key: `publish-${clip.clip_db_id}`, title: clip.title || clip.clip_id, meta: t('Wird veröffentlicht'), tone: 'gold' as Tone, onClick: () => onNavigate('clips', queueStage(clip)) })),
    ...(publish?.videos ?? []).filter(video => video.event?.destinations.some(destination => ['preparing', 'effect_started', 'processing'].includes(destination.state))).map(video => ({ key: `video-${video.id}`, title: videoTitle(video), meta: t('Wird veröffentlicht'), tone: 'gold' as Tone, badge: modeBadge(video.mode), onClick: () => onNavigate('clips', 'planned') })),
  ];
  const actions: OverviewRow[] = studioActions(data, now, t).map(action => ({ ...action, platform: rowPlatform(action.key), onClick: () => onNavigate(action.view, action.stage) }));
  const holdRow = actions.find(row => row.key === 'hold');
  const hold = schedule?.production_hold === 'empty_pool' || schedule?.production_hold === 'no_schedule' ? schedule.production_hold : null;
  const legacyReviews = actions.some(row => row.key === 'clips') ? countStage(clips, 'review') : 0;
  const reviewsInPlan = schedule?.jobs.some(job => job.status === 'review') ?? false;
  const warningRows = actions.filter(row => !['hold', 'clips', 'video-reviews'].includes(row.key) && (row.key !== 'reviews' || reviewsInPlan));
  const approvals = approvalVideos(publish);

  const nextRows: OverviewRow[] = [];
  const nextGroups = new Map<string, OverviewRow>();
  const posts = upcomingPosts(clips, now);
  for (const term of upcomingTerms(data, now)) {
    const video = publish?.videos.find(video => `video-${video.id}` === term.key);
    const job = schedule?.jobs.find(job => `job-${job.id}` === term.key);
    const post = posts.find(post => post.key === term.key);
    const clipId = post ? Number(term.key.slice(0, term.key.indexOf('-'))) : video?.clip_id ?? job?.clip_id;
    const termPlatforms = post ? [post.platform] : video ? video.event?.destinations.map(destination => destination.platform) ?? [] : job ? (job.reservations ?? []).map(reservation => reservation.platform) : [];
    const group = termGroup(term.key, clipId, term.at);
    const existing = nextGroups.get(group);
    if (existing) {
      existing.platforms = [...new Set([...(existing.platforms ?? []), ...termPlatforms])];
      continue;
    }
    if (nextRows.length >= 8) continue;
    const row: OverviewRow = {
      key: term.key,
      title: video ? videoTitle(video) : job ? clipTitle(job) : post ? post.title : t('Nächster freier Auto-Pilot-Termin'),
      meta: when(term.at, term.timezone),
      platforms: [...new Set<string>(termPlatforms)],
      onClick: () => post ? onNavigate('clips', post.stage) : onNavigate(upcomingTermView(term.key)),
    };
    nextGroups.set(group, row);
    nextRows.push(row);
  }

  const failed = data.scheduleFailed || data.publishFailed || data.clipsFailed || data.platformsFailed;
  const loading = (!schedule || !publish || !clips || !platforms) && !failed;
  const unavailable = failed ? t('Nicht abrufbar') : loading ? t('Wird geladen') : null;
  const statistics = statisticsPlatforms ?? (platforms ?? []).filter(status => status.capabilities?.statistics).map(status => status.platform);
  const showHold = Boolean(holdRow && hold);
  const lead = showHold || legacyReviews > 0 || (publish && approvals.length > 0) ? <>
    {holdRow && hold && <HoldBanner row={{ ...holdRow, hold }} onNavigate={onNavigate} />}
    {legacyReviews > 0 && <LegacyClipsBanner count={legacyReviews} onNavigate={onNavigate} />}
    {publish && approvals.length > 0 && <ApprovalEntries videos={approvals} snapshot={publish} clips={clips} owner={owner} client={publishClient} timezone={zone} onNavigate={onNavigate} />}
  </> : undefined;
  return <div className="studio-overview" aria-busy={loading || undefined}>
    {failed && <p className="studio-overview-notice" role="status">{t('Die Übersicht ist nicht vollständig abrufbar. Bereits geladene Einträge bleiben sichtbar.')}</p>}
    <div className="studio-overview-grid">
      <OverviewColumn variant="actions" icon={AlertTriangle} title={t('Prüfung & Freigabe')} count={warningRows.length + approvals.length + (legacyReviews > 0 ? 1 : 0)} rows={warningRows} lead={lead}
        empty={unavailable ?? t('Nichts offen. Du bist auf dem aktuellen Stand.')} />
      <div className="studio-overview-side">
        <OverviewColumn variant="now" icon={CircleDot} title={t('Läuft gerade')} count={null} rows={nowRows} empty={activity ? '' : unavailable ?? t('Gerade läuft nichts.')} footer={activity} />
        <OverviewColumn variant="next" icon={Clock3} title={t('Als Nächstes')} count={null} rows={nextRows} empty={unavailable ?? t('Noch nichts eingeplant.')}
          footer={<button type="button" className="studio-overview-link" onClick={() => onNavigate('planen')}>{t('Zeitplan öffnen')}<ArrowRight className="h-4 w-4" aria-hidden /></button>} />
      </div>
    </div>
    <StudioOverviewTiles schedule={schedule} publish={publish} clips={clips} owner={owner} streamer={streamer ?? clips?.[0]?.streamer_login ?? ''} isAdmin={isAdmin} statisticsPlatforms={statistics} onNavigate={onNavigate} />
  </div>;
}

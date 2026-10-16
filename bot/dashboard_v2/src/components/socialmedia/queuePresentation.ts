import type { ClipListResponseMitPosting, SocialClipMitPosting } from '@/api/socialMedia';
import type { SocialPlatform } from '@/types/socialMedia';
import type { PipelineSource, SourceEligibility } from './pipelineApi';

export type QueueStage = 'all' | 'new' | 'review' | 'planned' | 'posted' | 'failed' | 'archive';
export const QUEUE_FILTERS: Array<{ id: QueueStage; label: string }> = [
  { id: 'all', label: 'Alle' },
  { id: 'new', label: 'Neu' },
  { id: 'review', label: 'Freigabe' },
  { id: 'planned', label: 'Geplant' },
  { id: 'posted', label: 'Gepostet' },
  { id: 'failed', label: 'Fehler' },
  { id: 'archive', label: 'Archiv' },
];

export function formatTerminInZone(iso: string, locale: string, timezone: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const options: Intl.DateTimeFormatOptions = {
    weekday: 'short', day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
  };
  try {
    return date.toLocaleString(locale, { ...options, timeZone: timezone });
  } catch {
    return date.toLocaleString(locale, options);
  }
}

export function queuedPostings(clip: SocialClipMitPosting): Array<{ platform: SocialPlatform; at: string }> {
  if (clip.status !== 'approved' && clip.status !== 'publishing') return [];
  return Object.entries(clip.scheduled_at ?? {}).flatMap(([platform, at]) =>
    at && !clip.platform_status[platform as SocialPlatform]
      && !['waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.[platform as SocialPlatform] ?? '')
      ? [{ platform: platform as SocialPlatform, at }] : [],
  );
}

export function queuedSlots(clip: SocialClipMitPosting): number {
  return queuedPostings(clip).length;
}

export function queueStage(clip: SocialClipMitPosting): Exclude<QueueStage, 'all'> {
  if (clip.status === 'discarded' || clip.status === 'skipped' || clip.discarded_at)
    return 'archive';
  if (clip.status === 'published_all') return 'posted';
  if (clip.status === 'failed' || clip.status === 'published_partial') return 'failed';
  if (['waiting_tiktok_approval', 'waiting_schedule'].includes(clip.upload_states?.tiktok ?? ''))
    return clip.status === 'approved' || clip.status === 'publishing' ? 'planned' : 'review';
  if (clip.status === 'publishing' || queuedSlots(clip) > 0) return 'planned';
  if (clip.status === 'awaiting_approval') return 'review';
  return 'new';
}

export function queueStats(clips: SocialClipMitPosting[]) {
  return {
    total: clips.length,
    awaitingApproval: clips.filter((c) => queueStage(c) === 'review').length,
    scheduled: clips.reduce((sum, c) => sum + queuedSlots(c), 0),
    failed: clips.filter((c) => queueStage(c) === 'failed').length,
  };
}

export function filterQueue(
  clips: SocialClipMitPosting[],
  stage: QueueStage,
  search: string,
  platform: SocialPlatform | 'all',
) {
  const needle = search.trim().toLocaleLowerCase();
  return clips.filter(
    (clip) =>
      (stage === 'all' || queueStage(clip) === stage) &&
      (!needle || `${clip.title} ${clip.generated_title ?? ''} ${clip.streamer_login}`.toLocaleLowerCase().includes(needle)) &&
      (platform === 'all' ||
        clip.platform_status[platform] ||
        Boolean(clip.scheduled_at?.[platform]) ||
        clip.approval?.approved_platforms.includes(platform)),
  );
}

export async function loadQueueSnapshot(
  loadPage: (page: number) => Promise<ClipListResponseMitPosting>,
  signal?: AbortSignal,
): Promise<ClipListResponseMitPosting> {
  const items = new Map<number, SocialClipMitPosting>();
  let total = 0;
  let restarts = 0;
  for (let page = 1; page <= 200; page += 1) {
    signal?.throwIfAborted();
    const result = await loadPage(page);
    signal?.throwIfAborted();
    total = Math.max(total, result.total);
    const previous = items.size;
    for (const clip of result.items) items.set(clip.clip_db_id, clip);
    if (items.size >= total)
      return { items: [...items.values()], total, page: 1, page_size: total };
    if (result.items.length === 0) throw new Error('queue_changed');
    if (result.items.length < result.page_size)
      return { items: [...items.values()], total, page: 1, page_size: total };
    if (items.size > previous) continue;
    if (restarts >= 1) throw new Error('queue_changed');
    restarts += 1;
    page = 0;
  }
  throw new Error('queue_too_large');
}

const ACTIVE_DESTINATIONS = ['pending', 'processing', 'waiting_connection', 'waiting_tiktok_approval', 'waiting_schedule', 'waiting_exact_file', 'inbox_pending'];
const DONE_DESTINATIONS = ['completed', 'inbox'];

export function publishedPlatforms(source: PipelineSource): string[] {
  return [...new Set([
    ...source.destinations.filter((destination) => DONE_DESTINATIONS.includes(destination.status ?? '')).map((destination) => destination.platform),
    ...source.legacy_publications.map((publication) => publication.platform),
  ])];
}

export function failedPlatforms(source: PipelineSource): string[] {
  const done = publishedPlatforms(source);
  return [...new Set(source.destinations
    .filter((destination) => destination.status === 'failed' && !done.includes(destination.platform))
    .map((destination) => destination.platform))];
}

export function sourcePlatforms(source: PipelineSource): string[] {
  return [...new Set([
    ...source.destinations.map((destination) => destination.platform),
    ...source.jobs.flatMap((job) => job.reservations.map((reservation) => reservation.platform)),
    ...source.legacy_publications.map((publication) => publication.platform),
  ])];
}

export function partialResult(source: PipelineSource): boolean {
  return publishedPlatforms(source).length > 0 && failedPlatforms(source).length > 0;
}

function sourceActive(source: PipelineSource): boolean {
  return source.destinations.some((destination) => ACTIVE_DESTINATIONS.includes(destination.status ?? 'pending'))
    || source.jobs.some((job) => job.status === 'queued' || job.status === 'running')
    || source.versions.some((version) => ['rendering', 'approved', 'scheduled'].includes(version.status));
}

function sourceInReview(source: PipelineSource): boolean {
  return source.jobs.some((job) => job.status === 'review') || source.versions.some((version) => version.status === 'review');
}

export function sourceStage(source: PipelineSource): Exclude<QueueStage, 'all'> {
  if (source.discarded_at) return 'archive';
  if (sourceActive(source)) return 'planned';
  if (sourceInReview(source)) return 'review';
  if (failedPlatforms(source).length > 0 || (publishedPlatforms(source).length === 0 && source.jobs.some((job) => job.status === 'failed'))) return 'failed';
  if (publishedPlatforms(source).length > 0 || source.published_at) return 'posted';
  return 'new';
}

export type EligibilityFilter = SourceEligibility | 'all';

export function filterSources(
  sources: PipelineSource[],
  filter: { eligibility: EligibilityFilter; stage: QueueStage; search: string; platform: SocialPlatform | 'all' },
  title: (source: PipelineSource) => string,
) {
  const needle = filter.search.trim().toLocaleLowerCase();
  return sources.filter((source) =>
    (filter.eligibility === 'all' || source.eligibility === filter.eligibility)
    && (filter.stage === 'all' || sourceStage(source) === filter.stage)
    && (filter.platform === 'all' || sourcePlatforms(source).includes(filter.platform))
    && (!needle || `${title(source)} ${source.clip_id}`.toLocaleLowerCase().includes(needle)));
}

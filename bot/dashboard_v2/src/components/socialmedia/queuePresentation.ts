import type { ClipListResponseMitPosting, SocialClipMitPosting } from '@/api/socialMedia';
import type { SocialPlatform } from '@/types/socialMedia';

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

export function queuedSlots(clip: SocialClipMitPosting): number {
  if (clip.status !== 'approved' && clip.status !== 'publishing') return 0;
  return Object.entries(clip.scheduled_at ?? {}).filter(
    ([platform, time]) => Boolean(time) && !clip.platform_status[platform as SocialPlatform],
  ).length;
}

export function queueStage(clip: SocialClipMitPosting): Exclude<QueueStage, 'all'> {
  if (clip.status === 'discarded' || clip.status === 'skipped' || clip.discarded_at)
    return 'archive';
  if (clip.status === 'published_all') return 'posted';
  if (clip.status === 'failed' || clip.status === 'published_partial') return 'failed';
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
      (!needle || `${clip.title} ${clip.streamer_login}`.toLocaleLowerCase().includes(needle)) &&
      (platform === 'all' ||
        clip.platform_status[platform] ||
        Boolean(clip.scheduled_at?.[platform]) ||
        clip.approval?.approved_platforms.includes(platform)),
  );
}

/** Vollständiger, kanalgebundener Bestand. Ein Abbruch oder eine verschobene
 * Pagination liefert einen Fehler statt irreführend kleiner Kennzahlen. */
export async function loadQueueSnapshot(
  loadPage: (page: number) => Promise<ClipListResponseMitPosting>,
  signal?: AbortSignal,
): Promise<ClipListResponseMitPosting> {
  const items = new Map<number, SocialClipMitPosting>();
  let total = 0;
  for (let page = 1; page <= 200; page += 1) {
    signal?.throwIfAborted();
    const result = await loadPage(page);
    signal?.throwIfAborted();
    if (page === 1) total = result.total;
    if (!Number.isSafeInteger(total) || total < 0 || result.total !== total)
      throw new Error('queue_changed');
    const previous = items.size;
    for (const clip of result.items) items.set(clip.clip_db_id, clip);
    if (items.size === total)
      return { items: [...items.values()], total, page: 1, page_size: total };
    if (items.size === previous || items.size > total || result.items.length < result.page_size)
      throw new Error('queue_changed');
  }
  throw new Error('queue_too_large');
}

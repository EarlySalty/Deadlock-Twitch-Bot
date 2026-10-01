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

/** Vollständiger, kanalgebundener Bestand. Kommt zwischen zwei Seitenabrufen
 * ein Clip dazu, verschieben sich die Offset-Seiten: Überlappungen und ein
 * gewachsenes total werden über Dedup nach clip_db_id toleriert, statt die
 * Ansicht abzubrechen. Abgebrochen wird erst, wenn eine Seite leer bleibt,
 * eine volle Seite keinen einzigen neuen Clip bringt (auch nach einem
 * Neuanlauf ab Seite 1) oder der 200-Seiten-Rahmen gesprengt wird. */
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
    // Volle Seite ohne Fortschritt: die Offset-Seiten sind am Bestand
    // vorbeigerutscht. Ein Neuanlauf ab Seite 1 verankert sie neu.
    if (restarts >= 1) throw new Error('queue_changed');
    restarts += 1;
    page = 0;
  }
  throw new Error('queue_too_large');
}

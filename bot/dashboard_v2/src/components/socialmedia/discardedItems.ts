import type { SocialClipMitPosting } from '@/api/socialMedia';
import type { ScheduleQueueSnapshot } from './autopilot/schedule-queue';
import type { PublishSnapshot } from './autopilot/publish-review/api';
import { DESTINATION_REASONS, HOLD_REASONS } from './reviewReasons';

type Translate = (key: string, params?: Record<string, string | number>) => string;

export interface DiscardedItem { key: string; title: string; reason: string }

export function clipName(clips: SocialClipMitPosting[] | undefined, clipDbId: number, fallback: string): string {
  const clip = clips?.find(item => item.clip_db_id === clipDbId);
  return clip?.generated_title?.trim() || clip?.title || fallback;
}

function reasonText(reasons: string[], labels: Record<string, string>, t: Translate): string | null {
  const known = reasons.map(reason => labels[reason]).filter((label): label is string => Boolean(label));
  return known.length > 0 ? known.map(label => t(label)).join(' ') : reasons[0] ?? null;
}

const MANUAL_DESTINATION_REASONS = new Set(['revision_replaced', 'not_approved_at_execution']);

function automaticDestinationDiscard(state: string, reason: string | null | undefined): boolean {
  return state === 'failed' || (state === 'discarded' && Boolean(reason) && !MANUAL_DESTINATION_REASONS.has(reason ?? ''));
}

export function discardedItems(schedule: ScheduleQueueSnapshot | undefined, publish: PublishSnapshot | undefined, clips: SocialClipMitPosting[] | undefined, t: Translate): DiscardedItem[] {
  const jobs = (schedule?.jobs ?? []).filter(job => job.status === 'failed').map(job => ({
    key: `job-${job.id}`, title: job.clip_title || t('Clip {id}', { id: job.clip_id }),
    reason: reasonText(job.review_reasons, HOLD_REASONS, t) ?? (job.last_error ? t(job.last_error) : t('Produktion fehlgeschlagen')),
  }));
  const videos = (publish?.videos ?? []).flatMap(video => {
    const title = clipName(clips, video.clip_id, t('Video {id}', { id: video.id }));
    const own = video.status === 'failed' ? [{ key: `video-${video.id}`, title,
      reason: reasonText(video.review_reasons, HOLD_REASONS, t) ?? t('Video fehlgeschlagen') }] : [];
    const destinations = (video.event?.destinations ?? []).filter(destination => automaticDestinationDiscard(destination.state, destination.reason)).map(destination => ({
      key: `destination-${video.id}-${destination.platform}`, title,
      reason: destination.reason ? t(DESTINATION_REASONS[destination.reason] ?? destination.reason) : t('Veröffentlichung fehlgeschlagen'),
    }));
    return [...own, ...destinations];
  });
  return [...jobs, ...videos];
}

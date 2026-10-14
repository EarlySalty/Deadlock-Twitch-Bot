import type { StudioOverviewData } from './StudioOverview';
import { queuedPostings } from './queuePresentation';

export function upcomingTerms(data: StudioOverviewData, now: number) {
  const schedule = data.scheduleFailed ? undefined : data.schedule;
  const timezone = schedule?.settings.timezone ?? data.timezone;
  return [
    ...(schedule?.next_slot ? [{ key: 'slot', at: schedule.next_slot, timezone }] : []),
    ...(schedule?.jobs ?? []).filter(job => ['queued', 'running'].includes(job.status) && job.scheduled_at)
      .map(job => ({ key: `job-${job.id}`, at: job.scheduled_at as string, timezone })),
    ...(data.publishFailed ? [] : data.publish?.videos ?? [])
      .filter(video => video.event && !video.event.executed_at && !video.event.completed_at
        && video.artifact_state === 'present' && !['expired', 'rejected', 'failed'].includes(video.status)
        && (video.event.destinations.length === 0 || video.event.destinations.some(destination => ['pending', 'preparing', 'held'].includes(destination.state))))
      .map(video => ({ key: `video-${video.id}`, at: video.event!.scheduled_at, timezone })),
    ...(data.clipsFailed ? [] : data.clips ?? []).flatMap(clip => queuedPostings(clip)
      .map(post => ({ key: `${clip.clip_db_id}-${post.platform}`, at: post.at, timezone: data.timezone }))),
  ].filter(term => Number.isFinite(Date.parse(term.at)) && Date.parse(term.at) >= now)
    .sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

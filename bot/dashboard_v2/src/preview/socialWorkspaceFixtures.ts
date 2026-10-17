import type { QueueJob, ScheduleQueueApi, ScheduleQueueSnapshot } from '../components/socialmedia/autopilot/schedule-queue/api';
import type { PublishSnapshot } from '../components/socialmedia/autopilot/publish-review/api';

export type PreviewWorkspaceState = 'ready' | 'loading' | 'error' | 'empty';

export function previewWorkspaceState(
  search: string,
  workspace: 'automation' | 'library' | 'ai',
): PreviewWorkspaceState {
  const fixture = new URLSearchParams(search).get('fixture');
  for (const state of ['loading', 'error', 'empty'] as const) {
    if (fixture === `${workspace}-${state}`) return state;
  }
  return 'ready';
}

function daysAhead(days: number, hour: number): string {
  const date = new Date();
  date.setUTCDate(date.getUTCDate() + days);
  date.setUTCHours(hour, 0, 0, 0);
  return date.toISOString();
}

function studioPreviewJobs(snapshot: ScheduleQueueSnapshot): QueueJob[] {
  const base = snapshot.jobs[0];
  const queued = snapshot.jobs.filter(job => job.status === 'queued').map((job, index) => {
    const at = daysAhead(index + 1, 16);
    return { ...job, scheduled_at: at, reservations: [{ platform: 'youtube', scheduled_at: at }, { platform: 'tiktok', scheduled_at: at }] };
  });
  return [
    ...snapshot.jobs.filter(job => job.status !== 'queued'),
    ...queued,
    { ...base, id: 105, clip_id: 2, clip_title: 'Bebop dreht den Teamfight am Markt', mode: 'ai', status: 'running', stage: 'analyzing', started_at: daysAhead(0, 6) },
    { ...base, id: 106, clip_id: 6, clip_title: 'Der nächste Versuch wartet auf die Verbindung', mode: 'static', status: 'failed', stage: 'failed', scheduled_at: null, last_error: 'Die Quelle ist nicht mehr abrufbar.' },
    { ...base, id: 107, clip_id: 3, clip_title: 'Ein ruhiger Moment vor dem nächsten Push', mode: null, status: 'cancelled', stage: 'cancelled', scheduled_at: null, started_at: null, review_reasons: ['unsuitable_clip'] },
  ];
}

export function withStudioPreviewJobs(api: ScheduleQueueApi): ScheduleQueueApi {
  return {
    ...api,
    load: async (owner, signal) => {
      const snapshot = await api.load(owner, signal);
      return { ...snapshot, jobs: studioPreviewJobs(snapshot) };
    },
  };
}

export function studioPreviewPublish(snapshot: PublishSnapshot): PublishSnapshot {
  return {
    ...snapshot,
    videos: snapshot.videos.map(video => video.status === 'review' && video.event
      ? { ...video, event: { ...video.event, scheduled_at: daysAhead(2, 17) } }
      : video),
  };
}

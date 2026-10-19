import type { QueueJob, ScheduleQueueApi, ScheduleQueueSnapshot, TestRun } from '../components/socialmedia/autopilot/schedule-queue/api';
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

function previewTestRun(kind: 'running' | 'failed' | 'ready'): TestRun {
  const base = {
    job_id: 900,
    clip_id: 45,
    clip_title: 'Testclip aus dem Vorrat',
    mode: 'static' as const,
    last_error: null as string | null,
    video_id: null as number | null,
    video_status: null as string | null,
  };
  if (kind === 'running') return { ...base, status: 'running', stage: 'downloading' };
  if (kind === 'failed') {
    return { ...base, status: 'failed', stage: 'failed', last_error: 'Clip fehlt auf Twitch.' };
  }
  return { ...base, status: 'review', stage: 'review', video_id: 1, video_status: 'review' };
}

export function withStudioPreviewJobs(api: ScheduleQueueApi): ScheduleQueueApi {
  const fixture = typeof window === 'undefined' ? null : new URLSearchParams(window.location.search).get('fixture');
  return {
    ...api,
    load: async (owner, signal) => {
      const snapshot = await api.load(owner, signal);
      const jobs = studioPreviewJobs(snapshot);
      if (fixture === 'idle') {
        return {
          ...snapshot,
          jobs,
          test_run: null,
          settings: { ...snapshot.settings, enabled: false, paused: false },
        };
      }
      if (fixture === 'test-running') return { ...snapshot, jobs, test_run: previewTestRun('running') };
      if (fixture === 'test-failed') return { ...snapshot, jobs, test_run: previewTestRun('failed') };
      if (fixture === 'test-ready') return { ...snapshot, jobs, test_run: previewTestRun('ready') };
      return { ...snapshot, jobs };
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

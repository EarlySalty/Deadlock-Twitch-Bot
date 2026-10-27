import type { PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';

export function approvalVideos(publish: PublishSnapshot | undefined): PublishVideo[] {
  return (publish?.videos ?? []).filter(video => video.status === 'review' && video.artifact_state === 'present')
    .sort((a, b) => dueAt(a) - dueAt(b) || a.id - b.id);
}

function dueAt(video: PublishVideo): number {
  const at = Date.parse(video.event?.scheduled_at ?? video.expires_at ?? '');
  return Number.isNaN(at) ? Number.MAX_SAFE_INTEGER : at;
}

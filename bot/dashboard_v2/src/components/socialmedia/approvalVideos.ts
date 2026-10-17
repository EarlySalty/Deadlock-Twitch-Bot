import type { PublishSnapshot, PublishVideo } from './autopilot/publish-review/api';

export function approvalVideos(publish: PublishSnapshot | undefined): PublishVideo[] {
  return (publish?.videos ?? []).filter(video => video.status === 'review' && video.artifact_state === 'present')
    .sort((a, b) => Date.parse(a.event?.scheduled_at ?? a.expires_at ?? '') - Date.parse(b.event?.scheduled_at ?? b.expires_at ?? ''));
}

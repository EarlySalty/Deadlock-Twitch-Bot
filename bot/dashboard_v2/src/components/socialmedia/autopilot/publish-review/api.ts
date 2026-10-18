import { fetchJson, withCookieCredentials } from '@/api/core';
import { fetchAuthStatus } from '@/api/auth';
import type { TikTokDraft } from '@/api/socialMedia';

export type PublishPlatform = 'youtube' | 'tiktok' | 'instagram';
export type PublishMode = 'review' | 'full_auto';
export type ReviewMode = 'review' | 'full_auto' | 'veto_until_deadline';
export type TikTokConsentState = 'missing' | 'bound' | 'stale';
export type PendingHandling = 'keep_review' | 'transfer_eligible';
export type DestinationStatus = 'pending' | 'preparing' | 'effect_started' | 'processing' | 'published' | 'retry' | 'held' | 'uncertain' | 'discarded' | 'failed';

export interface NotificationPreferences {
  dashboard: boolean;
  discord: boolean;
  email: boolean;
  ready_for_review: boolean;
  publication_deadline: boolean;
  processing_failed: boolean;
  publication_failed: boolean;
  empty_pool: boolean;
  review_expiring: boolean;
  reminder_hours: number;
}

export interface TikTokVersionOptions {
  caption: string;
  privacy_level: string;
  allow_comment: boolean;
  allow_duet: boolean;
  allow_stitch: boolean;
  commercial_content: boolean;
  brand_organic_toggle: boolean;
  brand_content_toggle: boolean;
  consent: boolean;
  creator_username: string;
  credential_id: number;
  platform_user_id: string;
  approved_video_sha256: string;
  video_path: string;
}

export interface DestinationDecision {
  platform: PublishPlatform;
  tiktok_options?: TikTokVersionOptions | null;
}

export interface PublishDestination {
  platform: PublishPlatform;
  state: DestinationStatus;
  attempts: number;
  next_retry_at: string | null;
  external_id: string | null;
  reason: string | null;
  account_id: string | null;
  visibility: string | null;
}

export interface PublicationEvent {
  id: number;
  produced_video_id: number;
  scheduled_at: string;
  executed_at: string | null;
  completed_at: string | null;
  destinations: PublishDestination[];
}

export interface PublishVideo {
  id: number;
  clip_id: number;
  version_no: number;
  variant_index: number;
  mode: 'static' | 'ai';
  status: 'rendering' | 'review' | 'approved' | 'scheduled' | 'published' | 'expired' | 'rejected' | 'failed';
  duration_ms: number | null;
  expires_at: string | null;
  needs_review: boolean;
  review_reasons: string[];
  tiktok_consent?: TikTokConsentState;
  artifact_state: 'present' | 'deleted' | 'missing' | 'error';
  cleanup_error: string | null;
  approvals: { platform: PublishPlatform; state: string; approver_user_id: string | null; decided_at: string | null }[];
  event: PublicationEvent | null;
  test_job_id?: number | null;
}

export interface TikTokVersionContext {
  creator: {
    creator_username: string;
    creator_nickname: string;
    privacy_level_options: string[];
    comment_disabled: boolean;
    duet_disabled: boolean;
    stitch_disabled: boolean;
    max_video_post_duration_sec: number;
  };
  caption: string;
  duration_seconds: number;
  approved_video_sha256: string;
  credential_id: number;
  platform_user_id: string;
  produced_video_id: number;
  music_consent: boolean;
  draft: TikTokDraft | null;
  defaults: TikTokDraft | null;
  bound_options: TikTokVersionOptions | null;
}

export function fetchTikTokVersion(clipDbId: number, videoId: number, signal?: AbortSignal): Promise<TikTokVersionContext> {
  return fetchJson(`/twitch/social-media/api/clips/${clipDbId}/tiktok/creator-info?${new URLSearchParams({ produced_video_id: String(videoId) })}`, withCookieCredentials({ cache: 'no-store', signal }));
}

export interface PublishSnapshot {
  settings: { publish_mode: PublishMode; review_mode?: ReviewMode; paused: boolean; notifications: NotificationPreferences };
  target_platforms: PublishPlatform[];
  connections: { platform: PublishPlatform; connected: boolean; account_id: string | null }[];
  videos: PublishVideo[];
  notifications: { id: number; kind: string; content: string; created_at: string; discord_state: string; read_at: string | null }[];
  capabilities: { discord: boolean; email: false; tiktok_effect_boundary: boolean };
}

export interface PublishReviewClient {
  load(owner: string, signal?: AbortSignal): Promise<PublishSnapshot>;
  transition(owner: string, input: { mode: PublishMode; confirmed: true; pending?: PendingHandling }): Promise<void>;
  pause(owner: string, paused: boolean): Promise<void>;
  preferences(owner: string, input: NotificationPreferences): Promise<void>;
  approve(owner: string, videoId: number, decisions: DestinationDecision[]): Promise<void>;
  schedule(owner: string, videoId: number, platforms: PublishPlatform[]): Promise<void>;
  retry(owner: string, videoId: number, platform: PublishPlatform): Promise<void>;
  videoUrl?(owner: string, videoId: number): string;
}

const base = '/twitch/social-media/api/autopilot/publish';

function publishUrl(path: string): string {
  return `${base}${path}`;
}

async function request<T>(owner: string, url: string, method = 'GET', body?: unknown, signal?: AbortSignal): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (method !== 'GET') {
    const auth = await fetchAuthStatus();
    const csrf = auth?.csrfToken ?? auth?.csrf_token;
    if (csrf) headers['X-CSRF-Token'] = csrf;
    headers['Content-Type'] = 'application/json';
  }
  return fetchJson<T>(`${url}?${new URLSearchParams({ twitch_user_id: owner })}`, withCookieCredentials({ method, headers, signal, body: body === undefined ? undefined : JSON.stringify(body) }));
}

export const publishReviewClient: PublishReviewClient = {
  load: (owner, signal) => request(owner, publishUrl(''), 'GET', undefined, signal),
  transition: (owner, input) => request(owner, publishUrl('/mode'), 'PUT', input),
  pause: (owner, paused) => request(owner, publishUrl('/pause'), 'PUT', { paused }),
  preferences: (owner, input) => request(owner, publishUrl('/notifications'), 'PUT', input),
  approve: (owner, video, destinations) => request(owner, publishUrl(`/videos/${video}/approval`), 'PUT', { destinations }),
  schedule: (owner, video, platforms) => request(owner, publishUrl(`/videos/${video}/schedule`), 'POST', { platforms }),
  retry: (owner, video, platform) => request(owner, publishUrl(`/videos/${video}/retry`), 'POST', { platform }),
  videoUrl: (owner, video) => `${publishUrl(`/videos/${video}/file`)}?${new URLSearchParams({ twitch_user_id: owner })}`,
};

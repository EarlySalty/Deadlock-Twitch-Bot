import { fetchJson, withCookieCredentials } from '@/api/core';

export interface WeeklySlot {
  weekday: number;
  local_time: string;
}

export interface ScheduleSettings {
  enabled: boolean;
  paused: boolean;
  ai_editing_enabled: boolean;
  lead_time_hours: number;
  timezone: string;
  schedule_slots: WeeklySlot[];
  target_platforms: string[];
}

export type SchedulePatch = Partial<ScheduleSettings>;
export type PipelineMode = 'upload' | 'upload_text' | 'ai_edit';
export type ReviewPolicy = 'off' | 'required' | 'veto_until_deadline';
export type PolicyScope = 'shared' | 'per_platform';
export type DistributionPolicy = 'same_clip' | 'independent_clips';

export interface TextDefaults {
  title: string;
  description: string;
  hashtags: string[];
}

export interface PipelinePolicy {
  pipeline_mode: PipelineMode;
  review_policy: ReviewPolicy;
  metadata_scope: PolicyScope;
  text_defaults: { shared: TextDefaults; per_platform: Record<string, TextDefaults> };
  schedule_scope: PolicyScope;
  platform_schedule_slots: Record<string, WeeklySlot[]>;
  distribution_policy: DistributionPolicy;
  policy_revision: number;
}

export interface Reservation {
  platform: string;
  scheduled_at: string;
}
export type ProductionHold = 'disabled' | 'paused' | 'review_backlog' | 'empty_pool' | 'no_schedule' | 'outside_lead' | null;

export interface QueueJob {
  id: number;
  clip_id: number;
  clip_title: string;
  source: 'autopilot' | 'manual';
  mode: 'static' | 'ai' | null;
  requested_mode: 'static' | 'ai' | null;
  status: 'queued' | 'running' | 'review' | 'completed' | 'failed' | 'cancelled';
  stage: string;
  attempts: number;
  max_attempts: number;
  scheduled_at: string | null;
  reservations?: Reservation[];
  started_at: string | null;
  last_error: string | null;
  review_reasons: string[];
  review_explanation: string | null;
}

export interface ScheduleQueueSnapshot {
  settings: ScheduleSettings;
  persisted: boolean;
  schedule_scope?: PolicyScope;
  distribution_policy?: DistributionPolicy;
  next_slot: string | null;
  next_slots?: Record<string, string>;
  jobs: QueueJob[];
  review_count: number;
  production_hold: ProductionHold;
  connected_platforms: string[];
}

export type ReviewDecision = 'process' | 'skip';

export interface ScheduleQueueApi {
  load(twitchUserId?: string, signal?: AbortSignal): Promise<ScheduleQueueSnapshot>;
  save(patch: SchedulePatch, twitchUserId?: string): Promise<ScheduleQueueSnapshot>;
  reorder(jobIds: number[], twitchUserId?: string): Promise<ScheduleQueueSnapshot>;
  next(jobId: number, twitchUserId?: string): Promise<ScheduleQueueSnapshot>;
  cancel(jobId: number, twitchUserId?: string): Promise<ScheduleQueueSnapshot>;
  resolve(jobId: number, action: ReviewDecision, note: string, twitchUserId?: string): Promise<ScheduleQueueSnapshot>;
  loadPolicy(twitchUserId?: string, signal?: AbortSignal): Promise<PipelinePolicy>;
  savePolicy(policy: PipelinePolicy, twitchUserId?: string): Promise<PipelinePolicy>;
}

const PREFIX = '/twitch/social-media/api/autopilot';

export const scheduleQueueApi: ScheduleQueueApi = {
  load(twitchUserId, signal) {
    const query = new URLSearchParams();
    if (twitchUserId) query.set('twitch_user_id', twitchUserId);
    return fetchJson(`${PREFIX}/schedule${query.size ? `?${query}` : ''}`, withCookieCredentials({ signal }));
  },
  save(patch, twitchUserId) {
    return fetchJson(`${PREFIX}/schedule`, withCookieCredentials({
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId, ...patch }),
    }));
  },
  reorder(jobIds, twitchUserId) {
    return fetchJson(`${PREFIX}/queue/order`, withCookieCredentials({
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId, job_ids: jobIds }),
    }));
  },
  next(jobId, twitchUserId) {
    return fetchJson(`${PREFIX}/queue/${jobId}/next`, withCookieCredentials({
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId }),
    }));
  },
  resolve(jobId, action, note, twitchUserId) {
    return fetchJson(`${PREFIX}/queue/${jobId}/resolve`, withCookieCredentials({
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId, action, note }),
    }));
  },
  loadPolicy(twitchUserId, signal) {
    const query = new URLSearchParams();
    if (twitchUserId) query.set('twitch_user_id', twitchUserId);
    return fetchJson(`${PREFIX}/policy${query.size ? `?${query}` : ''}`, withCookieCredentials({ signal }));
  },
  savePolicy(policy, twitchUserId) {
    const query = new URLSearchParams();
    if (twitchUserId) query.set('twitch_user_id', twitchUserId);
    return fetchJson(`${PREFIX}/policy${query.size ? `?${query}` : ''}`, withCookieCredentials({
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(policy),
    }));
  },
  cancel(jobId, twitchUserId) {
    return fetchJson(`${PREFIX}/queue/${jobId}/cancel`, withCookieCredentials({
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId }),
    }));
  },
};

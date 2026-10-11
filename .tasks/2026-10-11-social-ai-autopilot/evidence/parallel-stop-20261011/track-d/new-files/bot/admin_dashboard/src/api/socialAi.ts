import { admin, postAdminJson } from '@/api/client';

export type UsageGrouping = 'user' | 'provider' | 'job';

export interface UsageTotals {
  attempts: number;
  incomplete_attempts: number;
  unknown_cost_attempts: number;
  input_tokens: number | null;
  output_tokens: number | null;
  cache_creation_input_tokens: number | null;
  cache_read_input_tokens: number | null;
  estimated_micro_usd: number | null;
  measured_micro_usd: number | null;
  held_micro_usd: number | null;
  provider_latency_ms: number | null;
}

export interface UsageGroup extends UsageTotals {
  twitch_user_id: string | null;
  provider: string | null;
  model: string | null;
  job_id: string | null;
  processing_ms: number | null;
  rendering_ms: number | null;
}

export interface UsageReport {
  totals: UsageTotals;
  groups: UsageGroup[];
  group_by: UsageGrouping;
  offset: number;
  has_more: boolean;
}

export interface ProcessingControl {
  revision: number;
  paused: boolean;
  initial_total_cap_micro_usd: number;
  updated_at: string;
  updated_by: string;
  reason: string;
}

export interface UserLimit {
  twitch_user_id: string;
  revision: number;
  paused: boolean;
  monthly_cap_micro_usd: number | null;
  updated_at: string;
  updated_by: string;
  reason: string;
}

export interface ProcessingLimits {
  global: ProcessingControl;
  users: UserLimit[];
  plan_policies_active: false;
}

export const fetchUsage = (groupBy: UsageGrouping, offset = 0) =>
  admin<UsageReport>(`/social-ai/usage?group_by=${groupBy}&offset=${offset}&limit=25`, { cache: 'no-store' });

export const fetchLimits = () => admin<ProcessingLimits>('/social-ai/limits', { cache: 'no-store' });

export const saveGlobalControl = (body: {
  expected_revision: number;
  paused: boolean;
  initial_total_cap_micro_usd: number;
  reason: string;
}) => postAdminJson<ProcessingLimits>('/social-ai/limits/global', body);

export const saveUserLimit = (body: {
  twitch_user_id: string;
  expected_revision: number;
  paused: boolean;
  monthly_cap_micro_usd: number | null;
  reason: string;
}) => postAdminJson<ProcessingLimits>('/social-ai/limits/user', body);

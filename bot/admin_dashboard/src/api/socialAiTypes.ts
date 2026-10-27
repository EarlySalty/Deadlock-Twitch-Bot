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
  invoiced_micro_usd: number | null;
  held_micro_usd: number | null;
  charged_micro_usd: number | null;
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

export interface UsageFilter {
  group_by: UsageGrouping;
  offset: number;
  twitch_user_id: string;
  provider: string;
  job_id: string;
}

export interface StageUsage extends UsageTotals {
  stage: 'analysis' | 'planning' | 'other';
  run: 'first_run' | 'revision';
}

export interface UsageReport {
  totals: UsageTotals;
  stages: StageUsage[];
  groups: UsageGroup[];
  group_by: UsageGrouping;
  offset: number;
  has_more: boolean;
  period_start: string | null;
  period_end: string | null;
}

export interface ProcessingControl {
  revision: number;
  paused: boolean;
  initial_total_cap_micro_usd: 20000000;
  monthly_cap_micro_usd: number | null;
  updated_at: string;
  updated_by: string;
  reason: string;
}

export interface UserLimit {
  twitch_user_id: string;
  revision: number;
  paused: boolean;
  monthly_cap_micro_usd: number | null;
  monthly_ai_jobs: number | null;
  monthly_revisions: number | null;
  updated_at: string;
  updated_by: string;
  reason: string;
}

export interface PlanLimit {
  plan_id: string;
  name: string;
  revision: number;
  monthly_cap_micro_usd: number | null;
  monthly_ai_jobs: number | null;
  monthly_revisions: number | null;
  reason: string;
}

export interface ProcessingLimits {
  enforcement_available: boolean;
  plan_configuration_available: boolean;
  plan_policies_active: false;
  global: ProcessingControl;
  users: UserLimit[];
  plans: PlanLimit[];
}

export interface GlobalControlChange {
  expected_revision: number;
  paused: boolean;
  monthly_cap_micro_usd: number | null;
  reason: string;
}

export interface UserLimitChange extends GlobalControlChange {
  twitch_user_id: string;
  monthly_ai_jobs: number | null;
  monthly_revisions: number | null;
}

export interface PlanLimitChange {
  plan_id: string;
  expected_revision: number;
  monthly_cap_micro_usd: number | null;
  monthly_ai_jobs: number | null;
  monthly_revisions: number | null;
  reason: string;
}

export interface SkillDiffLine {
  kind: 'added' | 'removed' | 'context';
  text: string;
}

export interface SkillVersion {
  version: string;
  content_hash: string | null;
  previous_version: string | null;
  changed_at: string;
  summary: string;
  reason: string;
  evidence: { summary: string; sample_count: number | null } | null;
  diff: SkillDiffLine[];
}

export interface SkillActivation {
  kind: 'learned' | 'rollback' | 'auto_rollback';
  from_version: string;
  to_version: string;
  actor: string;
  reason: string;
  created_at: string;
}

export interface SkillHistory {
  skill_id: string;
  name: string;
  current_version: string;
  versions: SkillVersion[];
  activations: SkillActivation[];
}

export interface ChangeProposal {
  id: string;
  title: string;
  created_at: string;
  summary: string;
  reason: string;
  status: 'proposed' | 'reviewed' | 'dismissed';
  evidence: string | null;
  reviewed_at: string | null;
  reviewed_by: string | null;
}

export interface SkillsReport {
  rollback_available: boolean;
  skills: SkillHistory[];
  proposals: ChangeProposal[];
}

export interface RollbackChange {
  expected_version: string;
  target_version: string;
  reason: string;
}

import type { ProcessingLimits, SkillsReport, UsageGrouping, UsageReport } from './socialAiTypes.ts';

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Ungültige Serverantwort. Bitte den Stand neu laden.');
  return value as Record<string, unknown>;
}

function string(value: unknown): value is string {
  return typeof value === 'string';
}

function count(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function nullableCount(value: unknown): boolean {
  return value === null || count(value);
}

function rows(value: unknown): Record<string, unknown>[] {
  if (!Array.isArray(value)) throw new Error('Unvollständige Serverantwort. Bitte den Stand neu laden.');
  return value.map(record);
}

function requireValid(valid: boolean): void {
  if (!valid) throw new Error('Die Serverantwort passt nicht zur KI-Verwaltung. Es wurde kein bestätigter Stand übernommen.');
}

function requireTotals(data: Record<string, unknown>): void {
  requireValid(count(data.attempts) && count(data.incomplete_attempts) && count(data.unknown_cost_attempts));
  requireValid(Number(data.incomplete_attempts) <= Number(data.attempts) && Number(data.unknown_cost_attempts) <= Number(data.attempts));
  for (const key of ['input_tokens', 'output_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens', 'estimated_micro_usd', 'measured_micro_usd', 'invoiced_micro_usd', 'held_micro_usd', 'charged_micro_usd', 'provider_latency_ms']) requireValid(nullableCount(data[key]));
}

export function requireUsage(value: unknown, grouping: UsageGrouping, offset: number): UsageReport {
  const data = record(value);
  requireValid(data.group_by === grouping && data.offset === offset && typeof data.has_more === 'boolean');
  requireValid((data.period_start === null || string(data.period_start)) && (data.period_end === null || string(data.period_end)));
  requireTotals(record(data.totals));
  for (const group of rows(data.groups)) {
    requireTotals(group);
    for (const key of ['twitch_user_id', 'provider', 'model', 'job_id']) requireValid(group[key] === null || string(group[key]));
    requireValid(nullableCount(group.processing_ms) && nullableCount(group.rendering_ms));
  }
  return data as unknown as UsageReport;
}

export function requireLimits(value: unknown): ProcessingLimits {
  const data = record(value);
  requireValid(typeof data.enforcement_available === 'boolean' && typeof data.plan_configuration_available === 'boolean' && data.plan_policies_active === false);
  const global = record(data.global);
  requireValid(count(global.revision) && global.initial_total_cap_micro_usd === 2_000_000 && typeof global.paused === 'boolean' && string(global.updated_at) && string(global.updated_by) && string(global.reason));
  const users = rows(data.users);
  const userIds = new Set<unknown>();
  for (const user of users) {
    requireValid(string(user.twitch_user_id) && /^\d+$/.test(user.twitch_user_id) && !userIds.has(user.twitch_user_id) && count(user.revision) && typeof user.paused === 'boolean' && nullableCount(user.monthly_cap_micro_usd) && string(user.updated_at) && string(user.updated_by) && string(user.reason));
    userIds.add(user.twitch_user_id);
    requireValid(user.monthly_cap_micro_usd === null || Number(user.monthly_cap_micro_usd) <= 2_000_000);
  }
  const planIds = new Set<unknown>();
  for (const plan of rows(data.plans)) {
    requireValid(string(plan.plan_id) && !!plan.plan_id && !planIds.has(plan.plan_id) && string(plan.name) && count(plan.revision) && string(plan.reason));
    planIds.add(plan.plan_id);
    for (const key of ['monthly_cap_micro_usd', 'monthly_ai_jobs', 'monthly_revisions']) requireValid(nullableCount(plan[key]));
    requireValid(plan.monthly_cap_micro_usd === null || Number(plan.monthly_cap_micro_usd) <= 2_000_000);
  }
  return data as unknown as ProcessingLimits;
}

export function requireSkills(value: unknown): SkillsReport {
  const data = record(value);
  requireValid(typeof data.rollback_available === 'boolean');
  const skills = rows(data.skills);
  const skillIds = new Set<unknown>();
  for (const skill of skills) {
    requireValid(string(skill.skill_id) && !!skill.skill_id && !skillIds.has(skill.skill_id) && string(skill.name) && string(skill.current_version));
    skillIds.add(skill.skill_id);
    const versions = rows(skill.versions);
    const versionIds = new Set<unknown>();
    for (const version of versions) {
      requireValid(string(version.version) && !!version.version && !versionIds.has(version.version) && (version.previous_version === null || string(version.previous_version)) && string(version.changed_at) && string(version.summary) && string(version.reason));
      versionIds.add(version.version);
      requireValid(version.content_hash === null || (string(version.content_hash) && version.content_hash.trim().length > 0));
      if (version.evidence !== null) {
        const evidence = record(version.evidence);
        requireValid(string(evidence.summary) && nullableCount(evidence.sample_count));
      }
      for (const line of rows(version.diff)) requireValid(['added', 'removed', 'context'].includes(String(line.kind)) && string(line.text));
    }
    requireValid(versions.length === 0 || versionIds.has(skill.current_version));
  }
  for (const proposal of rows(data.proposals)) {
    requireValid(string(proposal.id) && string(proposal.title) && string(proposal.created_at) && string(proposal.summary) && string(proposal.reason) && ['proposed', 'reviewed', 'dismissed'].includes(String(proposal.status)) && (proposal.evidence === null || string(proposal.evidence)));
  }
  return data as unknown as SkillsReport;
}

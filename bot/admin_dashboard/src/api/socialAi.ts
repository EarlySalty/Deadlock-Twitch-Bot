import { admin, postAdminJson } from '@/api/client';
import { requireLimits, requireSkills, requireUsage } from './socialAiValidation';
import type { GlobalControlChange, PlanLimitChange, RollbackChange, UsageFilter, UserLimitChange } from './socialAiTypes';

export const socialAiKeys = {
  skills: ['social-ai', 'skills'] as const,
  limits: ['social-ai', 'limits'] as const,
  usage: (filter: UsageFilter) => ['social-ai', 'usage', filter] as const,
};

export async function fetchUsage(filter: UsageFilter, signal?: AbortSignal) {
  const query = new URLSearchParams({ group_by: filter.group_by, offset: String(filter.offset), limit: '25' });
  for (const key of ['twitch_user_id', 'provider', 'job_id'] as const) {
    if (filter[key].trim()) query.set(key, filter[key].trim());
  }
  return requireUsage(await admin<unknown>(`/social-ai/usage?${query}`, { cache: 'no-store', signal }), filter.group_by, filter.offset);
}

export async function fetchLimits(signal?: AbortSignal) {
  return requireLimits(await admin<unknown>('/social-ai/limits', { cache: 'no-store', signal }));
}

export async function saveGlobalControl(body: GlobalControlChange) {
  const data = requireLimits(await postAdminJson<unknown, GlobalControlChange>('/social-ai/limits/global', body));
  if (data.global.revision <= body.expected_revision || data.global.paused !== body.paused || data.global.monthly_cap_micro_usd !== body.monthly_cap_micro_usd) throw new Error('Die Änderung wurde nicht bestätigt. Bitte den Stand neu laden.');
  return data;
}

export async function saveUserLimit(body: UserLimitChange) {
  const data = requireLimits(await postAdminJson<unknown, UserLimitChange>('/social-ai/limits/user', body));
  const user = data.users.find(user => user.twitch_user_id === body.twitch_user_id);
  if (!user || user.revision <= body.expected_revision || user.paused !== body.paused || user.monthly_cap_micro_usd !== body.monthly_cap_micro_usd || user.monthly_ai_jobs !== body.monthly_ai_jobs || user.monthly_revisions !== body.monthly_revisions) throw new Error('Die Nutzergrenze wurde nicht bestätigt. Bitte den Stand neu laden.');
  return data;
}

export async function savePlanLimit(body: PlanLimitChange) {
  const data = requireLimits(await postAdminJson<unknown, PlanLimitChange>('/social-ai/limits/plan', body));
  const plan = data.plans.find(plan => plan.plan_id === body.plan_id);
  if (!plan || plan.revision <= body.expected_revision || plan.monthly_cap_micro_usd !== body.monthly_cap_micro_usd || plan.monthly_ai_jobs !== body.monthly_ai_jobs || plan.monthly_revisions !== body.monthly_revisions) throw new Error('Die vorgemerkte Planregel wurde nicht bestätigt. Bitte den Stand neu laden.');
  return data;
}

export async function fetchSkills(signal?: AbortSignal) {
  return requireSkills(await admin<unknown>('/social-ai/skills', { cache: 'no-store', signal }));
}

export async function reviewProposal(proposalId: string, status: 'reviewed' | 'dismissed') {
  const data = requireSkills(await postAdminJson<unknown, { status: string }>(`/social-ai/proposals/${encodeURIComponent(proposalId)}/status`, { status }));
  const proposal = data.proposals.find(proposal => proposal.id === proposalId);
  if (!proposal || proposal.status !== status || !proposal.reviewed_at || !proposal.reviewed_by) {
    throw new Error('Der Vorschlagsstatus wurde nicht bestätigt. Bitte den Stand neu laden.');
  }
  return data;
}

export async function rollbackSkill(skillId: string, body: RollbackChange) {
  const data = requireSkills(await postAdminJson<unknown, RollbackChange>(`/social-ai/skills/${encodeURIComponent(skillId)}/rollback`, body));
  if (data.skills.find(skill => skill.skill_id === skillId)?.current_version !== body.target_version) {
    throw new Error('Die Zielversion wurde nicht bestätigt. Bitte den Stand neu laden.');
  }
  return data;
}

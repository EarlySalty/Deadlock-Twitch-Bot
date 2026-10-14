export const totals = {
  attempts: 2, incomplete_attempts: 0, unknown_cost_attempts: 0,
  input_tokens: 1450, output_tokens: 220, cache_creation_input_tokens: 0, cache_read_input_tokens: 100,
  estimated_micro_usd: 15320, measured_micro_usd: null, invoiced_micro_usd: null,
  held_micro_usd: 0, charged_micro_usd: 15320, provider_latency_ms: 8400,
};

export const limits = {
  enforcement_available: true, plan_configuration_available: true, plan_policies_active: false,
  global: { revision: 1, paused: false, initial_total_cap_micro_usd: 2000000, monthly_cap_micro_usd: 1500000, updated_at: '2026-10-11T12:00:00Z', updated_by: 'Synthetischer Admin', reason: 'Isolierter Teststand.' },
  users: [{ twitch_user_id: '100000001', revision: 1, paused: false, monthly_cap_micro_usd: 1000000, monthly_ai_jobs: 12, monthly_revisions: 4, updated_at: '2026-10-11T12:00:00Z', updated_by: 'Synthetischer Admin', reason: 'Isolierte Testgrenze.' }],
  plans: [{ plan_id: 'free', name: 'Free', revision: 0, monthly_cap_micro_usd: null, monthly_ai_jobs: null, monthly_revisions: null, reason: '' }],
};

export function usage(groupBy = 'user', offset = 0) {
  return {
    totals: { ...totals }, stages: [{ ...totals, attempts: 1, estimated_micro_usd: 10000, charged_micro_usd: 10000, provider_latency_ms: 6000, stage: 'analysis', run: 'first_run' }, { ...totals, attempts: 1, estimated_micro_usd: 5320, charged_micro_usd: 5320, provider_latency_ms: 2400, stage: 'planning', run: 'revision' }], groups: [{ ...totals, twitch_user_id: groupBy === 'provider' ? null : '100000001', provider: 'synthetic', model: 'synthetic-model', job_id: groupBy === 'job' ? 'manual:1' : null, processing_ms: groupBy === 'job' ? 18400 : null, rendering_ms: groupBy === 'provider' ? null : 6800 }],
    group_by: groupBy, offset, has_more: offset === 0, period_start: '2026-10-01T00:00:00Z', period_end: '2026-10-11T12:00:00Z',
  };
}

export const skills = {
  rollback_available: true,
  skills: [{ skill_id: 'copywriting', name: 'Texte', current_version: '2', versions: [
    { version: '2', content_hash: 'b'.repeat(64), previous_version: '1', changed_at: '2026-10-12T08:00:00Z', summary: 'Eigene Formulierungen', reason: 'Wiederholte Korrekturen', evidence: { summary: 'Wiederholte Korrekturen', sample_count: 6 }, diff: [{ kind: 'removed', text: '# Copywriting v1' }, { kind: 'added', text: '# Copywriting v2' }] },
    { version: '1', content_hash: 'a'.repeat(64), previous_version: null, changed_at: '2026-10-11T12:00:00Z', summary: 'Mitgelieferte Ausgangsversion', reason: 'Ausgangsstand', evidence: null, diff: [] },
  ], activations: [{ kind: 'learned', from_version: '1', to_version: '2', actor: 'ai-learning', reason: 'Wiederholte bestätigte Korrekturen', created_at: '2026-10-12T08:00:00Z' }] }],
  proposals: [{ id: '1', title: 'Feld vorschlagen', created_at: '2026-10-12T08:00:00Z', summary: 'Zusatzfeld', reason: 'Häufige Nachfrage', status: 'proposed', evidence: null, reviewed_at: null, reviewed_by: null }],
};

export const totals = {
  attempts: 2, incomplete_attempts: 0, unknown_cost_attempts: 0,
  input_tokens: 1450, output_tokens: 220, cache_creation_input_tokens: 0, cache_read_input_tokens: 100,
  estimated_micro_usd: 15320, measured_micro_usd: null, invoiced_micro_usd: null,
  held_micro_usd: 20000, provider_latency_ms: 8400,
};

export const skills = {
  rollback_available: true,
  skills: [{
    skill_id: 'fixture-editing', name: 'Synthetische Schnittstrategie', current_version: 'v2',
    versions: [{
      version: 'v2', content_hash: null, previous_version: 'v1', changed_at: '2026-10-11T12:00:00Z',
      summary: 'Reaktionen in kurzen Szenen erhalten.', reason: 'Anonymisierte Testbeispiele zeigen abgeschnittene Reaktionen.',
      evidence: { summary: 'Isolierte synthetische Qualitätsbeispiele, keine echten Nutzerdaten.', sample_count: 8 },
      diff: [{ kind: 'context', text: 'Originalton beibehalten.' }, { kind: 'removed', text: 'Pausen ohne Kontext kürzen.' }, { kind: 'added', text: 'Vor Kürzungen prüfen, ob die Reaktion zum Highlight gehört. Wichtige Reaktionen vollständig erhalten und bei unsicherem Kontext zur Prüfung vorlegen.' }],
    }, {
      version: 'v1', content_hash: null, previous_version: null, changed_at: '2026-10-10T12:00:00Z',
      summary: 'Synthetische Erstversion.', reason: 'Testgrundlage.', evidence: null, diff: [],
    }],
  }],
  proposals: [{ id: 'fixture-proposal', title: 'Synthetischer Werkzeugvorschlag', created_at: '2026-10-11T12:00:00Z', summary: 'Ein mögliches Werkzeug zur manuellen Prüfung vormerken.', reason: 'Vergleich isolierter Testabläufe.', status: 'proposed', evidence: 'Keine reale Wirkung geprüft.' }],
};

export const limits = {
  enforcement_available: true, plan_configuration_available: true, plan_policies_active: false,
  global: { revision: 1, paused: false, initial_total_cap_micro_usd: 2000000, updated_at: '2026-10-11T12:00:00Z', updated_by: 'Synthetischer Admin', reason: 'Isolierter Teststand.' },
  users: [{ twitch_user_id: '100000001', revision: 1, paused: false, monthly_cap_micro_usd: 1000000, updated_at: '2026-10-11T12:00:00Z', updated_by: 'Synthetischer Admin', reason: 'Isolierte Testgrenze.' }],
  plans: [{ plan_id: 'fixture-existing-plan', name: 'Synthetischer bestehender Plan', revision: 1, monthly_cap_micro_usd: null, monthly_ai_jobs: null, monthly_revisions: null, reason: 'Testentwurf, inaktiv.' }],
};

export function usage(groupBy = 'user', offset = 0) {
  return {
    totals: { ...totals }, groups: [{ ...totals, twitch_user_id: groupBy === 'provider' ? null : '100000001', provider: 'Testanbieter', model: 'Synthetisches Modell', job_id: groupBy === 'job' ? 'fixture-job' : null, processing_ms: 18400, rendering_ms: 6800 }],
    group_by: groupBy, offset, has_more: offset === 0, period_start: '2026-10-01T00:00:00Z', period_end: '2026-10-11T12:00:00Z',
  };
}

import type { AiConsent, AiJobView } from './api';

export type AiPreviewScenario = 'consent-off' | 'global-off' | 'render' | 'review' | 'failed' | 'handoff' | 'race';

export const aiPreviewScenarios: AiPreviewScenario[] = ['consent-off', 'global-off', 'render', 'review', 'failed', 'handoff', 'race'];

const CHANNEL = '123456789';
const PREFIX = '/twitch/social-media/api/ai';
const ORIGINAL = `${PREFIX}/jobs/1101/original`;
const OUTPUT = `${PREFIX}/jobs/1101/versions/2201/file`;

const BASE_JOB: AiJobView = {
  id: 1101,
  clip_db_id: 501,
  status: 'review',
  stage: 'review',
  created_at: '2026-10-11T08:15:00Z',
  updated_at: '2026-10-11T08:18:03Z',
  completed_at: '2026-10-11T08:18:03Z',
  provider: 'Anbieter aus Konfiguration',
  model: 'Modell aus Konfiguration',
  original_duration_ms: 54800,
  output_duration_ms: 42400,
  processing_ms: 183000,
  rendering_ms: 42900,
  input_tokens: 12894,
  output_tokens: 921,
  estimated_provider_cost_usd: 0.04372,
  measured_provider_cost_usd: null,
  cost_basis: 'usage_priced_estimate',
  highlights: [
    { start_ms: 4200, end_ms: 16500, description: 'Der Einstieg zeigt, wie das Team den Bereich sichert.' },
    { start_ms: 26500, end_ms: 50500, description: 'Der Teamkampf und die Reaktion bleiben erhalten.' },
  ],
  decisions: [
    { start_ms: 0, end_ms: 4200, description: 'Die ruhige Einleitung wurde entfernt.' },
    { start_ms: 16500, end_ms: 26500, description: 'Eine Pause ohne neue Spielhandlung wurde gekürzt.' },
  ],
  uncertainty: ['Eine Spielfähigkeit ist aus den Einzelbildern nicht sicher erkennbar.'],
  review_reasons: ['Prüfe, ob die Untertitel den gesprochenen Begriff korrekt wiedergeben.'],
  original_url: ORIGINAL,
  versions: [
    { id: 2201, version_no: 1, created_at: '2026-10-11T08:18:03Z', status: 'ready', duration_ms: 42400, file_url: OUTPUT, parent_version_id: null },
    { id: 2202, version_no: 2, created_at: '2026-10-11T08:25:00Z', status: 'failed', duration_ms: null, file_url: null, parent_version_id: 2201 },
  ],
  failure_code: null,
  failure_message: null,
  can_retry: false,
};

export function aiPreviewJob(scenario: AiPreviewScenario): AiJobView | null {
  if (scenario === 'consent-off' || scenario === 'global-off') return null;
  if (scenario === 'render') return {
    ...BASE_JOB, status: 'rendering', stage: 'rendering', updated_at: '2026-10-11T08:17:23Z', completed_at: null,
    output_duration_ms: null, processing_ms: null, rendering_ms: null, versions: [],
    input_tokens: 12894, output_tokens: 921, estimated_provider_cost_usd: 0.04372,
    review_reasons: [],
  };
  if (scenario === 'failed' || scenario === 'race') return {
    ...BASE_JOB, status: 'failed', stage: 'rendering', completed_at: null, output_duration_ms: null,
    versions: scenario === 'race' ? BASE_JOB.versions : [], rendering_ms: null, failure_code: 'render_failed', failure_message: 'Der Renderversuch ist fehlgeschlagen.', can_retry: true,
  };
  return BASE_JOB;
}

export function aiPreviewConsent(scenario: AiPreviewScenario): AiConsent {
  return { external_ai_consent: scenario !== 'consent-off' && scenario !== 'global-off' && scenario !== 'race', global_allowed: scenario !== 'global-off' };
}

function response(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

export function createAiPreviewFetch(originalFetch: typeof fetch, scenario: AiPreviewScenario): typeof fetch {
  let consent = aiPreviewConsent(scenario);
  let job = aiPreviewJob(scenario);
  let polls = 0;
  let secondConsent = false;
  const secondJob: AiJobView = { ...BASE_JOB, id: 1201, clip_db_id: 601, status: 'review', original_url: null, versions: [] };
  let secondFailure: AiJobView = { ...BASE_JOB, id: 1202, clip_db_id: 601, status: 'failed', stage: 'rendering', original_url: null, versions: [], failure_code: 'render_failed', failure_message: 'Die Prüfung ist fehlgeschlagen. Versuche es erneut.', can_retry: true };
  return async (input, init) => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, window.location.origin);
    if (url.pathname !== '/twitch/social-media/api/admin/clips' && !url.pathname.startsWith(`${PREFIX}/`)) return originalFetch(input, init);
    const method = (init?.method ?? 'GET').toUpperCase();
    const body = init?.body && typeof init.body === 'string' ? JSON.parse(init.body) as Record<string, unknown> : {};
    const channel = method === 'GET' ? url.searchParams.get('twitch_user_id') : body.twitch_user_id;
    if (scenario === 'race' && channel === '987654321') {
      if (url.pathname === '/twitch/social-media/api/admin/clips') return response({ items: [{ clip_db_id: 601, source_kind: 'twitch', title: 'Clip aus dem zweiten Kanal', created_at: '2026-10-11T07:00:00Z' }], total: 1, page: Number(url.searchParams.get('page')), page_size: Number(url.searchParams.get('page_size')) });
      if (url.pathname === `${PREFIX}/consent` && method === 'GET') return response({ external_ai_consent: secondConsent, global_allowed: true });
      if (url.pathname === `${PREFIX}/consent` && method === 'PUT' && typeof body.external_ai_consent === 'boolean') {
        secondConsent = body.external_ai_consent;
        return response({ external_ai_consent: secondConsent, global_allowed: true });
      }
      if (url.pathname === `${PREFIX}/jobs` && method === 'GET') return response({ items: [secondJob, secondFailure] });
      if (url.pathname === `${PREFIX}/jobs/1201` && method === 'GET') return response(secondJob);
      if (url.pathname === `${PREFIX}/jobs/1202` && method === 'GET') return response(secondFailure);
      if (url.pathname === `${PREFIX}/jobs/1202/retry` && method === 'POST') {
        secondFailure = { ...secondFailure, status: 'rendering', failure_code: null, failure_message: null, can_retry: false };
        return response({ job: secondFailure });
      }
      return response({ error: 'not_found' }, 404);
    }
    if (channel !== CHANNEL) return response({ error: 'channel_not_found' }, 403);
    if (scenario === 'handoff' && url.pathname === '/twitch/social-media/api/admin/clips' && method === 'GET') {
      const page = Number(url.searchParams.get('page'));
      const pageSize = Number(url.searchParams.get('page_size'));
      const item = { clip_db_id: 135, source_kind: 'twitch', title: 'Übernommener Clip aus dem Kanal', created_at: '2026-10-10T20:00:00Z' };
      return response({ items: page === 2 && pageSize === 100 ? [item] : page === 1 ? Array.from({ length: pageSize }, (_, index) => ({ ...item, clip_db_id: index + 1 })) : [], total: 135, page, page_size: pageSize });
    }
    if (url.pathname === '/twitch/social-media/api/admin/clips' && method === 'GET') return response({
      items: [
        { clip_db_id: 501, source_kind: 'twitch', title: 'Teamkampf am letzten Wächter', created_at: '2026-10-10T20:00:00Z' },
        { clip_db_id: 502, source_kind: 'twitch', title: 'Entscheidender Angriff im Mittelgang', created_at: '2026-10-09T19:00:00Z' },
      ], total: 2, page: 1, page_size: 30,
    });
    if (url.pathname === `${PREFIX}/consent` && method === 'GET') return response(consent);
    if (url.pathname === `${PREFIX}/consent` && method === 'PUT' && typeof body.external_ai_consent === 'boolean') {
      consent = { ...consent, external_ai_consent: body.external_ai_consent };
      return response(consent);
    }
    if (url.pathname === `${PREFIX}/jobs` && method === 'GET') return response({ items: job ? [job] : [] });
    if (url.pathname === `${PREFIX}/jobs` && method === 'POST') {
      if (!consent.external_ai_consent || !consent.global_allowed || ![501, 502, ...(scenario === 'handoff' ? [135] : [])].includes(Number(body.clip_db_id))) return response({ error: 'not_allowed' }, 403);
      job = { ...BASE_JOB, clip_db_id: Number(body.clip_db_id), status: 'rendering', stage: 'rendering', completed_at: null, processing_ms: null, rendering_ms: null, versions: [], can_retry: false };
      polls = 0;
      return response({ job });
    }
    if (url.pathname === `${PREFIX}/jobs/1101/retry` && method === 'POST') {
      if (!job || job.status !== 'failed' || !job.can_retry || !consent.external_ai_consent || !consent.global_allowed) return response({ error: 'retry_not_allowed' }, 409);
      job = { ...job, status: 'rendering', stage: 'rendering', failure_code: null, failure_message: null, can_retry: false };
      polls = 0;
      return response({ job });
    }
    if (url.pathname === `${PREFIX}/jobs/1101` && method === 'GET' && job) {
      if (job.status === 'rendering' && ++polls >= 4) job = { ...BASE_JOB, clip_db_id: job.clip_db_id };
      return response(job);
    }
    return response({ error: 'not_found' }, 404);
  };
}

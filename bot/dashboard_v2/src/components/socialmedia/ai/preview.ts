import type { AiConsent, AiJobView, AiVersionMetadata } from './api';

export type AiPreviewScenario = 'consent-off' | 'global-off' | 'render' | 'review' | 'failed' | 'handoff' | 'race' | 'revisions';

export const aiPreviewScenarios: AiPreviewScenario[] = ['consent-off', 'global-off', 'render', 'review', 'failed', 'handoff', 'race', 'revisions'];

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
  output_duration_ms: 40600,
  processing_ms: 183000,
  provider_latency_ms: 78200,
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
    { start_ms: 4200, end_ms: 16500, description: 'Der Einstieg und die Vorbereitung bleiben erhalten.' },
    { start_ms: 26500, end_ms: 54800, description: 'Teamkampf und ursprüngliche Reaktion bleiben erhalten.' },
  ],
  uncertainty: ['Eine Spielfähigkeit ist aus den Einzelbildern nicht sicher erkennbar.'],
  review_reasons: ['Prüfe, ob die Untertitel den gesprochenen Begriff korrekt wiedergeben.'],
  original_url: ORIGINAL,
  versions: [
    { id: 2201, version_no: 1, created_at: '2026-10-11T08:18:03Z', status: 'review', duration_ms: 40600, file_url: OUTPUT, parent_version_id: null, job_id: 1101, variant_index: 0, original_url: ORIGINAL, decisions: [{start_ms:4200,end_ms:16500,description:'Einstieg behalten.'},{start_ms:26500,end_ms:54800,description:'Teamkampf und Reaktion behalten.'}], approved_platforms: [] },
    { id: 2202, version_no: 2, created_at: '2026-10-11T08:25:00Z', status: 'failed', duration_ms: null, file_url: null, parent_version_id: 2201, job_id: 1102, variant_index: 0, original_url: null, decisions: [], approved_platforms: [] },
  ],
  failure_code: null,
  failure_message: null,
  can_retry: false,
  root_job_id: 1101,
  parent_version_id: null,
  selected_version_id: null,
  revision_proposal: null,
};

export function aiPreviewJob(scenario: AiPreviewScenario): AiJobView | null {
  if (scenario === 'consent-off' || scenario === 'global-off') return null;
  if (scenario === 'render') return {
    ...BASE_JOB, status: 'rendering', stage: 'rendering', updated_at: '2026-10-11T08:17:23Z', completed_at: null,
    output_duration_ms: null, processing_ms: null, provider_latency_ms: null, rendering_ms: null, versions: [],
    input_tokens: 12894, output_tokens: 921, estimated_provider_cost_usd: 0.04372,
    review_reasons: [],
  };
  if (scenario === 'failed' || scenario === 'race') return {
    ...BASE_JOB, status: 'failed', stage: 'rendering', completed_at: null, output_duration_ms: null,
    versions: scenario === 'race' ? BASE_JOB.versions : [], rendering_ms: null, failure_code: 'render_failed', failure_message: 'Der Renderversuch ist fehlgeschlagen.', can_retry: true,
  };
  if (scenario === 'revisions') return {
    ...BASE_JOB,
    versions: [...BASE_JOB.versions, {...BASE_JOB.versions[0],id:2203,version_no:3,parent_version_id:2201,job_id:1103,duration_ms:38000,decisions:[{start_ms:4200,end_ms:12000,description:'Kurzen Einstieg behalten.'},{start_ms:24600,end_ms:54800,description:'Teamkampf und Reaktion behalten.'}],created_at:'2026-10-11T09:00:00Z'}],
  };
  return BASE_JOB;
}

export function aiPreviewConsent(scenario: AiPreviewScenario): AiConsent {
  return { external_ai_consent: scenario !== 'consent-off' && scenario !== 'global-off' && scenario !== 'race', global_allowed: scenario !== 'global-off' };
}

function response(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

export function createAiPreviewFetch(originalFetch: typeof fetch, scenario: AiPreviewScenario, autopilot = false): typeof fetch {
  let consent = aiPreviewConsent(scenario);
  let job = aiPreviewJob(scenario);
  if (autopilot && job) job = {...job,selected_version_id:2203,versions:job.versions.map((version) => version.id === 2201 ? {...version,status:'rejected'} : version)};
  let polls = 0;
  let secondConsent = false;
  let options = { multi_output_enabled: false, experimental_ai_layout: false, publish_language: 'de' };
  const metadata = new Map<number, AiVersionMetadata['platforms']>();
  const secondJob: AiJobView = { ...BASE_JOB, id: 1201, clip_db_id: 601, status: 'review', original_url: null, versions: [] };
  let secondFailure: AiJobView = { ...BASE_JOB, id: 1202, clip_db_id: 601, status: 'failed', stage: 'rendering', original_url: null, versions: [], failure_code: 'render_failed', failure_message: 'Die Prüfung ist fehlgeschlagen. Der Anbieter meldet SIMULIERTER_WERT aus /var/test/mock.', can_retry: true };
  let secondStarted: AiJobView | null = null;
  return async (input, init) => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, window.location.origin);
    if (url.pathname !== '/twitch/social-media/api/admin/clips' && !url.pathname.startsWith(`${PREFIX}/`)) return originalFetch(input, init);
    const method = (init?.method ?? 'GET').toUpperCase();
    const body = init?.body && typeof init.body === 'string' ? JSON.parse(init.body) as Record<string, unknown> : {};
    const channel = method === 'GET' ? url.searchParams.get('twitch_user_id') : body.twitch_user_id;
    if (url.pathname === `${PREFIX}/options` && method === 'GET') return response(options);
    if (url.pathname === `${PREFIX}/options` && method === 'PUT' && typeof body.multi_output_enabled === 'boolean' && typeof body.experimental_ai_layout === 'boolean') {
      options = { multi_output_enabled: body.multi_output_enabled, experimental_ai_layout: body.experimental_ai_layout, publish_language: typeof body.publish_language === 'string' ? body.publish_language : options.publish_language };
      return response(options);
    }
    if (scenario === 'race' && channel === '987654321') {
      if (url.pathname === '/twitch/social-media/api/admin/clips') return response({ items: [
        { clip_db_id: 601, source_kind: 'twitch', title: 'Clip aus dem zweiten Kanal', created_at: '2026-10-11T07:00:00Z' },
        { clip_db_id: 602, source_kind: 'twitch', title: 'Weiterer Clip aus dem zweiten Kanal', created_at: '2026-10-11T07:01:00Z' },
      ], total: 2, page: Number(url.searchParams.get('page')), page_size: Number(url.searchParams.get('page_size')) });
      if (url.pathname === `${PREFIX}/consent` && method === 'GET') return response({ external_ai_consent: secondConsent, global_allowed: true });
      if (url.pathname === `${PREFIX}/consent` && method === 'PUT' && typeof body.external_ai_consent === 'boolean') {
        secondConsent = body.external_ai_consent;
        return response({ external_ai_consent: secondConsent, global_allowed: true });
      }
      if (url.pathname === `${PREFIX}/jobs` && method === 'GET') return response({ items: [secondJob, secondFailure, ...(secondStarted ? [secondStarted] : [])] });
      if (url.pathname === `${PREFIX}/jobs` && method === 'POST' && secondConsent && [601, 602].includes(Number(body.clip_db_id))) {
        secondStarted = { ...BASE_JOB, id: 1203, clip_db_id: Number(body.clip_db_id), status: 'rendering', stage: 'rendering', provider_latency_ms: null, original_url: null, versions: [], failure_code: null, failure_message: null, can_retry: false };
        return response({ job: secondStarted });
      }
      if (url.pathname === `${PREFIX}/jobs/1203` && method === 'GET' && secondStarted) return response(secondStarted);
      if (url.pathname === `${PREFIX}/jobs/1201` && method === 'GET') return response(secondJob);
      if (url.pathname === `${PREFIX}/jobs/1202` && method === 'GET') return response(secondFailure);
      if (url.pathname === `${PREFIX}/jobs/1202/retry` && method === 'POST') {
        secondFailure = { ...secondFailure, status: 'rendering', failure_code: null, failure_message: null, can_retry: false };
        return response({ job: secondFailure });
      }
      return response({ error: 'not_found' }, 404);
    }
    if (channel !== CHANNEL) return response({ error: 'channel_not_found' }, 403);
    if (/\/videos\/\d+\/editor$/.test(url.pathname) && method === 'POST' && job) return response({ job });
    const copyMatch = url.pathname.match(/\/videos\/(\d+)\/metadata$/);
    if (copyMatch && job) {
      const id = Number(copyMatch[1]);
      const version = job.versions.find((item) => item.id === id);
      if (!version) return response({ error: 'not_found' }, 404);
      const editable = version.status === 'review';
      if (method === 'PUT') {
        if (!editable) return response({ error: 'Diese Fassung ist nicht mehr zur Sichtung offen.' }, 409);
        metadata.set(id, body.platforms as AiVersionMetadata['platforms']);
      }
      const copy = { title: 'Teamkampf am letzten Wächter', description: 'Der entscheidende Spielmoment aus dem Deadlock-Clip.', hashtags: ['Deadlock', 'Clips'] };
      return response({video_id: id, editable, platforms: metadata.get(id) ?? {youtube: copy, tiktok: copy, instagram: copy}});
    }
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
      job = { ...BASE_JOB, clip_db_id: Number(body.clip_db_id), status: 'rendering', stage: 'rendering', completed_at: null, processing_ms: null, provider_latency_ms: null, rendering_ms: null, versions: [], can_retry: false };
      polls = 0;
      return response({ job });
    }
    if (url.pathname === `${PREFIX}/jobs/1101/retry` && method === 'POST') {
      if (!job || job.status !== 'failed' || !job.can_retry || !consent.external_ai_consent || !consent.global_allowed) return response({ error: 'retry_not_allowed' }, 409);
      job = { ...job, status: 'rendering', stage: 'rendering', failure_code: null, failure_message: null, can_retry: false };
      polls = 0;
      return response({ job });
    }
    const versionAction = url.pathname.match(/\/jobs\/(\d+)\/versions\/(\d+)\/(revision|select|decision|approve|schedule)$/);
    if (versionAction && method === 'POST' && job && Number(versionAction[1]) === job.id) {
      const version = job.versions.find((item) => item.id === Number(versionAction[2]) && item.file_url);
      if (!version) return response({error:'not_found'},404);
      const action = versionAction[3];
      if (action === 'revision') {
        if (!consent.external_ai_consent || !consent.global_allowed || typeof body.feedback !== 'string' || !body.feedback.trim()) return response({error:'not_allowed'},403);
        job = {...job,id:1104,parent_version_id:version.id,versions:autopilot ? job.versions.map((item) => item.id === version.id ? {...item,status:'rejected',approved_platforms:[]} : item) : job.versions,status:'awaiting_confirmation',stage:'planning',completed_at:null,revision_proposal:{summary:'Der Einstieg wird stärker gekürzt.',impact:'Die neue Fassung behält den Teamkampf und die ursprüngliche Reaktion.',reanalyze:false,requires_confirmation:true,estimated_provider_cost_usd:0}};
      } else if (action === 'select') job = {...job,selected_version_id:version.id};
      else if (action === 'approve' || action === 'schedule') {
        const selected = Array.isArray(body.platforms) ? body.platforms.filter((item): item is string => typeof item === 'string' && ['youtube','tiktok','instagram'].includes(item)) : [];
        if (!selected.length || (action === 'schedule' && selected.some((item) => !version.approved_platforms.includes(item)))) return response({error:'not_approved'},409);
        job = {...job,versions:job.versions.map((item) => item.id === version.id ? {...item,status:action === 'approve' ? 'approved' : 'scheduled',approved_platforms:[...new Set([...item.approved_platforms,...selected])]} : item)};
        if (action === 'schedule') return response({job,reservation:{produced_video_id:version.id,scheduled_at:'2026-10-12T18:00:00Z',queue_ids:selected.map((platform,index) => [platform,3301 + index])}});
      }
      return response({job});
    }
    if (url.pathname === `${PREFIX}/jobs/1104/proposal` && method === 'POST' && job?.id === 1104 && job.status === 'awaiting_confirmation') {
      job = {...job,status:body.accept === true ? 'rendering' : 'rejected',stage:body.accept === true ? 'rendering' : 'planning',versions:autopilot && body.accept !== true ? job.versions.map((item) => item.id === job?.parent_version_id ? {...item,status:'review'} : item) : job.versions};
      polls = 0;
      return response({job});
    }
    if (url.pathname === `${PREFIX}/jobs/${job?.id}` && method === 'GET' && job) {
      if (job.status === 'rendering' && ++polls >= 4) job = job.id === 1104 ? {...job,status:'review',stage:'review',selected_version_id:2204,versions:[...job.versions,{...BASE_JOB.versions[0],id:2204,version_no:4,job_id:1104,parent_version_id:job.parent_version_id,duration_ms:33000,decisions:[{start_ms:8500,end_ms:12000,description:'Kürzeren Einstieg behalten.'},{start_ms:25300,end_ms:54800,description:'Teamkampf und Reaktion behalten.'}]}],revision_proposal:null} : { ...BASE_JOB, clip_db_id: job.clip_db_id };
      return response({...job,publishing_in_autopilot:autopilot});
    }
    return response({ error: 'not_found' }, 404);
  };
}

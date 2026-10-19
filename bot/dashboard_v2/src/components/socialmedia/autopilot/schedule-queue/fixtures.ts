import type { PipelinePolicy, ScheduleQueueApi, ScheduleQueueSnapshot } from './api';

export interface ScheduleQueueFixture {
  api: ScheduleQueueApi;
  requests: { method: string; path: string; body: unknown }[];
  snapshot(): ScheduleQueueSnapshot;
  policy(): PipelinePolicy;
}

export function createScheduleQueueFixture(): ScheduleQueueFixture {
  const requests: ScheduleQueueFixture['requests'] = [];
  const state: ScheduleQueueSnapshot = {
    persisted: true,
    settings: { enabled: true, paused: false, ai_editing_enabled: false, lead_time_hours: 24, timezone: 'Europe/Berlin', target_platforms: ['youtube'], schedule_slots: [{ weekday: 1, local_time: '18:00' }, { weekday: 3, local_time: '19:00' }, { weekday: 5, local_time: '17:30' }] },
    schedule_scope: 'shared', distribution_policy: 'same_clip',
    next_slot: '2026-10-12T16:00:00Z', next_slots: { youtube: '2026-10-12T16:00:00Z' }, review_count: 1, production_hold: null,
    connected_platforms: ['youtube', 'tiktok'],
    jobs: [
      { id: 101, clip_id: 41, clip_title: 'Teamfight am Mid-Boss', source: 'autopilot', mode: 'static', requested_mode: null, status: 'running', stage: 'rendering', attempts: 1, max_attempts: 3, scheduled_at: '2026-10-12T16:00:00Z', started_at: '2026-10-11T17:15:00Z', last_error: null, review_explanation: null, review_reasons: [] },
      { id: 102, clip_id: 42, clip_title: 'Letzter Push mit dem Team', source: 'autopilot', mode: null, requested_mode: null, status: 'queued', stage: 'queued', attempts: 0, max_attempts: 3, scheduled_at: '2026-10-14T17:00:00Z', started_at: null, last_error: null, review_explanation: null, review_reasons: [] },
      { id: 103, clip_id: 43, clip_title: 'Knapp gerettet: die letzte Sekunde', source: 'autopilot', mode: null, requested_mode: null, status: 'queued', stage: 'queued', attempts: 0, max_attempts: 3, scheduled_at: '2026-10-16T15:30:00Z', started_at: null, last_error: null, review_explanation: null, review_reasons: [] },
      { id: 104, clip_id: 44, clip_title: 'KI-Auftrag wartet auf Prüfung', source: 'autopilot', mode: 'ai', requested_mode: null, status: 'review', stage: 'review', attempts: 1, max_attempts: 3, scheduled_at: null, started_at: '2026-10-11T15:00:00Z', last_error: null, review_explanation: 'Die entscheidende Szene ist möglicherweise nicht vollständig im Clip enthalten.', review_reasons: ['unsuitable_clip'] },
    ],
  };
  const policy: PipelinePolicy = {
    pipeline_mode: 'upload', review_policy: 'required', metadata_scope: 'shared',
    text_defaults: { shared: { title: '', description: '', hashtags: [] }, per_platform: {} },
    schedule_scope: 'shared', platform_schedule_slots: {}, distribution_policy: 'same_clip', policy_revision: 1,
  };
  const copy = () => structuredClone(state);
  function order(ids: number[]) {
    const queued = state.jobs.filter(job => job.status === 'queued' && !job.started_at);
    if (ids.length !== queued.length || new Set(ids).size !== ids.length || ids.some(id => !queued.some(job => job.id === id))) throw new Error('Die Warteschlange wurde inzwischen geändert. Bitte aktualisiere sie.');
    const slots = queued.map(job => job.scheduled_at);
    const changed = ids.map((id, index) => ({ ...queued.find(job => job.id === id)!, scheduled_at: slots[index] }));
    state.jobs = [...state.jobs.filter(job => job.status !== 'queued' || job.started_at), ...changed];
  }
  const api: ScheduleQueueApi = {
    async load(owner) { requests.push({ method: 'GET', path: '/autopilot/schedule', body: { twitch_user_id: owner } }); return copy(); },
    async save(patch, owner) {
      requests.push({ method: 'PUT', path: '/autopilot/schedule', body: { twitch_user_id: owner, ...patch } });
      try { new Intl.DateTimeFormat('de-DE', { timeZone: patch.timezone ?? state.settings.timezone }); } catch { throw new Error('Unbekannte IANA-Zeitzone.'); }
      Object.assign(state.settings, patch);
      state.persisted = true;
      state.production_hold = !state.settings.enabled ? 'disabled' : state.settings.paused ? 'paused' : null;
      return copy();
    },
    async reorder(ids, owner) { requests.push({ method: 'PUT', path: '/autopilot/queue/order', body: { twitch_user_id: owner, job_ids: ids } }); order(ids); return copy(); },
    async next(id, owner) { requests.push({ method: 'POST', path: `/autopilot/queue/${id}/next`, body: { twitch_user_id: owner } }); const ids = state.jobs.filter(job => job.status === 'queued' && !job.started_at).map(job => job.id); order([id, ...ids.filter(value => value !== id)]); return copy(); },
    async resolve(id, action, note, owner) {
      requests.push({ method: 'POST', path: `/autopilot/queue/${id}/resolve`, body: { twitch_user_id: owner, action, note } });
      const job = state.jobs.find(value => value.id === id && value.status === 'review');
      if (!job) throw new Error('Der Auftrag wartet nicht mehr auf eine Entscheidung.');
      job.status = action === 'process' ? 'queued' : 'cancelled';
      if (action === 'process') {
        job.started_at = null;
        job.attempts = 0;
        job.review_reasons = [];
        state.jobs = [job, ...state.jobs.filter(value => value.id !== id)];
      }
      state.review_count = Math.max(0, state.review_count - 1);
      return copy();
    },
    async loadPolicy(owner) { requests.push({ method: 'GET', path: '/autopilot/policy', body: { twitch_user_id: owner } }); return structuredClone(policy); },
    async savePolicy(next, owner) {
      requests.push({ method: 'PUT', path: '/autopilot/policy', body: { twitch_user_id: owner, ...next } });
      if (next.policy_revision !== policy.policy_revision) throw new Error('Die Einstellungen wurden zwischenzeitlich geändert. Bitte neu laden.');
      Object.assign(policy, structuredClone(next), { policy_revision: policy.policy_revision + 1 });
      state.schedule_scope = policy.schedule_scope;
      state.distribution_policy = policy.distribution_policy;
      return structuredClone(policy);
    },
    async startTestRun(owner) {
      requests.push({ method: 'POST', path: '/autopilot/test-run', body: { twitch_user_id: owner } });
      if (state.test_run) throw new Error('Es läuft bereits ein Testlauf. Prüfe oder verwirf ihn zuerst.');
      const mode = state.settings.ai_editing_enabled ? 'ai' as const : 'static' as const;
      const job = { id: 900, clip_id: 45, clip_title: 'Testclip aus dem Vorrat', source: 'test' as const, mode, requested_mode: null, status: 'running' as const, stage: 'starting', attempts: 1, max_attempts: 3, scheduled_at: null, started_at: new Date().toISOString(), last_error: null, review_explanation: null, review_reasons: [] };
      state.jobs = [job, ...state.jobs];
      state.test_run = { job_id: job.id, clip_id: job.clip_id, clip_title: job.clip_title, status: job.status, stage: job.stage, mode, last_error: null, video_id: null, video_status: null };
      return copy();
    },
    async discardTestRun(id, owner) {
      requests.push({ method: 'POST', path: `/autopilot/test-run/${id}/discard`, body: { twitch_user_id: owner } });
      if (state.test_run?.job_id !== id) throw new Error('Dieser Testlauf ist nicht mehr aktiv.');
      state.jobs = state.jobs.map(job => job.id === id ? { ...job, status: 'cancelled', stage: 'test_discarded' } : job);
      state.test_run = null;
      return copy();
    },
    async retryTestRun(id, owner) {
      requests.push({ method: 'POST', path: `/autopilot/test-run/${id}/retry`, body: { twitch_user_id: owner } });
      if (state.test_run?.job_id !== id) throw new Error('Dieser Testlauf ist nicht mehr aktiv.');
      if (state.test_run.video_status === 'review') throw new Error('Dieser Testlauf kann nicht erneut gestartet werden.');
      const job = state.jobs.find(value => value.id === id);
      if (job) Object.assign(job, { status: 'queued' as const, stage: 'queued', last_error: null, attempts: 0 });
      state.test_run = { ...state.test_run, status: 'queued', stage: 'queued', last_error: null, video_id: null, video_status: null };
      return copy();
    },
    async cancel(id, owner) { requests.push({ method: 'POST', path: `/autopilot/queue/${id}/cancel`, body: { twitch_user_id: owner } }); const job = state.jobs.find(value => value.id === id && value.status === 'queued' && !value.started_at); if (!job) throw new Error('Der Auftrag kann nicht mehr entfernt werden.'); job.status = 'cancelled'; return copy(); },
  };
  return { api, requests, snapshot: copy, policy: () => structuredClone(policy) };
}

import type { ScheduleQueueApi, ScheduleQueueSnapshot } from './api';

export interface ScheduleQueueFixture {
  api: ScheduleQueueApi;
  requests: { method: string; path: string; body: unknown }[];
  snapshot(): ScheduleQueueSnapshot;
}

export function createScheduleQueueFixture(): ScheduleQueueFixture {
  const requests: ScheduleQueueFixture['requests'] = [];
  const state: ScheduleQueueSnapshot = {
    persisted: true,
    settings: { enabled: true, paused: false, ai_editing_enabled: false, lead_time_hours: 24, timezone: 'Europe/Berlin', schedule_slots: [{ weekday: 1, local_time: '18:00' }, { weekday: 3, local_time: '19:00' }, { weekday: 5, local_time: '17:30' }] },
    next_slot: '2026-10-12T16:00:00Z', review_count: 1, production_hold: null,
    jobs: [
      { id: 101, clip_id: 41, clip_title: 'Teamfight am Mid-Boss', source: 'autopilot', mode: 'static', requested_mode: null, status: 'running', stage: 'rendering', attempts: 1, max_attempts: 3, scheduled_at: '2026-10-12T16:00:00Z', started_at: '2026-10-11T17:15:00Z', last_error: null, review_reasons: [] },
      { id: 102, clip_id: 42, clip_title: 'Letzter Push mit dem Team', source: 'autopilot', mode: null, requested_mode: null, status: 'queued', stage: 'queued', attempts: 0, max_attempts: 3, scheduled_at: '2026-10-14T17:00:00Z', started_at: null, last_error: null, review_reasons: [] },
      { id: 103, clip_id: 43, clip_title: 'Knapp gerettet: die letzte Sekunde', source: 'autopilot', mode: null, requested_mode: null, status: 'queued', stage: 'queued', attempts: 0, max_attempts: 3, scheduled_at: '2026-10-16T15:30:00Z', started_at: null, last_error: null, review_reasons: [] },
      { id: 104, clip_id: 44, clip_title: 'KI-Auftrag wartet auf Prüfung', source: 'autopilot', mode: 'ai', requested_mode: null, status: 'review', stage: 'review', attempts: 1, max_attempts: 3, scheduled_at: null, started_at: '2026-10-11T15:00:00Z', last_error: null, review_reasons: ['ai_editor_unavailable'] },
    ],
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
    async cancel(id, owner) { requests.push({ method: 'POST', path: `/autopilot/queue/${id}/cancel`, body: { twitch_user_id: owner } }); const job = state.jobs.find(value => value.id === id && value.status === 'queued' && !value.started_at); if (!job) throw new Error('Der Auftrag kann nicht mehr entfernt werden.'); job.status = 'cancelled'; return copy(); },
  };
  return { api, requests, snapshot: copy };
}

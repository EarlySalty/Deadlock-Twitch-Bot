import type { PublishReviewClient, PublishSnapshot } from './api';

export function createPublishReviewFixture(initial?: PublishSnapshot) {
  let snapshot: PublishSnapshot = structuredClone(initial ?? {
    settings: {
      publish_mode: 'review', paused: false,
      notifications: { dashboard: true, discord: false, email: false, ready_for_review: true, publication_deadline: true, processing_failed: true, publication_failed: true, empty_pool: true, review_expiring: true, reminder_hours: 24 },
    },
    target_platforms: ['youtube', 'instagram', 'tiktok'],
    connections: [{ platform: 'youtube', connected: true, account_id: 'synthetic-youtube' }, { platform: 'instagram', connected: true, account_id: 'synthetic-instagram' }, { platform: 'tiktok', connected: false, account_id: null }],
    videos: [
      { id: 101, clip_id: 1, version_no: 2, variant_index: 0, mode: 'ai', status: 'review', duration_ms: 55000, expires_at: '2026-10-18T16:00:00Z', needs_review: true, review_reasons: ['Bitte prüfe, ob die wichtige Spielsituation vollständig erhalten ist.'], artifact_state: 'present', cleanup_error: null, approvals: [], event: null },
      { id: 102, clip_id: 2, version_no: 1, variant_index: 0, mode: 'static', status: 'scheduled', duration_ms: 22000, expires_at: '2026-10-17T16:00:00Z', needs_review: false, review_reasons: [], artifact_state: 'present', cleanup_error: null, approvals: [{ platform: 'youtube', state: 'approved', approver_user_id: 'synthetic-owner', decided_at: '2026-10-11T16:00:00Z' }, { platform: 'instagram', state: 'approved', approver_user_id: 'synthetic-owner', decided_at: '2026-10-11T16:00:00Z' }], event: { id: 501, produced_video_id: 102, scheduled_at: '2026-10-12T16:00:00Z', executed_at: '2026-10-12T16:01:00Z', completed_at: null, destinations: [{ platform: 'youtube', state: 'published', attempts: 0, next_retry_at: null, external_id: 'synthetic-receipt', account_id: 'synthetic-youtube', reason: null, visibility: 'public' }, { platform: 'instagram', state: 'retry', attempts: 1, next_retry_at: '2026-10-12T16:30:00Z', external_id: null, account_id: 'synthetic-instagram', reason: 'connection_or_quota', visibility: null }, { platform: 'tiktok', state: 'discarded', attempts: 0, next_retry_at: null, external_id: null, account_id: null, reason: 'not_approved_at_execution', visibility: null }] } },
      { id: 103, clip_id: 3, version_no: 1, variant_index: 0, mode: 'static', status: 'expired', duration_ms: 30000, expires_at: '2026-10-09T16:00:00Z', needs_review: true, review_reasons: [], artifact_state: 'deleted', cleanup_error: null, approvals: [], event: null },
      { id: 104, clip_id: 4, version_no: 1, variant_index: 1, mode: 'ai', status: 'scheduled', duration_ms: 35000, expires_at: '2026-10-18T16:00:00Z', needs_review: false, review_reasons: [], artifact_state: 'present', cleanup_error: null, approvals: [{ platform: 'youtube', state: 'approved', approver_user_id: 'synthetic-owner', decided_at: '2026-10-11T16:00:00Z' }], event: { id: 502, produced_video_id: 104, scheduled_at: '2026-10-12T17:00:00Z', executed_at: '2026-10-12T17:01:00Z', completed_at: null, destinations: [{ platform: 'youtube', state: 'uncertain', attempts: 0, next_retry_at: null, external_id: null, account_id: 'synthetic-youtube', reason: 'publication_outcome_unknown', visibility: null }] } },
    ],
    notifications: [{ id: 901, kind: 'empty_pool', content: 'Im ausgewählten Zeitraum sind keine unverarbeiteten Clips mehr verfügbar. Freigegebene Videos bleiben eingeplant.', created_at: '2026-10-11T16:00:00Z', discord_state: 'disabled', read_at: null }],
    capabilities: { discord: true, email: false, tiktok_effect_boundary: false },
  } satisfies PublishSnapshot);
  const calls: { method: string; owner: string; payload: unknown }[] = [];
  const client: PublishReviewClient = {
    load: async (owner, signal) => { if (signal?.aborted) throw new Error('aborted'); calls.push({ method: 'load', owner, payload: null }); return structuredClone(snapshot); },
    transition: async (owner, input) => { calls.push({ method: 'transition', owner, payload: input }); snapshot.settings.publish_mode = input.mode; },
    pause: async (owner, paused) => { calls.push({ method: 'pause', owner, payload: { paused } }); snapshot.settings.paused = paused; },
    preferences: async (owner, input) => { calls.push({ method: 'preferences', owner, payload: input }); snapshot.settings.notifications = structuredClone(input); },
    approve: async (owner, id, decisions) => {
      calls.push({ method: 'approve', owner, payload: { id, decisions } });
      const video = snapshot.videos.find((video) => video.id === id);
      if (!video || decisions.length === 0) throw new Error('Bitte wähle mindestens eine Plattform.');
      video.approvals = decisions.map(({ platform }) => ({ platform, state: 'approved', approver_user_id: owner, decided_at: '2026-10-11T16:00:00Z' }));
      video.status = 'approved'; video.needs_review = false;
    },
    schedule: async (owner, id, platforms) => {
      calls.push({ method: 'schedule', owner, payload: { id, platforms } });
      const video = snapshot.videos.find((video) => video.id === id);
      if (!video) throw new Error('Das Video wurde nicht gefunden.');
      video.status = 'scheduled';
      video.event = { id: 600 + id, produced_video_id: id, scheduled_at: '2026-10-13T16:00:00Z', executed_at: null, completed_at: null, destinations: platforms.map((platform) => ({ platform, state: 'pending', attempts: 0, next_retry_at: null, external_id: null, reason: null, account_id: null, visibility: null })) };
    },
    retry: async (owner, id, platform) => {
      calls.push({ method: 'retry', owner, payload: { id, platform } });
      const destination = snapshot.videos.find((video) => video.id === id)?.event?.destinations.find((destination) => destination.platform === platform);
      if (!destination || !['retry', 'held'].includes(destination.state)) throw new Error('Dieses Ziel kann nicht erneut übertragen werden.');
      destination.state = 'retry';
    },
  };
  let delayed: { method: keyof PublishReviewClient; promise: Promise<void> } | null = null;
  let failure: { method: keyof PublishReviewClient; message: string } | null = null;
  const controlled = new Proxy(client, {
    get(target, key) {
      const operation = Reflect.get(target, key);
      if (typeof operation !== 'function') return operation;
      return async (...args: unknown[]) => {
        if (delayed?.method === key) {
          const promise = delayed.promise;
          delayed = null;
          await promise;
        }
        if (failure?.method === key) {
          const message = failure.message;
          failure = null;
          throw new Error(message);
        }
        return Reflect.apply(operation, target, args);
      };
    },
  });
  return {
    client: controlled, calls,
    snapshot: () => structuredClone(snapshot),
    replace: (next: PublishSnapshot) => { snapshot = structuredClone(next); },
    holdNext: (method: keyof PublishReviewClient) => {
      let release: () => void = () => {};
      const promise = new Promise<void>((resolve) => { release = resolve; });
      delayed = { method, promise };
      return release;
    },
    failNext: (method: keyof PublishReviewClient, message = 'Die Änderung konnte nicht gespeichert werden.') => { failure = { method, message }; },
  };
}

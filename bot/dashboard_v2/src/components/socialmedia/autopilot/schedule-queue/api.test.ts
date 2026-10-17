import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createScheduleQueueFixture } from './fixtures';

Object.defineProperty(globalThis, 'window', { value: { location: { origin: 'https://dashboard.example.test', pathname: '/twitch/social-media', hostname: 'dashboard.example.test' } }, configurable: true });

const { scheduleQueueApi } = await import('./api');
const plan = await import('../../pipelinePlan');

test('schedule adapter uses the shared cookie helper and narrow owner-bound requests', async () => {
  const original = globalThis.fetch;
  const calls: { path: string; init: RequestInit }[] = [];
  globalThis.fetch = async (input, init = {}) => {
    calls.push({ path: String(input), init });
    return new Response(JSON.stringify(createScheduleQueueFixture().snapshot()), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    await scheduleQueueApi.load('42');
    await scheduleQueueApi.save({ paused: true, target_platforms: ['youtube'] }, '42');
    await scheduleQueueApi.reorder([102, 103], '42');
    await scheduleQueueApi.next(103, '42');
    await scheduleQueueApi.cancel(103, '42');
    await scheduleQueueApi.resolve(104, 'process', 'Weiterverarbeiten', '42');
    await scheduleQueueApi.loadPolicy('42');
    await scheduleQueueApi.savePolicy(createScheduleQueueFixture().policy(), '42');
    assert.deepEqual(calls.map(call => [call.path, call.init.method ?? 'GET', call.init.credentials]), [
      ['/twitch/social-media/api/autopilot/schedule?twitch_user_id=42', 'GET', 'same-origin'],
      ['/twitch/social-media/api/autopilot/schedule', 'PUT', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/order', 'PUT', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/103/next', 'POST', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/103/cancel', 'POST', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/104/resolve', 'POST', 'same-origin'],
      ['/twitch/social-media/api/autopilot/policy?twitch_user_id=42', 'GET', 'same-origin'],
      ['/twitch/social-media/api/autopilot/policy?twitch_user_id=42', 'PUT', 'same-origin'],
    ]);
    assert.deepEqual(JSON.parse(String(calls[7].init.body)), createScheduleQueueFixture().policy());
    assert.deepEqual(JSON.parse(String(calls[1].init.body)), { twitch_user_id: '42', paused: true, target_platforms: ['youtube'] });
    assert.deepEqual(JSON.parse(String(calls[5].init.body)), { twitch_user_id: '42', action: 'process', note: 'Weiterverarbeiten' });
    assert.deepEqual(JSON.parse(String(calls[2].init.body)), { twitch_user_id: '42', job_ids: [102, 103] });
  } finally {
    globalThis.fetch = original;
  }
});

test('fixture rejects stale queue reorder and cannot cancel started work', async () => {
  const fixture = createScheduleQueueFixture();
  await assert.rejects(fixture.api.reorder([102]));
  await assert.rejects(fixture.api.cancel(101));
  await fixture.api.next(103);
  await fixture.api.cancel(103);
  assert.equal(fixture.snapshot().jobs.find(job => job.id === 101)?.status, 'running');
  assert.equal(fixture.snapshot().jobs.find(job => job.id === 103)?.status, 'cancelled');
});

const german = plan.planText('de');

test('plan save writes schedule and policy, reloads and leaves a clean draft', async () => {
  const fixture = createScheduleQueueFixture();
  const base = await plan.loadPlan(fixture.api, '42');
  const draft = plan.planDraft(base);
  assert.equal(plan.isDirty(base, draft), false);
  draft.pipeline_mode = 'ai_edit';
  draft.review_policy = 'veto_until_deadline';
  draft.distribution_policy = 'independent_clips';
  draft.shared = plan.addTime(plan.addTime(draft.shared, 1), 1);
  assert.deepEqual(draft.shared.filter(slot => slot.weekday === 1).map(slot => slot.local_time), ['18:00', '20:00', '12:00']);
  assert.equal(plan.isDirty(base, draft), true);
  const outcome = await plan.savePlan(fixture.api, '42', base, draft);
  assert.equal(outcome.failure, undefined);
  assert.equal(plan.isDirty(outcome.state, plan.planDraft(outcome.state)), false);
  assert.equal(plan.isDirty(outcome.state, draft), false);
  assert.equal(fixture.snapshot().settings.ai_editing_enabled, true);
  assert.deepEqual(fixture.snapshot().settings.schedule_slots.slice(0, 3), [{ weekday: 1, local_time: '12:00' }, { weekday: 1, local_time: '18:00' }, { weekday: 1, local_time: '20:00' }]);
  assert.equal(fixture.policy().review_policy, 'veto_until_deadline');
  assert.equal(fixture.policy().policy_revision, 2);
  assert.deepEqual(fixture.requests.map(request => `${request.method} ${request.path}`), ['GET /autopilot/schedule', 'GET /autopilot/policy', 'PUT /autopilot/schedule', 'PUT /autopilot/policy', 'GET /autopilot/schedule']);
});

test('plan save keeps the saved part and the unsaved rest after a revision conflict', async () => {
  const fixture = createScheduleQueueFixture();
  const base = await plan.loadPlan(fixture.api);
  const stale = { ...base, policy: { ...base.policy, policy_revision: 0 } };
  const draft = plan.planDraft(stale);
  draft.lead_time_hours = 48;
  draft.review_policy = 'off';
  const outcome = await plan.savePlan(fixture.api, undefined, stale, draft);
  assert.ok(outcome.failure);
  assert.equal(outcome.partial, true);
  assert.equal(outcome.state.snapshot.settings.lead_time_hours, 48);
  assert.deepEqual(plan.schedulePatch(outcome.state, draft), {});
  assert.equal(plan.policyChange(outcome.state, draft)?.review_policy, 'off');
  assert.match(plan.failureText(outcome.failure), /inzwischen woanders geändert/);
  const fresh = await plan.loadPlan(fixture.api);
  const retry = plan.planDraft(fresh);
  retry.review_policy = 'off';
  assert.equal((await plan.savePlan(fixture.api, undefined, fresh, retry)).failure, undefined);
  assert.equal(fixture.policy().review_policy, 'off');
});

test('plan save reports success when only the refetch after a saved policy fails', async () => {
  const fixture = createScheduleQueueFixture();
  const base = await plan.loadPlan(fixture.api);
  const draft = plan.planDraft(base);
  draft.review_policy = 'off';
  const api = { ...fixture.api, load: async () => { throw new TypeError('offline'); } };
  const outcome = await plan.savePlan(api, undefined, base, draft);
  assert.equal(outcome.failure, undefined);
  assert.equal(outcome.partial, false);
  assert.equal(outcome.state.policy.review_policy, 'off');
  assert.equal(plan.policyChange(outcome.state, draft), null);
  assert.equal(fixture.policy().review_policy, 'off');
});

test('plan validation reports duplicate and empty times without a slot ceiling', () => {
  const fixture = createScheduleQueueFixture();
  const draft = plan.planDraft({ snapshot: fixture.snapshot(), policy: fixture.policy() });
  draft.shared = Array.from({ length: 7 }, (_, day) => ['06:00', '08:00', '10:00', '12:00', '14:00'].map(local_time => ({ weekday: day + 1, local_time }))).flat();
  assert.equal(draft.shared.length, 35);
  assert.deepEqual(plan.validateDraft(draft, german), []);
  draft.shared = [...draft.shared, { weekday: 2, local_time: '08:00' }, { weekday: 3, local_time: '' }];
  assert.deepEqual(plan.validateDraft(draft, german), ['Jede Zeit braucht eine Uhrzeit.', 'Dienstag 08:00 ist doppelt.']);
});

test('switching scope copies shared times and asks before merging different platform times', () => {
  const fixture = createScheduleQueueFixture();
  const base = { snapshot: fixture.snapshot(), policy: fixture.policy() };
  const draft = { ...plan.planDraft(base), target_platforms: ['youtube', 'tiktok'] };
  const split = plan.switchScope(draft, 'per_platform');
  assert.equal(split.choices, undefined);
  assert.deepEqual(split.draft.per_platform.youtube, draft.shared);
  assert.deepEqual(split.draft.per_platform.tiktok, draft.shared);
  const back = plan.switchScope(split.draft, 'shared');
  assert.equal(back.choices, undefined);
  assert.equal(back.draft.schedule_scope, 'shared');
  const different = { ...split.draft, per_platform: { ...split.draft.per_platform, tiktok: [{ weekday: 1, local_time: '08:00' }, { weekday: 1, local_time: '22:00' }] } };
  const conflict = plan.switchScope(different, 'shared');
  assert.equal(conflict.draft, different);
  assert.deepEqual(conflict.choices?.map(choice => choice.platforms), [['youtube'], ['tiktok']]);
  assert.equal(plan.policyChange(base, split.draft)?.schedule_scope, 'per_platform');
});

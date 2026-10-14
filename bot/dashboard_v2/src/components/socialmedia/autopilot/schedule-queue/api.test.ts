import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createScheduleQueueFixture } from './fixtures';

Object.defineProperty(globalThis, 'window', { value: { location: { origin: 'https://dashboard.example.test', pathname: '/twitch/social-media', hostname: 'dashboard.example.test' } }, configurable: true });

const { scheduleQueueApi } = await import('./api');

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
    assert.deepEqual(calls.map(call => [call.path, call.init.method ?? 'GET', call.init.credentials]), [
      ['/twitch/social-media/api/autopilot/schedule?twitch_user_id=42', 'GET', 'same-origin'],
      ['/twitch/social-media/api/autopilot/schedule', 'PUT', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/order', 'PUT', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/103/next', 'POST', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/103/cancel', 'POST', 'same-origin'],
      ['/twitch/social-media/api/autopilot/queue/104/resolve', 'POST', 'same-origin'],
    ]);
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

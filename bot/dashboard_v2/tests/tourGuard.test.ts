import assert from 'node:assert/strict';
import test from 'node:test';

const ORIGIN = 'https://deutsche-deadlock-community.de';
const TOUR = `${ORIGIN}/twitch/rundgang/verwaltung`;

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    location: {
      href: TOUR,
      origin: ORIGIN,
      pathname: '/twitch/rundgang/verwaltung',
      search: '',
      hostname: 'deutsche-deadlock-community.de',
    },
  },
});

const { createTourFetch, tourNavigation, TOUR_WRITE_BLOCKED_MESSAGE } = await import('../src/tourGuard');
const { livePathFromTourPath } = await import('../src/publicTour');

function recordingFetch() {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  const fetchFn = (async (url: RequestInfo | URL, init: RequestInit = {}) => {
    calls.push({ url: String(url), init });
    return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
  }) as typeof fetch;
  return { calls, fetchFn };
}

test('writes from the tour never reach the network', async () => {
  const { calls, fetchFn } = recordingFetch();
  let blocked = 0;
  const guarded = createTourFetch(fetchFn, ORIGIN, () => { blocked += 1; });
  const targets = [
    '/twitch/api/v2/streamer/disconnect-bot',
    '/twitch/api/v2/streamer/moderation/settings',
    '/twitch/api/billing/trial/start',
    '/twitch/guild-onboarding/api/select',
    '/twitch/demo/api/v2/ai/analysis',
    `${ORIGIN}/twitch/social-media/api/clips/1/clip-contest`,
    'https://admin.deutsche-deadlock-community.de/twitch/api/v2/streamer/greeting-settings',
  ];
  const methods = ['POST', 'PUT', 'PATCH', 'DELETE', 'post'];
  for (const method of methods) {
    for (const target of targets) {
      const inputs = [target, new URL(target, ORIGIN), new Request(new URL(target, ORIGIN), { method, body: '{}' })];
      for (const input of inputs) {
        const init: RequestInit | undefined = input instanceof Request
          ? undefined
          : { method, credentials: 'include', body: '{}' };
        const response = await guarded(input, init);
        assert.equal(response.status, 403);
        const body = await response.json() as { error: string; message: string };
        assert.equal(body.error, TOUR_WRITE_BLOCKED_MESSAGE);
        assert.equal(body.message, TOUR_WRITE_BLOCKED_MESSAGE);
      }
    }
  }
  assert.equal(calls.length, 0);
  assert.equal(blocked, methods.length * targets.length * 3);
});

test('reads from the tour never send credentials to live endpoints', async () => {
  const { calls, fetchFn } = recordingFetch();
  const guarded = createTourFetch(fetchFn, ORIGIN);

  const fixture = await guarded('/twitch/api/v2/streamer/moderation/settings', { credentials: 'same-origin' });
  assert.equal(fixture.status, 200);
  assert.equal(((await fixture.json()) as { global_ban_enabled: boolean }).global_ban_enabled, true);

  const billing = await guarded('/twitch/api/billing/status', { credentials: 'same-origin' });
  assert.equal(billing.status, 404);
  const onboarding = await guarded('/twitch/guild-onboarding/api/session', { credentials: 'same-origin' });
  assert.equal(onboarding.status, 404);

  await guarded('/twitch/api/v2/category-collector?days=7', { credentials: 'same-origin' });
  await guarded('/twitch/demo/api/v2/overview?days=30', { credentials: 'include' });
  await guarded(new Request(`${ORIGIN}/twitch/api/v2/streamer/unknown?x=1`, { credentials: 'include' }));
  await guarded('https://static.example/font.woff2', { credentials: 'include' });

  assert.deepEqual(calls.map(call => call.url), [
    `${ORIGIN}/twitch/demo/api/v2/category-collector?days=7`,
    `${ORIGIN}/twitch/demo/api/v2/overview?days=30`,
    `${ORIGIN}/twitch/demo/api/v2/streamer/unknown?x=1`,
    'https://static.example/font.woff2',
  ]);
  for (const call of calls) {
    assert.equal(call.init.credentials, 'omit');
    assert.ok(!new URL(call.url).pathname.startsWith('/twitch/api/'));
  }
});

test('tour navigation blocks login, checkout and admin targets', () => {
  const block = { kind: 'block' };
  assert.deepEqual(tourNavigation('/twitch/auth/login?next=%2Ftwitch%2Fdashboard', TOUR), block);
  assert.deepEqual(tourNavigation('/twitch/abbo/bezahlen?plan_id=studio_ki&cycle=1', TOUR), block);
  assert.deepEqual(tourNavigation('/twitch/social-media/oauth/start/tiktok', TOUR), block);
  assert.deepEqual(tourNavigation('/twitch/api/v2/streamer/private-vod-clips/1/download', TOUR), block);
  assert.deepEqual(tourNavigation('/twitch/guild-onboarding', TOUR), block);
  assert.deepEqual(tourNavigation('https://admin.deutsche-deadlock-community.de/twitch/admin', TOUR), block);
  assert.deepEqual(tourNavigation('https://id.twitch.tv/oauth2/authorize', TOUR), block);
  assert.deepEqual(tourNavigation('data:text/html,hi', TOUR), block);
  assert.deepEqual(tourNavigation('/twitch/pricing', TOUR), { kind: 'rewrite', href: `${ORIGIN}/twitch/rundgang/pricing` });
  assert.deepEqual(tourNavigation('/twitch/rundgang/uplink', TOUR), { kind: 'allow' });
  assert.deepEqual(tourNavigation('/twitch/faq', TOUR), { kind: 'allow' });
  assert.deepEqual(tourNavigation('https://discord.gg/deadlock', TOUR), { kind: 'allow' });
});

test('tour paths resolve only to allowlisted pages', () => {
  assert.equal(livePathFromTourPath('/twitch/rundgang/verwaltung'), '/twitch/verwaltung');
  assert.equal(livePathFromTourPath('/twitch/rundgang/social-media/konten'), '/twitch/social-media/konten');
  assert.equal(livePathFromTourPath('/twitch/rundgang/guild-onboarding'), '/twitch/dashboard');
  assert.equal(livePathFromTourPath('/twitch/rundgang/kategorie'), '/twitch/dashboard');
  assert.equal(livePathFromTourPath('/twitch/rundgang/admin'), '/twitch/dashboard');
  assert.equal(livePathFromTourPath('/twitch/analyse'), null);
});

import assert from 'node:assert/strict';
import test from 'node:test';

(globalThis as typeof globalThis & {
  window: {
    __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown>;
  };
}).window = {
  __TWITCH_DASHBOARD_RUNTIME__: {
    apiBase: '/twitch/api/v2/../admin',
    demoMode: true,
    allowedDemoProfiles: ['midcore_live'],
    defaultDemoProfile: 'midcore_live',
  },
};

const runtimeConfigModule = await import('../src/runtimeConfig');

test('falls back to the live API base for non-allowlisted runtime config values', () => {
  assert.equal(runtimeConfigModule.dashboardRuntimeConfig.apiBase, runtimeConfigModule.LIVE_API_BASE);
});

test('requires both demo route and demo namespace for effective demo mode', () => {
  const demoConfig = {
    apiBase: runtimeConfigModule.DEMO_API_BASE,
    demoMode: true,
    allowedDemoProfiles: ['midcore_live'],
    defaultDemoProfile: 'midcore_live',
  };

  assert.equal(
    runtimeConfigModule.resolveEffectiveDemoMode({
      pathname: '/twitch/dashboard-v2',
      runtimeConfig: demoConfig,
    }),
    false
  );

  assert.equal(
    runtimeConfigModule.resolveEffectiveDemoMode({
      pathname: '/twitch/demo',
      runtimeConfig: demoConfig,
    }),
    true
  );
});

test('does not treat demoMode alone as a valid demo runtime', () => {
  assert.equal(
    runtimeConfigModule.hasDemoRuntimeConfig({
      apiBase: runtimeConfigModule.LIVE_API_BASE,
      demoMode: true,
      allowedDemoProfiles: [],
      defaultDemoProfile: null,
    }),
    false
  );
});

test('partner token is captured once, scrubbed from the URL, and sent as a header', async () => {
  const previousWindow = globalThis.window;
  const previousFetch = globalThis.fetch;
  const sessionState = new Map<string, string>();
  const location = new URL(
    'https://dashboard.example/twitch/dashboard?partner_token=abc123&streamer=midcore_live'
  );

  const sessionStorage = {
    getItem(key: string) {
      return sessionState.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      sessionState.set(key, String(value));
    },
    removeItem(key: string) {
      sessionState.delete(key);
    },
    clear() {
      sessionState.clear();
    },
    key(index: number) {
      return Array.from(sessionState.keys())[index] ?? null;
    },
    get length() {
      return sessionState.size;
    },
  };

  const history = {
    replaceState(_: unknown, __: string, nextUrl?: string | URL | null) {
      const resolved = new URL(String(nextUrl ?? location.href), location.origin);
      location.href = resolved.href;
      location.pathname = resolved.pathname;
      location.search = resolved.search;
      location.hash = resolved.hash;
    },
  };

  globalThis.window = {
    __TWITCH_DASHBOARD_RUNTIME__: {
      apiBase: runtimeConfigModule.LIVE_API_BASE,
      demoMode: false,
      allowedDemoProfiles: [],
      defaultDemoProfile: null,
    },
    location,
    history,
    sessionStorage,
  } as typeof globalThis.window;

  const requests: Array<{ url: string; headers: Headers; method: string }> = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({
      url: typeof input === 'string' ? input : input.toString(),
      headers: new Headers(init?.headers),
      method: String(init?.method || 'GET').toUpperCase(),
    });
    return {
      ok: true,
      status: 200,
      json: async () => ({ ok: true }),
    } as Response;
  }) as typeof fetch;

  try {
    const clientModule = await import('../src/api/client');

    assert.equal(clientModule.getPartnerToken(), 'abc123');
    assert.equal(sessionStorage.getItem('partner_token'), 'abc123');
    assert.equal(location.search.includes('partner_token='), false);

    const builtUrl = clientModule.buildApiUrl('/internal-home', { streamer: 'midcore_live' });
    assert.equal(builtUrl.includes('partner_token='), false);

    await clientModule.fetchApi('/internal-home', { streamer: 'midcore_live' });
    const apiRequest = requests.at(-1);
    assert.equal(apiRequest?.url.includes('partner_token='), false);
    assert.equal(apiRequest?.headers.get('X-Partner-Token'), 'abc123');

    await clientModule.fetchAdminAffiliates();
    const adminRequest = requests.at(-1);
    assert.equal(adminRequest?.url.includes('partner_token='), false);
    assert.equal(adminRequest?.headers.get('X-Partner-Token'), 'abc123');
    assert.equal(adminRequest?.method, 'GET');

    await clientModule.toggleAffiliate('midcore_live', 'csrf-123');
    const toggleRequest = requests.at(-1);
    assert.equal(toggleRequest?.url.includes('partner_token='), false);
    assert.equal(toggleRequest?.headers.get('X-Partner-Token'), 'abc123');
    assert.equal(toggleRequest?.headers.get('X-CSRF-Token'), 'csrf-123');
    assert.equal(toggleRequest?.method, 'POST');
  } finally {
    globalThis.fetch = previousFetch;
    globalThis.window = previousWindow;
  }
});

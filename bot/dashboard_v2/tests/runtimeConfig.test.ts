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

test('cookie-based dashboard requests stay same-origin without token bootstrapping', async () => {
  const previousWindow = globalThis.window;
  const previousFetch = globalThis.fetch;
  const location = new URL(
    'https://dashboard.example/twitch/dashboard?partner_token=abc123&streamer=midcore_live'
  );
  const initialHref = location.href;

  const sessionStorage = {
    getItem() {
      throw new Error('sessionStorage must not be used for dashboard auth bootstrap');
    },
    setItem() {
      throw new Error('sessionStorage must not be used for dashboard auth bootstrap');
    },
    removeItem() {
      throw new Error('sessionStorage must not be used for dashboard auth bootstrap');
    },
    clear() {
      throw new Error('sessionStorage must not be used for dashboard auth bootstrap');
    },
    key() {
      throw new Error('sessionStorage must not be used for dashboard auth bootstrap');
    },
    get length() {
      return 0;
    },
  };

  const history = {
    replaceState(_: unknown, __: string, nextUrl?: string | URL | null) {
      throw new Error(`history.replaceState must not be called during bootstrap: ${String(nextUrl ?? '')}`);
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

  const requests: Array<{ url: string; headers: Headers; method: string; credentials?: RequestCredentials }> = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({
      url: typeof input === 'string' ? input : input.toString(),
      headers: new Headers(init?.headers),
      method: String(init?.method || 'GET').toUpperCase(),
      credentials: init?.credentials,
    });
    return {
      ok: true,
      status: 200,
      json: async () => ({ ok: true }),
    } as Response;
  }) as typeof fetch;

  try {
    const clientModule = await import('../src/api/client');

    assert.equal(location.href, initialHref);

    const builtUrl = clientModule.buildApiUrl('/internal-home', { streamer: 'midcore_live' });
    assert.equal(builtUrl.includes('partner_token='), false);

    await clientModule.fetchApi('/internal-home', { streamer: 'midcore_live' });
    const apiRequest = requests.at(-1);
    assert.equal(apiRequest?.url.includes('partner_token='), false);
    assert.equal(apiRequest?.headers.get('X-Partner-Token'), null);
    assert.equal(apiRequest?.credentials, 'same-origin');

    await clientModule.fetchAdminAffiliates();
    const adminRequest = requests.at(-1);
    assert.equal(adminRequest?.url.includes('partner_token='), false);
    assert.equal(adminRequest?.headers.get('X-Partner-Token'), null);
    assert.equal(adminRequest?.method, 'GET');
    assert.equal(adminRequest?.credentials, 'same-origin');

    await clientModule.toggleAffiliate('midcore_live', 'csrf-123');
    const toggleRequest = requests.at(-1);
    assert.equal(toggleRequest?.url.includes('partner_token='), false);
    assert.equal(toggleRequest?.headers.get('X-Partner-Token'), null);
    assert.equal(toggleRequest?.headers.get('X-CSRF-Token'), 'csrf-123');
    assert.equal(toggleRequest?.method, 'POST');
    assert.equal(toggleRequest?.credentials, 'same-origin');
  } finally {
    globalThis.fetch = previousFetch;
    globalThis.window = previousWindow;
  }
});

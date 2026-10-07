import test from 'node:test';
import assert from 'node:assert/strict';

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: { location: new URL('https://example.com/social-media') },
});
const { submitClipToContest } = await import('../src/api/socialMedia');

test('Clip-Einreichung übernimmt Session-CSRF und sendet keine freie Akteur-ID', async () => {
  const originalFetch = globalThis.fetch;
  try {
    for (const key of ['csrfToken', 'csrf_token']) {
      const calls: Array<{ url: string; init?: RequestInit }> = [];
      globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
        calls.push({ url: String(input), init });
        return new Response(JSON.stringify(init?.method === 'POST'
          ? { status: 'accepted', message: 'Clip ist eingereicht.', clip_url: 'https://clips.twitch.tv/AbcDef' }
          : { authenticated: true, [key]: 'synthetischer-csrf-wert' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        });
      }) as typeof fetch;
      const result = await submitClipToContest(7);
      assert.equal(calls.length, 2);
      assert.equal(new URL(calls[0].url).pathname, '/twitch/api/v2/auth-status');
      assert.equal(calls[1].url, '/social-media/api/clips/7/clip-contest');
      assert.equal(calls[1].init?.credentials, 'same-origin');
      assert.equal(new Headers(calls[1].init?.headers).get('X-CSRF-Token'), 'synthetischer-csrf-wert');
      assert.equal(calls[1].init?.body, undefined);
      assert.equal(result.status, 'accepted');
      assert.equal(result.ok, true);
    }
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('Fehlgeschlagene Sessionprüfung sendet keine Clip-Einreichung', async () => {
  const originalFetch = globalThis.fetch;
  let requests = 0;
  globalThis.fetch = (async () => {
    requests += 1;
    throw new Error('Sessionprüfung nicht verfügbar');
  }) as typeof fetch;
  try {
    await assert.rejects(submitClipToContest(7));
    assert.equal(requests, 1);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { fetchClipCommandSettings, toggleClipCommand } from '../src/api/clipCommand';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

function mockFetch(status: number, body: unknown) {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  globalThis.fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
    };
  }) as unknown as typeof fetch;
  return calls;
}

test('liest den Status ohne Streamer-Parameter aus der Session', async () => {
  const calls = mockFetch(200, { clip_command_enabled: true });
  const data = await fetchClipCommandSettings();
  assert.equal(calls[0].url, '/twitch/api/v2/streamer/clip-command-settings');
  // Ohne Cookie wüsste der Server nicht, wessen Kanal gemeint ist.
  assert.equal((calls[0].init as { credentials?: string }).credentials, 'same-origin');
  assert.equal(data.clip_command_enabled, true);
});

test('schaltet !clip per POST ab', async () => {
  const calls = mockFetch(200, { ok: true, clip_command_enabled: false });
  const data = await toggleClipCommand(false);
  assert.equal(calls[0].url, '/twitch/api/v2/streamer/clip-command-settings');
  assert.equal(calls[0].init.method, 'POST');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), { clip_command_enabled: false });
  assert.equal(data.clip_command_enabled, false);
});

test('hängt den Streamer als Query an, wenn Admins fremde Kanäle setzen', async () => {
  const calls = mockFetch(200, { ok: true, clip_command_enabled: true });
  await toggleClipCommand(true, 'early salty');
  assert.equal(
    calls[0].url,
    '/twitch/api/v2/streamer/clip-command-settings?streamer=early%20salty',
  );
});

test('macht aus einem Fehler-Body eine lesbare Meldung', async () => {
  mockFetch(403, { error: 'Kein Zugriff auf diesen Kanal' });
  await assert.rejects(
    () => fetchClipCommandSettings(),
    /Kein Zugriff auf diesen Kanal/,
  );
});

test('fällt auf den Statuscode zurück, wenn der Body kein JSON ist', async () => {
  globalThis.fetch = (async () => ({
    ok: false,
    status: 500,
    json: async () => {
      throw new Error('kein JSON');
    },
  })) as unknown as typeof fetch;
  await assert.rejects(() => fetchClipCommandSettings(), /HTTP 500/);
});

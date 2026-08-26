import assert from 'node:assert/strict';
import test from 'node:test';

const globalState = globalThis as typeof globalThis & {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
const vorherigesFenster = globalState.window;
globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const {
  reconnectWaitEingabe,
  reconnectWaitPayload,
  saveUplinkReconnectWait,
  UPLINK_RECONNECT_WAIT_TEXT,
} = await import('../src/api/uplink');
globalState.window = vorherigesFenster;

test('Wartezeit gilt nur fuer unerwartete Abrisse und wird nicht lokal geklemmt', () => {
  assert.equal(reconnectWaitEingabe(90), '90');
  assert.equal(reconnectWaitEingabe(0), '0');
  assert.equal(reconnectWaitEingabe(undefined), '');
  assert.equal(reconnectWaitPayload('0'), 0);
  assert.equal(reconnectWaitPayload('300'), 300);
  assert.equal(reconnectWaitPayload('301'), 301);
  assert.equal(reconnectWaitPayload('30.5'), null);
  assert.equal(reconnectWaitPayload('-1'), null);
  assert.equal(reconnectWaitPayload(''), null);
  assert.match(UPLINK_RECONNECT_WAIT_TEXT, /unerwarteten Internetabriss/);
  assert.match(UPLINK_RECONNECT_WAIT_TEXT, /OBS/);
});

test('die Wartezeit wird mit dem bestehenden Proxy gespeichert', async () => {
  const vorher = globalThis.fetch;
  let gesendeterRumpf: unknown;
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/twitch/api/v2/uplink/reconnect-wait');
    gesendeterRumpf = JSON.parse(String(init?.body));
    return new Response(JSON.stringify({ reconnect_wait_s: 0, reconnect_wait_max_s: 300 }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const antwort = await saveUplinkReconnectWait(0);
    assert.deepEqual(gesendeterRumpf, { reconnect_wait_s: 0 });
    assert.deepEqual(antwort, { reconnect_wait_s: 0, reconnect_wait_max_s: 300 });
  } finally {
    globalThis.fetch = vorher;
  }
});

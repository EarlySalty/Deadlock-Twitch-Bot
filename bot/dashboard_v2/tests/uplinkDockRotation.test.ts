import assert from 'node:assert/strict';
import test from 'node:test';

const globalState = globalThis as typeof globalThis & {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
const previousWindow = globalState.window;
globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { newestDockRotation, rotateUplinkDockToken } = await import('../src/api/uplink');
globalState.window = previousWindow;
type DockRotation = import('../src/api/uplink').DockRotation;

function rotation(generation: number): DockRotation {
  return {
    generation,
    dock_urls: {
      chat: `https://relay.test/dock/chat?t=synthetic-${generation}`,
      activity: `https://relay.test/dock/activity?t=synthetic-${generation}`,
      stream_info: `https://relay.test/dock/stream-info?t=synthetic-${generation}`,
      points: `https://relay.test/dock/points?t=synthetic-${generation}`,
    },
  };
}

test('out-of-order rotation responses retain only the highest generation', async () => {
  const originalFetch = globalThis.fetch;
  const pending: Array<(response: Response) => void> = [];
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/twitch/api/v2/uplink/dock-token/rotate');
    assert.equal(init?.method, 'POST');
    return new Promise<Response>((resolve) => pending.push(resolve));
  };
  try {
    let current: DockRotation | null = null;
    const apply = (incoming: DockRotation) => {
      current = newestDockRotation(current, incoming);
    };
    const first = rotateUplinkDockToken().then(apply);
    const second = rotateUplinkDockToken().then(apply);
    pending[1](Response.json(rotation(2)));
    await second;
    assert.deepEqual(current, rotation(2));
    pending[0](Response.json(rotation(1)));
    await first;
    assert.deepEqual(current, rotation(2));
    assert.deepEqual(newestDockRotation(current, rotation(3)), rotation(3));
    assert.equal(newestDockRotation(current, rotation(2)), current);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('rotation responses require a positive exact generation before displaying links', async () => {
  const originalFetch = globalThis.fetch;
  try {
    for (const generation of [undefined, null, 0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
      globalThis.fetch = async () => Response.json({ ...rotation(1), generation });
      await assert.rejects(rotateUplinkDockToken(), /nicht als aktuell bestätigt/);
    }
    globalThis.fetch = async () => Response.json(rotation(17));
    assert.deepEqual(await rotateUplinkDockToken(), rotation(17));
  } finally {
    globalThis.fetch = originalFetch;
  }
});

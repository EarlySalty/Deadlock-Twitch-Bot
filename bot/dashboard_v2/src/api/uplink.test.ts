import assert from 'node:assert/strict';
import test from 'node:test';

const browserGlobal = globalThis as unknown as {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
browserGlobal.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { fetchUplinkDestinations, saveUplinkDestination } = await import('./uplink');

test('Zielspeichern überträgt nur Plattform und Qualität ohne Tonwahl', async () => {
  const vorher = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/twitch/api/v2/uplink/destinations');
    assert.equal(init?.method, 'PUT');
    assert.equal(init?.credentials, 'same-origin');
    assert.deepEqual(JSON.parse(String(init?.body)), {
      platform: 'twitch', profil: '1080p60',
    });
    return Response.json({ destinations: [] });
  };
  try {
    assert.deepEqual(await saveUplinkDestination({ platform: 'twitch', profil: '1080p60' }), {
      destinations: [],
    });
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Zielstatus erhält die empfangene Spurzahl und VOD-Zuordnung unverändert', async () => {
  const vorher = globalThis.fetch;
  try {
    for (const audio of [
      { source_tracks: null, vod: null },
      { source_tracks: 1, vod: 'gleich' },
      { source_tracks: 2, vod: 'zweite_spur' },
      { source_tracks: 3, vod: 'zweite_spur' },
    ]) {
      const antwort = { destinations: [{ platform: 'twitch', enabled: true, rtmp_url: '', audio }] };
      globalThis.fetch = async (input, init) => {
        assert.equal(String(input), '/twitch/api/v2/uplink/destinations');
        assert.equal(init?.credentials, 'same-origin');
        return Response.json(antwort);
      };
      assert.deepEqual(await fetchUplinkDestinations(), antwort);
    }
  } finally {
    globalThis.fetch = vorher;
  }
});

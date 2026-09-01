import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import type { AdManagerResponse, AdManagerSettingsInput } from '../src/api/adManager';

(globalThis as typeof globalThis & { window: unknown }).window = {
  __TWITCH_DASHBOARD_RUNTIME__: {},
  location: new URL('https://deutsche-deadlock-community.de/twitch/verwaltung#werbung'),
};

const {
  AD_DURATION_OPTIONS,
  adManagerReauthUrl,
  adManagerSettingsInput,
  fetchAdManager,
  normalizeAdManagerSettings,
  queueAdManagerAction,
  saveAdManagerSettings,
} = await import('../src/api/adManager');
const { getPreviewPathFixture } = await import('../src/preview/fixtures');
const adManagerSectionSource = readFileSync(
  join(import.meta.dirname, '../src/components/verwaltung/AdManagerSection.tsx'),
  'utf8',
);

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

const response: AdManagerResponse = {
  settings: {
    enabled: true,
    strategy: 'smart',
    adDurationSeconds: 90,
    minIntervalMinutes: 45,
    startupDelayMinutes: 20,
    quietWindowMinutes: 8,
    actionLeadSeconds: 60,
    updatedAt: '2026-09-01T10:00:00Z',
  },
  status: {
    isLive: true,
    nextAdAt: '2026-09-01T10:30:00Z',
    lastAdAt: null,
    durationSeconds: 90,
    prerollFreeSeconds: 1200,
    snoozeCount: 2,
    snoozeRefreshAt: '2026-09-01T11:00:00Z',
    observedAt: '2026-09-01T10:00:00Z',
    workerHealthy: true,
    workerHeartbeatAt: '2026-09-01T09:59:58Z',
    lastAction: null,
    scopes: { read: true, snooze: true, commercial: true },
  },
};

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

test('liest den Werbemanager aus der angemeldeten Session', async () => {
  const calls = mockFetch(200, response);
  const result = await fetchAdManager();

  assert.equal(calls[0].url, '/twitch/api/v2/streamer/ad-manager');
  assert.equal(calls[0].init.credentials, 'same-origin');
  assert.equal(result.settings.strategy, 'smart');
  assert.equal(result.status.workerHealthy, true);
  assert.equal(result.status.workerHeartbeatAt, '2026-09-01T09:59:58Z');
});

test('übernimmt einen ausgefallenen Worker ehrlich statt Aktivität abzuleiten', async () => {
  mockFetch(200, {
    ...response,
    status: {
      ...response.status,
      isLive: true,
      workerHealthy: false,
      workerHeartbeatAt: '2026-09-01T09:52:00Z',
    },
  });

  const result = await fetchAdManager();

  assert.equal(result.settings.enabled, true);
  assert.equal(result.status.isLive, true);
  assert.equal(result.status.workerHealthy, false);
  assert.equal(result.status.workerHeartbeatAt, '2026-09-01T09:52:00Z');
});

test('reicht das AbortSignal weiter und verschluckt den Abbruch nicht', async () => {
  const controller = new AbortController();
  let receivedSignal: AbortSignal | null | undefined;
  globalThis.fetch = ((_url: string, init: RequestInit) => {
    receivedSignal = init.signal;
    return new Promise((_resolve, reject) => {
      init.signal?.addEventListener('abort', () => {
        reject(new DOMException('Abgebrochen', 'AbortError'));
      }, { once: true });
    });
  }) as unknown as typeof fetch;

  const pending = fetchAdManager(controller.signal);
  controller.abort();

  await assert.rejects(
    pending,
    (error: unknown) => error instanceof DOMException && error.name === 'AbortError',
  );
  assert.equal(receivedSignal, controller.signal);
});

test('speichert nur normalisierte Eingabefelder ohne Server-Zeitstempel', async () => {
  const calls = mockFetch(200, response);
  const draft: AdManagerSettingsInput = {
    enabled: true,
    strategy: 'snooze',
    adDurationSeconds: 77,
    minIntervalMinutes: 999,
    startupDelayMinutes: -4,
    quietWindowMinutes: 8.6,
    actionLeadSeconds: 2,
  };

  await saveAdManagerSettings(draft);

  assert.equal(calls[0].init.method, 'POST');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), {
    enabled: true,
    strategy: 'snooze',
    adDurationSeconds: 90,
    minIntervalMinutes: 180,
    startupDelayMinutes: 0,
    quietWindowMinutes: 9,
    actionLeadSeconds: 10,
  });
});

test('Erstnutzer können die serverseitigen Standardwerte initial speichern', async () => {
  const initialSettings = {
    ...response.settings,
    updatedAt: null,
  };
  const input = adManagerSettingsInput(initialSettings);
  const calls = mockFetch(200, response);

  await saveAdManagerSettings(input);

  assert.equal('updatedAt' in input, false);
  assert.equal('updatedAt' in JSON.parse(String(calls[0].init.body)), false);
  assert.match(
    adManagerSectionSource,
    /needsInitialSave\s*=\s*data\?\.settings\.updatedAt\s*===\s*null/,
    'updatedAt:null muss den initialen Speichervorgang markieren',
  );
  assert.match(
    adManagerSectionSource,
    /needsInitialSave\s*\|\|\s*!settingsEqual\(draft,\s*baseline\)/,
    'Initialspeichern darf nicht von einer vorherigen Draft-Änderung abhängen',
  );
  assert.match(
    adManagerSectionSource,
    /disabled=\{!dirty\s*\|\|\s*saving\}/,
    'der Speichern-Knopf muss dem Initialspeicher-Zustand folgen',
  );
});

test('reiht Snooze und Werbung ein, ohne eine Ausführung zu behaupten', async () => {
  const calls = mockFetch(202, { queued: true });

  const snoozeKey = '11111111-1111-4111-8111-111111111111';
  const commercialKey = '22222222-2222-4222-8222-222222222222';
  const snooze = await queueAdManagerAction({ action: 'snooze' }, snoozeKey);
  const commercial = await queueAdManagerAction(
    { action: 'commercial', durationSeconds: 150 },
    commercialKey,
  );

  assert.equal(calls[0].url, '/twitch/api/v2/streamer/ad-manager/action');
  assert.deepEqual(JSON.parse(String(calls[0].init.body)), {
    action: 'snooze',
    idempotencyKey: snoozeKey,
  });
  assert.deepEqual(JSON.parse(String(calls[1].init.body)), {
    action: 'commercial',
    durationSeconds: 150,
    idempotencyKey: commercialKey,
  });
  assert.deepEqual(snooze, { queued: true });
  assert.deepEqual(commercial, { queued: true });
  for (const result of [snooze, commercial]) {
    assert.deepEqual(Object.keys(result), ['queued']);
    assert.equal('success' in result, false);
    assert.equal('executed' in result, false);
    assert.equal('outcome' in result, false);
  }
});

test('Preview-Fixture enthält den echten Worker-Health-Vertrag', () => {
  const fixture = getPreviewPathFixture(
    '/twitch/api/v2/streamer/ad-manager',
  ) as AdManagerResponse;

  assert.equal(typeof fixture.status.workerHealthy, 'boolean');
  assert.equal(typeof fixture.status.workerHeartbeatAt, 'string');
  assert.match(fixture.status.workerHeartbeatAt ?? '', /^\d{4}-\d{2}-\d{2}T/);
});

test('normalisiert alle Zahlen auf sichere und von Twitch unterstützte Werte', () => {
  assert.deepEqual(AD_DURATION_OPTIONS, [30, 60, 90, 120, 150, 180]);
  assert.deepEqual(
    normalizeAdManagerSettings({
      enabled: false,
      strategy: 'monitor',
      adDurationSeconds: Number.NaN,
      minIntervalMinutes: 7.7,
      startupDelayMinutes: 22.4,
      quietWindowMinutes: 70,
      actionLeadSeconds: 87.8,
    }),
    {
      enabled: false,
      strategy: 'monitor',
      adDurationSeconds: 90,
      minIntervalMinutes: 8,
      startupDelayMinutes: 22,
      quietWindowMinutes: 60,
      actionLeadSeconds: 88,
    },
  );
  assert.equal(
    normalizeAdManagerSettings({
      ...response.settings,
      strategy: 'unbekannt' as 'monitor',
    }).strategy,
    'monitor',
  );
});

test('zeigt Serverfehler nicht als erfolgreichen Speicherlauf', async () => {
  mockFetch(403, { message: 'Twitch-Berechtigung fehlt' });
  await assert.rejects(
    () => fetchAdManager(),
    /Twitch-Berechtigung fehlt/,
  );
});

test('erzwingt beim erneuten Verbinden das vollständige Dashboard-Reauth-Profil', () => {
  assert.equal(
    adManagerReauthUrl('/twitch/raid/auth?next=%2Ftwitch%2Fverwaltung'),
    '/twitch/raid/auth?next=%2Ftwitch%2Fverwaltung&scope_profile=dashboard_reauth',
  );
  assert.equal(
    adManagerReauthUrl('/twitch/raid/auth?scope_profile=auto'),
    '/twitch/raid/auth?scope_profile=dashboard_reauth',
  );
  assert.equal(
    adManagerReauthUrl('https://fremd.example/auth'),
    '/twitch/raid/auth?scope_profile=dashboard_reauth',
  );
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

import {
  CLIP_VORBEREITUNG_POLL_MS,
  MAX_MANUELLER_UPLOAD_BYTES,
  freigabeAktion,
  istVorbereitungAktiv,
  pruefeManuellenUpload,
  rueckmeldungNachVorbereitungsfehler,
  sichereVorbereitungVorMutation,
  vorbereitungSchritte,
  verwerfenWarnung,
} from './clipVorbereitung';
import type { ClipPreparation, ClipPreparationState } from '../../types/socialMedia';
import { getSocialMediaPreviewFixture } from '../../preview/fixtures';
import { LanguageProvider } from '../../context/LanguageContext';

(globalThis as { React?: typeof React }).React = React;

function preparation(state: ClipPreparationState): ClipPreparation {
  return {
    clip_db_id: 42,
    state,
    source_ready: state === 'source_ready' || state === 'rendering' || state === 'preview_ready',
    preview_ready: state === 'preview_ready',
    preview_url: state === 'preview_ready' ? '/social-media/previews/42.mp4' : null,
    download_url: state === 'preview_ready' ? '/social-media/previews/42/download' : null,
    error_code: state === 'failed' ? 'render_failed' : null,
    error_message: state === 'failed' ? 'Renderer nicht erreichbar' : null,
    requested_at: '2026-09-01T10:00:00Z',
    started_at: '2026-09-01T10:00:01Z',
    completed_at: state === 'preview_ready' || state === 'failed' ? '2026-09-01T10:00:05Z' : null,
    updated_at: '2026-09-01T10:00:05Z',
  };
}

test('Polling läuft nur, solange die Vorbereitung weiterarbeiten kann', () => {
  for (const state of ['pending', 'materializing', 'source_ready', 'rendering'] as const) {
    assert.equal(istVorbereitungAktiv(state), true, state);
  }
  for (const state of ['preview_ready', 'failed'] as const) {
    assert.equal(istVorbereitungAktiv(state), false, state);
  }
  assert.equal(CLIP_VORBEREITUNG_POLL_MS, 2500);
});

test('manueller Upload akzeptiert nur MP4 bis einschließlich 200 MB', () => {
  assert.equal(
    pruefeManuellenUpload({ name: 'clip.mov', type: 'video/quicktime', size: 1024 }),
    'format',
  );
  assert.equal(
    pruefeManuellenUpload({ name: 'clip.mp4', type: '', size: MAX_MANUELLER_UPLOAD_BYTES }),
    null,
  );
  assert.equal(
    pruefeManuellenUpload({
      name: 'clip.mp4',
      type: 'video/mp4',
      size: MAX_MANUELLER_UPLOAD_BYTES + 1,
    }),
    'zu_gross',
  );
});

test('die kompakte Pipeline unterscheidet Quelle, Render und prüfbare Vorschau', () => {
  assert.deepEqual(
    vorbereitungSchritte(preparation('pending')).map((schritt) => schritt.status),
    ['wartet', 'wartet', 'wartet'],
  );
  assert.deepEqual(
    vorbereitungSchritte(preparation('materializing')).map((schritt) => schritt.status),
    ['aktiv', 'wartet', 'wartet'],
  );
  assert.deepEqual(
    vorbereitungSchritte(preparation('source_ready')).map((schritt) => schritt.status),
    ['fertig', 'aktiv', 'wartet'],
  );
  assert.deepEqual(
    vorbereitungSchritte(preparation('preview_ready')).map((schritt) => schritt.status),
    ['fertig', 'fertig', 'fertig'],
  );
});

test('Freigabe bleibt ohne Ziel gesperrt und heißt im Testbetrieb eindeutig anders', () => {
  assert.deepEqual(freigabeAktion(false, 0, false, true), {
    disabled: true,
    label: 'Für später freigeben',
    hinweis: 'Wähle mindestens eine Zielplattform.',
  });
  assert.deepEqual(freigabeAktion(false, 1, false, true), {
    disabled: false,
    label: 'Für später freigeben',
    hinweis: null,
  });
  assert.equal(
    freigabeAktion(true, 1, false, true).label,
    'Zur Veröffentlichung freigeben',
  );
  assert.equal(freigabeAktion(true, 1, true, true).disabled, true);
});

test('Freigabe ist fail-closed, bis Plan und gerenderte Vorschau sicher vorliegen', () => {
  assert.deepEqual(freigabeAktion(null, 1, false, true), {
    disabled: true,
    label: 'Freigabemodus wird geladen…',
    hinweis: 'Freigabemodus wird geladen.',
  });
  assert.deepEqual(freigabeAktion(false, 1, false, false), {
    disabled: true,
    label: 'Für später freigeben',
    hinweis: 'Bereite den Clip zuerst auf und prüfe die Vorschau.',
  });
  assert.equal(
    freigabeAktion(true, 1, false, false).disabled,
    true,
    'auch im Live-Modus darf ein ungeprüfter Clip nicht rausgehen',
  );
});

test('ein fehlgeschlagenes Neurendern stellt die zuvor gültige Vorschau wieder her', () => {
  const vorherigeVorschau = preparation('preview_ready');

  assert.equal(
    sichereVorbereitungVorMutation(vorherigeVorschau, true),
    vorherigeVorschau,
    'die Mutation merkt sich die weiterhin abspielbare Vorschau',
  );
  assert.equal(
    rueckmeldungNachVorbereitungsfehler(vorherigeVorschau),
    vorherigeVorschau,
    'nach dem POST-Fehler darf die Freigabe nicht dauerhaft gesperrt bleiben',
  );
  assert.equal(
    sichereVorbereitungVorMutation(vorherigeVorschau, false),
    null,
    'eine bereits kaputte Vorschau darf bei einem Fehler nicht wieder freigegeben werden',
  );
});

test('ein Konflikt beim Verwerfen warnt vor einem möglicherweise weiterlaufenden Upload', () => {
  assert.equal(
    verwerfenWarnung({
      clip_db_id: 42,
      discarded: true,
      pending_stopped: 2,
      already_running: 1,
    }),
    'Mindestens ein laufender Upload konnte möglicherweise nicht mehr gestoppt werden. Prüfe die Zielplattformen.',
  );
  assert.equal(
    verwerfenWarnung({
      clip_db_id: 42,
      discarded: true,
      pending_stopped: 2,
      already_running: 0,
    }),
    null,
  );
});

test('Preparation-API nutzt den festen GET- und POST-Vertrag', async (t) => {
  const originalWindow = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TWITCH_DASHBOARD_RUNTIME__: {},
      location: { pathname: '/', origin: 'http://localhost' },
    },
  });
  t.after(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
  });

  const {
    clipPreparationMediaUrl,
    fetchClipPreparation,
    requestClipPreparation,
  } = await import('../../api/socialMedia');
  const aufrufe: Array<{ url: string; method: string }> = [];
  const antwort = preparation('preview_ready');
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    aufrufe.push({
      url: String(input),
      method: (init?.method ?? 'GET').toUpperCase(),
    });
    return new Response(JSON.stringify(antwort), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  assert.deepEqual(await fetchClipPreparation(42), antwort);
  assert.deepEqual(await requestClipPreparation(42), antwort);
  assert.deepEqual(aufrufe, [
    { url: '/social-media/api/admin/clips/42/preparation', method: 'GET' },
    { url: '/social-media/api/admin/clips/42/preparation', method: 'POST' },
  ]);
  assert.equal(
    clipPreparationMediaUrl(42),
    '/social-media/api/admin/clips/42/preparation/media',
  );
  assert.equal(
    clipPreparationMediaUrl(42, true),
    '/social-media/api/admin/clips/42/preparation/media?download=1',
  );
});

test('manueller Plattformabgleich nennt Clip, Streamer und nur geprüfte Ziele', async (t) => {
  const originalWindow = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TWITCH_DASHBOARD_RUNTIME__: {},
      location: { pathname: '/', origin: 'http://localhost' },
    },
  });
  t.after(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
  });

  const { markClipPublished } = await import('../../api/socialMedia');
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/social-media/api/mark-uploaded');
    assert.equal(init?.method, 'POST');
    assert.deepEqual(JSON.parse(String(init?.body)), {
      clip_id: 42,
      reconciliations: [
        {
          reconciliation_id: '701',
          platform: 'youtube',
          provider_started_at: '2026-09-01T10:00:00Z',
          provider_external_id: 'yt-42',
        },
        {
          reconciliation_id: '702',
          platform: 'instagram',
          provider_started_at: '2026-09-01T10:01:00Z',
          provider_external_id: null,
        },
      ],
      streamer: 'midcore_live',
    });
    return new Response(
      JSON.stringify({
        ok: true,
        message: 'Clip manuell als veröffentlicht bestätigt',
        reconciled_platforms: ['youtube', 'instagram'],
      }),
      { status: 200, headers: { 'Content-Type': 'application/json' } },
    );
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  const result = await markClipPublished({
    clipDbId: 42,
    reconciliations: [
      {
        reconciliation_id: '701',
        platform: 'youtube',
        provider_started_at: '2026-09-01T10:00:00Z',
        provider_external_id: 'yt-42',
      },
      {
        reconciliation_id: '702',
        platform: 'instagram',
        provider_started_at: '2026-09-01T10:01:00Z',
        provider_external_id: null,
      },
    ],
    streamer: 'midcore_live',
  });
  assert.deepEqual(result.reconciled_platforms, ['youtube', 'instagram']);
});

test('Verwerfen akzeptiert den kontrollierten 409-Erfolg und liefert die Warnfelder', async (t) => {
  const originalWindow = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TWITCH_DASHBOARD_RUNTIME__: {},
      location: { pathname: '/', origin: 'http://localhost' },
    },
  });
  t.after(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
  });

  const { discardClip } = await import('../../api/socialMedia');
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    assert.equal(String(input), '/social-media/api/admin/clips/42/discard');
    assert.equal((init?.method ?? 'GET').toUpperCase(), 'POST');
    return new Response(
      JSON.stringify({
        clip_db_id: 42,
        discarded: true,
        pending_stopped: 2,
        already_running: 1,
      }),
      {
        status: 409,
        headers: { 'Content-Type': 'application/json' },
      },
    );
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  assert.deepEqual(await discardClip(42), {
    clip_db_id: 42,
    discarded: true,
    pending_stopped: 2,
    already_running: 1,
  });
});

test('Verwerfen behandelt einen fremden 409-Payload weiterhin als Fehler', async (t) => {
  const originalWindow = globalThis.window;
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      __TWITCH_DASHBOARD_RUNTIME__: {},
      location: { pathname: '/', origin: 'http://localhost' },
    },
  });
  t.after(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
  });

  const { discardClip, SocialMediaApiError } = await import('../../api/socialMedia');
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ error: 'other_conflict' }), {
      status: 409,
      headers: { 'Content-Type': 'application/json' },
    });
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  await assert.rejects(
    discardClip(42),
    (error: unknown) =>
      error instanceof SocialMediaApiError && error.code === 'discard_conflict_invalid',
  );
});

test('lokale Preview startet sicher ohne Release und simuliert das Render-Polling', () => {
  const plan = getSocialMediaPreviewFixture(
    '/social-media/api/admin/settings/posting-plan?streamer_login=midcore_live',
  ) as { release_enabled?: boolean };
  assert.equal(plan.release_enabled, false);

  const gestartet = getSocialMediaPreviewFixture(
    '/social-media/api/admin/clips/999/preparation',
    'POST',
  ) as ClipPreparation;
  assert.equal(gestartet.state, 'rendering');
  assert.equal(gestartet.preview_ready, false);

  const poll = getSocialMediaPreviewFixture(
    '/social-media/api/admin/clips/999/preparation',
    'GET',
  ) as ClipPreparation;
  assert.equal(istVorbereitungAktiv(poll.state), true);

  const konten = getSocialMediaPreviewFixture(
    '/social-media/api/platforms/status?streamer=midcore_live',
  ) as { platforms: Array<{ connected: boolean }> };
  assert.equal(konten.platforms.every((platform) => platform.connected === false), true);
  const clips = getSocialMediaPreviewFixture(
    '/social-media/api/admin/clips?status=pending&streamer=midcore_live',
  ) as { items: Array<{ clip_url: string | null; thumbnail_url: string | null }> };
  assert.equal(clips.items.every((clip) => clip.clip_url === null), true);
  assert.equal(
    clips.items.every((clip) => clip.thumbnail_url?.startsWith('data:image/svg+xml,') === true),
    true,
  );
  assert.equal(
    getSocialMediaPreviewFixture(
      '/social-media/api/admin/approval/301/decision',
      'POST',
    ),
    undefined,
    'die lokale Preview darf keine Veröffentlichung oder Freigabe vortäuschen',
  );
  assert.deepEqual(
    getSocialMediaPreviewFixture('/social-media/api/mark-uploaded', 'POST'),
    {
      ok: false,
      error: 'preview_read_only',
      message: 'preview_read_only',
    },
    'der manuelle Plattformabgleich bleibt in der lokalen Vorschau schreibgeschützt',
  );

  const fertigeVorschau = getSocialMediaPreviewFixture(
    '/social-media/api/admin/clips/301/preparation',
  ) as ClipPreparation;
  assert.match(fertigeVorschau.preview_url ?? '', /^data:video\/mp4;base64,/);
});

test('prüfbare Vorbereitung rendert echte Videosteuerung, Download und Live-Status', async () => {
  const { ClipPreparationWorkbench } = await import('./ClipPreparationWorkbench');
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(
    ['social-media', 'clip-preparation', 42],
    {
      ...preparation('preview_ready'),
      preview_url: 'data:video/mp4;base64,AAAA',
      download_url: 'data:video/mp4;base64,AAAA',
    } satisfies ClipPreparation,
  );

  const markup = renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client: queryClient },
      React.createElement(
        LanguageProvider,
        null,
        React.createElement(ClipPreparationWorkbench, {
          clipDbId: 42,
          clipTitle: 'Prüfclip',
        }),
      ),
    ),
  );

  assert.match(markup, /<video[^>]+controls=""/);
  assert.match(markup, /aria-live="polite"/);
  assert.match(markup, /MP4 herunterladen/);
  assert.match(markup, /data:video\/mp4;base64,AAAA/);
});

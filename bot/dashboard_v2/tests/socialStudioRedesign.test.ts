import { studioActions } from '../src/components/socialmedia/studioActions';
import { clipDisplayTitles } from '../src/components/socialmedia/clipTitles';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  queueStage,
  queueStats,
  filterQueue,
  loadQueueSnapshot,
} from '../src/components/socialmedia/queuePresentation';
import type { SocialClipMitPosting } from '../src/api/socialMedia';
import { upcomingTerms } from '../src/components/socialmedia/upcomingTerms';
import { DEFAULT_LAYOUT } from '../src/types/socialMedia';
import { approvalFact, authorizationFact, clipPlatformPresentation, connectionFact, publicationFact, transferFact } from '../src/components/socialmedia/platformPresentation';
import { socialAiClipFromSearch, socialViewFromSearch, socialViewSearch, socialWorkspaceView } from '../src/components/socialmedia/studioNavigation';
import { getPreviewSocialResponse, SOCIAL_CLIPS_FIXTURE, SOCIAL_PLATFORMS_FIXTURE } from '../src/preview/fixtures';

const clip = (id: number, overrides: Partial<SocialClipMitPosting> = {}): SocialClipMitPosting => ({
  clip_db_id: id,
  clip_id: String(id),
  title: `Clip ${id}`,
  streamer_login: 'earlysalty',
  clip_url: null,
  thumbnail_url: null,
  created_at: '2026-09-21T12:00:00Z',
  duration_seconds: 30,
  view_count: 100,
  game_name: 'Deadlock',
  status: 'pending',
  source_kind: 'twitch',
  upload_local_path: null,
  retention_until: null,
  discarded_at: null,
  platform_status: { youtube: false, tiktok: false, instagram: false },
  layout_override: null,
  effective_layout: DEFAULT_LAYOUT,
  ...overrides,
});

test('veröffentlichte und archivierte Clips bleiben trotz altem Freigabestand terminal', () => {
  const approval = { state: 'awaiting_approval' } as SocialClipMitPosting['approval'];
  assert.equal(queueStage(clip(1, { status: 'published_all', approval })), 'posted');
  assert.equal(queueStage(clip(2, { status: 'discarded', approval })), 'archive');
  assert.equal(queueStage(clip(3, { status: 'awaiting_approval' })), 'review');
  assert.equal(queueStage(clip(4, { status: 'approved' })), 'new');
});

test('geplante Posts zählen Plattformtermine, nicht Clips oder vergangene Veröffentlichungen', () => {
  const scheduled = { youtube: '2026-09-24T18:00:00Z', tiktok: '2026-09-24T19:00:00Z' };
  const data = [
    clip(1, { status: 'approved', scheduled_at: scheduled }),
    clip(2, { status: 'published_all', scheduled_at: scheduled }),
    clip(3, { status: 'published_partial' }),
  ];
  assert.equal(queueStats(data).scheduled, 2);
  assert.equal(queueStats(data).failed, 1);
  assert.equal(queueStage(data[0]), 'planned');
});

test('Suche, Plattform und Status kombinieren sich über den gesamten Bestand', () => {
  const data = [
    clip(1, {
      title: 'Bebop ÜBERLEBT',
      status: 'awaiting_approval',
      approval: { approved_platforms: ['youtube'] } as SocialClipMitPosting['approval'],
    }),
    clip(2),
  ];
  assert.deepEqual(
    filterQueue(data, 'review', 'überlebt', 'youtube').map((c) => c.clip_db_id),
    [1],
  );
  assert.equal(filterQueue(data, 'review', 'überlebt', 'tiktok').length, 0);
});

test('Bestandsabfrage lädt nachfolgende Seiten und zählt nicht bloß die ersten 100', async () => {
  const calls: number[] = [];
  const data = await loadQueueSnapshot(async (page) => {
    calls.push(page);
    return { items: page === 1 ? [clip(1), clip(2)] : [clip(3)], total: 3, page, page_size: 2 };
  });
  assert.deepEqual(calls, [1, 2]);
  assert.equal(data.items.length, 3);
});

test('unvollständiger oder fehlgeschlagener Bestand wird nicht als vollständige Null angezeigt', async () => {
  await assert.rejects(
    loadQueueSnapshot(async (page) => ({
      items: page === 1 ? [clip(1)] : [],
      total: 3,
      page,
      page_size: 1,
    })),
  );
  await assert.rejects(
    loadQueueSnapshot(async () => {
      throw new Error('offline');
    }),
  );
});

test('Abbruch stoppt weitere Seitenabfragen', async () => {
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(
    loadQueueSnapshot(
      async (page) => ({ items: [], total: 0, page, page_size: 100 }),
      controller.signal,
    ),
  );
});

test('Pagination-Verschiebung: neuer Clip zwischen zwei Abrufen bricht die Ansicht nicht ab', async () => {
  // 101 Clips, Seitengroesse 100. Zwischen Seite 1 und 2 kommt Clip 0 dazu:
  // alles rueckt um eine Position, Seite 2 liefert c100 erneut plus c101.
  const bestand = Array.from({ length: 101 }, (_, i) => clip(i + 1));
  const calls: number[] = [];
  const data = await loadQueueSnapshot(async (page) => {
    calls.push(page);
    if (page === 1) return { items: bestand.slice(0, 100), total: 101, page, page_size: 100 };
    return { items: [clip(100), clip(101)], total: 102, page, page_size: 100 };
  });
  assert.deepEqual(calls, [1, 2]);
  assert.equal(data.items.length, 101);
  assert.equal(data.total, 102);
});

test('Kontostand, Upload-Rechte und Videofreigabe bleiben unabhängige Fakten', () => {
  const [youtube, tiktok] = SOCIAL_PLATFORMS_FIXTURE;
  const partial = SOCIAL_CLIPS_FIXTURE[1];
  assert.equal(connectionFact(youtube).state, 'connected');
  assert.equal(authorizationFact(youtube).state, 'sufficient');
  assert.equal(approvalFact(partial, 'youtube').state, 'approved');
  assert.equal(approvalFact(partial, 'tiktok').state, 'not_approved');
  assert.equal(authorizationFact(tiktok).state, 'missing');
  assert.equal(connectionFact(tiktok).state, 'channel_required');
  assert.equal(publicationFact(partial, 'youtube').state, 'unknown');
  const undecided = { ...partial, approval: { ...partial.approval!, state: 'editing' as const } };
  assert.equal(approvalFact(undecided, 'youtube').state, 'not_approved');
});

test('fehlende oder gescheiterte Statusdaten behaupten keine fehlende Verbindung', () => {
  const stored = SOCIAL_PLATFORMS_FIXTURE[0];
  assert.equal(connectionFact(undefined).state, 'unknown');
  assert.equal(connectionFact(stored, false, true).state, 'error');
  assert.equal(connectionFact(stored, true).state, 'loading');
  assert.equal(authorizationFact({ ...stored, authorization: undefined }).state, 'unknown');
  assert.equal(approvalFact(clip(1), 'youtube').state, 'unknown');
  const row = clipPlatformPresentation(SOCIAL_CLIPS_FIXTURE[1], 'youtube', stored, 'error');
  assert.equal(row.connection.state, 'error');
  assert.equal(row.authorization.state, 'error');
  assert.equal(row.approval.state, 'approved');
});

test('Übertragung, Postfach und private YouTube-Sichtbarkeit bedeuten keine öffentliche Publikation', () => {
  assert.equal(transferFact(SOCIAL_CLIPS_FIXTURE[1], 'youtube').state, 'queued');
  assert.equal(transferFact(SOCIAL_CLIPS_FIXTURE[3], 'tiktok').state, 'inbox');
  assert.equal(publicationFact(SOCIAL_CLIPS_FIXTURE[3], 'tiktok').state, 'inbox');
  assert.equal(transferFact(SOCIAL_CLIPS_FIXTURE[4], 'youtube').state, 'transferred');
  assert.equal(publicationFact(SOCIAL_CLIPS_FIXTURE[4], 'youtube').state, 'private');
  assert.equal(transferFact(SOCIAL_CLIPS_FIXTURE[5], 'youtube').state, 'failed');
  assert.equal(transferFact(SOCIAL_CLIPS_FIXTURE[5], 'instagram').state, 'waiting_connection');
  assert.equal(publicationFact({ ...SOCIAL_CLIPS_FIXTURE[3], publish_states: { tiktok: 'PUBLISH_COMPLETE' } }, 'tiktok').state, 'confirmed');
});

test('alte Studio-Adressen und OAuth-Rückkehr bleiben erreichbar', () => {
  assert.equal(socialViewFromSearch('?view=pool'), 'freigabe');
  for (const view of ['clips', 'unrecognized']) assert.equal(socialViewFromSearch(`?view=${view}`), 'heute');
  for (const [alias, view] of [['schedule', 'planen'], ['settings', 'einstellungen'], ['vod', 'veroeffentlicht'], ['layout', 'einstellungen'], ['ai', 'freigabe']]) {
    assert.equal(socialViewFromSearch(`?view=${alias}`), view);
  }
  assert.equal(socialWorkspaceView('layout'), 'konten');
  for (const callback of ['oauth_success=youtube', 'oauth_error=provider_error']) assert.equal(socialViewFromSearch(`?view=pool&${callback}`), 'einstellungen');
  const search = socialViewSearch('?twitch_user_id=1&streamer=alpha&oauth_success=youtube', 'plan');
  assert.equal(socialViewFromSearch(`?${search}`), 'planen');
  assert.equal(new URLSearchParams(search).get('twitch_user_id'), '1');
  assert.equal(new URLSearchParams(search).get('streamer'), 'alpha');
});

test('upcoming terms exclude past and invalid dates while preserving legacy timezones', () => {
  const data = {
    schedule: undefined,
    scheduleFailed: false,
    publish: undefined,
    publishFailed: false,
    clips: [
      clip(1, { status: 'approved', scheduled_at: { youtube: '2026-10-13T10:00:00Z' } }),
      clip(2, { status: 'approved', scheduled_at: { youtube: '2026-10-15T10:00:00Z' } }),
      clip(3, { status: 'approved', scheduled_at: { youtube: 'invalid' } }),
    ],
    clipsFailed: false,
    platforms: [],
    platformsFailed: false,
    timezone: 'Europe/Berlin',
  };
  assert.deepEqual(upcomingTerms(data, Date.parse('2026-10-14T12:00:00Z')), [
    { key: '2-youtube', at: '2026-10-15T10:00:00Z', timezone: 'Europe/Berlin' },
  ]);
  assert.deepEqual(upcomingTerms({ ...data, clipsFailed: true }, Date.parse('2026-10-14T12:00:00Z')), []);
});

test('five canonical views resolve every historical workspace without dropping OAuth', () => {
  const groups = {
    freigabe: ['pool', 'ai', 'review'],
    planen: ['plan', 'schedule', 'autopilot', 'library', 'import'],
    einstellungen: ['konten', 'settings', 'layout'],
    veroeffentlicht: ['archiv', 'vod'],
    heute: [],
  } as const;
  for (const [view, aliases] of Object.entries(groups)) {
    assert.equal(socialViewFromSearch(`?view=${view}`), view);
    for (const alias of aliases) assert.equal(socialViewFromSearch(`?view=${alias}`), view);
  }
  assert.equal(socialViewFromSearch(''), 'heute');
});

test('KI-Einstieg bewahrt Clipkontext beim Neuladen und entfernt ihn außerhalb der KI-Ansicht', () => {
  const search = socialViewSearch('?twitch_user_id=1', 'ai', 42);
  assert.equal(socialAiClipFromSearch(`?${search}`), 42);
  assert.equal(socialAiClipFromSearch(`?${socialViewSearch(search, 'pool')}`), undefined);
  for (const value of ['0', '-1', '1.5', 'NaN', '9007199254740992']) assert.equal(socialAiClipFromSearch(`?clip_db_id=${value}`), undefined);
});

test('lokale Social-Vorschau sperrt schreibende Aufrufe und simuliert keine Speicherung', async () => {
  for (const path of ['/twitch/social-media/api/admin/approval/1/decision', '/twitch/social-media/api/admin/clips/1/preview', '/twitch/social-media/oauth/disconnect/youtube', '/twitch/social-media/api/ai/jobs']) {
    const response = getPreviewSocialResponse(path, { method: 'POST' });
    assert.equal(response.status, 409);
    assert.equal((await response.json()).error, 'preview_read_only');
  }
  assert.equal(getPreviewSocialResponse('/twitch/social-media/api/unimplemented').status, 404);
});

test('lokale Lesefehler und Renderzustände behalten unbekannte oder fehlerhafte Ergebnisse', async () => {
  assert.equal(getPreviewSocialResponse('/twitch/social-media/api/platforms/status', {}, 'accounts-error').status, 503);
  assert.deepEqual((await getPreviewSocialResponse('/twitch/social-media/api/platforms/status', {}, 'unknown').json()).platforms, []);
  assert.equal(getPreviewSocialResponse('/twitch/social-media/api/admin/clips', {}, 'clips-error').status, 503);
  const state = await getPreviewSocialResponse('/twitch/social-media/api/admin/clips/2/preview', {}, 'preview-rendering').json();
  assert.equal(state.clip_db_id, 2);
  assert.equal(state.status, 'rendering');
  assert.equal(state.ready, false);
});

test('volle Seite ohne Fortschritt startet die Bestandsabfrage neu statt abzubrechen', async () => {
  const calls: number[] = [];
  const data = await loadQueueSnapshot(async (page) => {
    calls.push(page);
    if (page === 1 && calls.length < 3)
      return { items: [clip(1), clip(2)], total: 4, page, page_size: 2 };
    if (page === 2) return { items: [clip(1), clip(2)], total: 4, page, page_size: 2 };
    return { items: [clip(3), clip(4)], total: 4, page, page_size: 2 };
  });
  assert.deepEqual(calls, [1, 2, 1]);
  assert.equal(data.items.length, 4);
});

test('approved clips awaiting TikTok remain planned rather than needing clip approval', () => {
  const approved = clip(1, { status: 'approved', upload_states: { tiktok: 'waiting_tiktok_approval' }, scheduled_at: { youtube: '2026-10-15T12:00:00Z' } });
  assert.equal(queueStage(approved), 'planned');
  assert.equal(filterQueue([approved], 'review', '', 'all').length, 0);
  assert.equal(queueStage(clip(2, { status: 'awaiting_approval', upload_states: { tiktok: 'waiting_tiktok_approval' } })), 'review');
});

test('generated titles win while duplicate Twitch titles gain date and time', () => {
  const clips = [clip(1, { title: 'Streamtitel', generated_title: '  Mid-Boss gerettet  ' }), clip(2, { title: 'Streamtitel', generated_title: ' ' }), clip(3, { title: 'Eigenständiger Titel' })];
  const titles = clipDisplayTitles(clips, 'de-DE', 'Europe/Berlin');
  assert.equal(titles.get(1), 'Mid-Boss gerettet');
  assert.match(titles.get(2) ?? '', /Streamtitel.*21\.09\..*14:00/);
  assert.equal(titles.get(3), 'Eigenständiger Titel');
});

test('Today prioritizes the same action as its overview and ignores failed sources', () => {
  const data = { clips: [clip(1, { status: 'awaiting_approval' })], clipsFailed: false, platforms: [], platformsFailed: false, scheduleFailed: false, publishFailed: false, timezone: 'Europe/Berlin' };
  const t = (key: string) => key;
  assert.deepEqual(studioActions(data, 0, t).map(action => [action.key, action.view, action.stage]), [['clips', 'freigabe', 'review']]);
  assert.deepEqual(studioActions({ ...data, clipsFailed: true }, 0, t), []);
});

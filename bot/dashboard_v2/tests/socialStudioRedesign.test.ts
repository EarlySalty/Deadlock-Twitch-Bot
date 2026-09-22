import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  queueStage,
  queueStats,
  filterQueue,
  loadQueueSnapshot,
} from '../src/components/socialmedia/queuePresentation';
import type { SocialClipMitPosting } from '../src/api/socialMedia';
import { DEFAULT_LAYOUT } from '../src/types/socialMedia';

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

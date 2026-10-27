import { test } from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;
import { PipelineOverview, StudioStatusStrip, type StudioOverviewData } from '../src/components/socialmedia/StudioOverview';
import { createScheduleQueueFixture } from '../src/components/socialmedia/autopilot/schedule-queue/fixtures';
import { createPublishReviewFixture } from '../src/components/socialmedia/autopilot/publish-review/fixtures';
import { SOCIAL_CLIPS_FIXTURE } from '../src/preview/fixtures';

const complete = (): StudioOverviewData => ({
  schedule: { ...createScheduleQueueFixture().snapshot(), jobs: [], review_count: 0, next_slot: null },
  scheduleFailed: false,
  publish: { ...createPublishReviewFixture().snapshot(), videos: [] },
  publishFailed: false,
  clips: [],
  clipsFailed: false,
  platforms: [],
  platformsFailed: false,
  timezone: 'Europe/Berlin',
});

function statusValues(data: StudioOverviewData) {
  const html = renderToStaticMarkup(<StudioStatusStrip data={data} onNavigate={() => {}} />);
  return [...html.matchAll(/class="studio-status-value">(.*?)<\/strong>/g)].map(value => value[1]);
}

function reviewValue(data: StudioOverviewData) {
  return statusValues(data)[3];
}

test('review total requires both successfully loaded sources', () => {
  assert.equal(reviewValue(complete()), '0');
  assert.equal(reviewValue({ ...complete(), clips: undefined }), '…');
  assert.equal(reviewValue({ ...complete(), schedule: undefined }), '…');
  assert.equal(reviewValue({ ...complete(), clips: undefined, clipsFailed: true }), '?');
  assert.equal(reviewValue({ ...complete(), schedule: undefined, scheduleFailed: true }), '?');
});

test('failed review refetch does not present a cached partial total as current', () => {
  assert.equal(reviewValue({ ...complete(), clipsFailed: true }), '?');
});

test('cached schedule and platform values are unknown after a failed refresh', () => {
  const failed = statusValues({ ...complete(), scheduleFailed: true, platformsFailed: true });
  assert.deepEqual(failed, ['?', '?', '?', '?', '?']);
});

test('scheduled primary publication remains visible after its production job leaves the queue', () => {
  const data = complete();
  const fixture = createPublishReviewFixture().snapshot();
  const video = fixture.videos.find(value => value.event && !value.event.executed_at)!;
  const event = { ...video.event!, scheduled_at: '2026-10-15T16:00:00Z', executed_at: null, completed_at: null };
  data.publish = { ...fixture, videos: [{ ...video, event }] };
  data.schedule!.next_slot = '2026-10-16T16:00:00Z';
  assert.match(statusValues(data)[1], /15\.10\./);
  const html = renderToStaticMarkup(<PipelineOverview data={data} now={Date.parse('2026-10-14T12:00:00Z')} onNavigate={() => {}} />);
  assert.match(html, /15\.10\./);
  assert.match(html, /16\.10\./);
});

test('legacy next appointment retains its own timezone and waits for all schedule sources', () => {
  const data = complete();
  data.schedule!.next_slot = '2026-10-16T16:00:00Z';
  data.schedule!.settings.timezone = 'America/New_York';
  data.clips = [{ ...SOCIAL_CLIPS_FIXTURE[0], status: 'approved', platform_status: { youtube: false, tiktok: false, instagram: false },
    scheduled_at: { youtube: '2026-10-15T16:00:00Z' }, upload_states: { youtube: 'pending' } }];
  const strip = renderToStaticMarkup(<StudioStatusStrip data={data} onNavigate={() => {}} />);
  assert.match(statusValues(data)[1], /15\.10\./);
  assert.match(strip, /18:00/);
  assert.match(strip, /title="Europe\/Berlin"/);
  assert.equal(statusValues({ ...data, clips: undefined })[1], '…');
  assert.equal(statusValues({ ...data, clipsFailed: true })[1], '?');
});

test('one approved legacy destination does not make a blocked destination upcoming', () => {
  const data = complete();
  data.clips = [{ ...SOCIAL_CLIPS_FIXTURE[0], status: 'approved', platform_status: { youtube: false, tiktok: false, instagram: false },
    scheduled_at: { youtube: '2026-10-16T16:00:00Z', tiktok: '2026-10-15T16:00:00Z' },
    upload_states: { youtube: 'pending', tiktok: 'waiting_tiktok_approval' } }];
  assert.match(statusValues(data)[1], /16\.10\./);
  const html = renderToStaticMarkup(<PipelineOverview data={data} now={Date.parse('2026-10-14T12:00:00Z')} onNavigate={() => {}} />);
  assert.match(html, /16\.10\./);
  assert.doesNotMatch(html, /15\.10\./);
});

test('empty pool is an actionable warning rather than an all-clear', () => {
  const data = complete();
  data.schedule!.production_hold = 'empty_pool';
  const strip = renderToStaticMarkup(<StudioStatusStrip data={data} onNavigate={() => {}} />);
  assert.match(strip, /data-tone="warning"/);
  const html = renderToStaticMarkup(<PipelineOverview data={data} now={Date.parse('2026-10-14T12:00:00Z')} onNavigate={() => {}} />);
  assert.match(html, /class="studio-overview-banner" data-tone="warning"/);
});

test('empty overview exposes incomplete data instead of asserting an all-clear', () => {
  const now = Date.parse('2026-10-14T12:00:00Z');
  const full = renderToStaticMarkup(<PipelineOverview data={complete()} now={now} onNavigate={() => {}} />);
  assert.doesNotMatch(full, /class="studio-overview-notice"/);
  for (const failure of ['scheduleFailed', 'publishFailed', 'clipsFailed', 'platformsFailed'] as const) {
    const partial = renderToStaticMarkup(<PipelineOverview data={{ ...complete(), [failure]: true }} now={now} onNavigate={() => {}} />);
    assert.match(partial, /class="studio-overview-notice" role="status"/);
  }
});

function renderHeute(data: StudioOverviewData) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <PipelineOverview data={data} now={Date.parse('2026-10-14T12:00:00Z')} onNavigate={() => {}} testRunApi={createScheduleQueueFixture().api} />
    </QueryClientProvider>,
  );
}

test('failed test run shows the error and retry on Heute instead of a fake wait', () => {
  const data = complete();
  data.schedule!.test_run = {
    job_id: 1, clip_id: 45, clip_title: 'He got Deletet', status: 'failed', stage: 'failed',
    mode: 'ai', last_error: 'Der Clip konnte nicht von Twitch geladen werden.', video_id: null, video_status: null,
  };
  const html = renderHeute(data);
  assert.match(html, /Der Clip konnte nicht von Twitch geladen werden/);
  assert.match(html, /Erneut versuchen/);
  assert.match(html, /Test verwerfen/);
  assert.doesNotMatch(html, /wartet auf deine Entscheidung/);
  assert.doesNotMatch(html, /Gerade läuft nichts/);
});

test('review without a produced video is a failure, not a decision wait', () => {
  const data = complete();
  data.schedule!.test_run = {
    job_id: 1, clip_id: 45, clip_title: 'He got Deletet', status: 'review', stage: 'review',
    mode: 'ai', last_error: 'Die erlaubten Versuche sind aufgebraucht.', video_id: null, video_status: null,
  };
  const html = renderHeute(data);
  assert.match(html, /Die erlaubten Versuche sind aufgebraucht/);
  assert.match(html, /Erneut versuchen/);
  assert.doesNotMatch(html, /wartet auf deine Entscheidung/);
});

test('a produced test video still waits for a decision', () => {
  const data = complete();
  data.schedule!.test_run = {
    job_id: 1, clip_id: 45, clip_title: 'Fertiger Test', status: 'review', stage: 'review',
    mode: 'ai', last_error: null, video_id: 9, video_status: 'review',
  };
  const html = renderHeute(data);
  assert.match(html, /wartet auf deine Entscheidung/);
  assert.doesNotMatch(html, /Erneut versuchen/);
});

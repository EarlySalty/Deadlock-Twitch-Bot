import test from 'node:test';
import assert from 'node:assert/strict';
import {
  socialViewFromSearch,
  socialViewSearch,
  socialWorkspaceDepartureAllowed,
} from '../src/components/socialmedia/studioNavigation';
import { previewWorkspaceState } from '../src/preview/socialWorkspaceFixtures';
import type { AiJobView } from '../src/components/socialmedia/ai/api';
import { getPreviewSocialResponse } from '../src/preview/fixtures';

test('grouped workspaces keep old aliases and numeric channel context', () => {
  for (const [alias, view] of [
    ['autopilot', 'planen'],
    ['schedule', 'planen'],
    ['library', 'clips'],
    ['import', 'clips'],
    ['review', 'clips'],
    ['clips', 'clips'],
    ['settings', 'einstellungen'],
    ['vod', 'vod'],
    ['veroeffentlicht', 'clips'],
  ] as const) {
    assert.equal(socialViewFromSearch(`?view=${alias}`), view);
  }
  const search = socialViewSearch(
    '?twitch_user_id=123456789&streamer=alpha&fixture=ai-rendering&clip_db_id=42',
    'review',
  );
  const params = new URLSearchParams(search);
  assert.equal(params.get('twitch_user_id'), '123456789');
  assert.equal(params.get('clip_db_id'), null);
  assert.equal(params.get('stage'), 'review');
  assert.equal(params.get('fixture'), 'ai-rendering');
  assert.equal(
    socialViewFromSearch(`?${search}&oauth_error=provider_error`),
    'einstellungen',
  );
});

test('pending saving cannot be discarded while dirty departures require confirmation', () => {
  let confirmations = 0;
  const confirm = () => {
    confirmations += 1;
    return true;
  };
  const scope = (saving: boolean, unsaved: boolean) => ({
    querySelector: (selector: string) =>
      (selector.includes('saving') ? saving : unsaved) ? ({} as Element) : null,
  });
  assert.equal(
    socialWorkspaceDepartureAllowed(scope(true, true), confirm),
    false,
  );
  assert.equal(confirmations, 0);
  assert.equal(
    socialWorkspaceDepartureAllowed(scope(false, true), () => false),
    false,
  );
  assert.equal(
    socialWorkspaceDepartureAllowed(scope(false, true), confirm),
    true,
  );
  assert.equal(confirmations, 1);
  assert.equal(
    socialWorkspaceDepartureAllowed(scope(false, false), confirm),
    true,
  );
  assert.equal(confirmations, 1);
});

test('real editor preview reuses accepted job contracts without media or write requests', async () => {
  const path = '/twitch/social-media/api/ai/jobs/1101?twitch_user_id=123456789';
  const review = (await getPreviewSocialResponse(path).json()) as AiJobView;
  assert.equal(review.clip_db_id, 1);
  assert.equal(review.measured_provider_cost_usd, null);
  assert.equal(review.cost_basis, 'usage_priced_estimate');
  assert.equal(review.original_url, null);
  assert.ok(review.versions.every((version) => version.file_url === null));
  assert.ok(review.provider_latency_ms);
  assert.ok(review.highlights.length);
  assert.ok(review.decisions.length);
  assert.equal(review.versions[0].status, 'review');
  assert.equal(review.versions[1].parent_version_id, review.versions[0].id);
  for (const [fixture, status] of [
    ['ai-rendering', 'rendering'],
    ['ai-last-version', 'failed'],
  ] as const) {
    const job = (await getPreviewSocialResponse(
      path,
      {},
      fixture,
    ).json()) as AiJobView;
    assert.equal(job.status, status);
    assert.equal(job.output_duration_ms, null);
  }
  const unknown = (await getPreviewSocialResponse(
    path,
    {},
    'ai-unknown-cost',
  ).json()) as AiJobView;
  assert.equal(unknown.cost_basis, 'unknown');
  assert.equal(unknown.estimated_provider_cost_usd, null);
  assert.equal(unknown.measured_provider_cost_usd, null);
  assert.equal(
    getPreviewSocialResponse(path.replace('123456789', 'streamer')).status,
    403,
  );
  assert.equal(
    getPreviewSocialResponse('/twitch/social-media/api/ai/jobs').status,
    403,
  );
  assert.equal(getPreviewSocialResponse(path, { method: 'POST' }).status, 409);
});

test('preview states and library fixtures remain scoped to their workspace and owner', async () => {
  for (const workspace of ['automation', 'library', 'ai'] as const) {
    for (const state of ['loading', 'error', 'empty'] as const)
      assert.equal(
        previewWorkspaceState(`?fixture=${workspace}-${state}`, workspace),
        state,
      );
  }
  assert.equal(previewWorkspaceState('?fixture=ai-error', 'library'), 'ready');
  const empty = await getPreviewSocialResponse(
    '/twitch/social-media/api/admin/clips',
    {},
    'library-empty',
  ).json();
  assert.deepEqual(empty.items, []);
  assert.equal(empty.total, 0);
  const second = await getPreviewSocialResponse(
    '/twitch/social-media/api/admin/clips?twitch_user_id=987654321',
  ).json();
  assert.ok(
    second.items.every(
      (clip: { streamer_login: string }) =>
        clip.streamer_login === 'preview_second',
    ),
  );
});

test('delayed preview saves reject rather than fabricate persistence', async (context) => {
  context.mock.timers.enable({ apis: ['setTimeout'] });
  const response = getPreviewSocialResponse(
    '/twitch/social-media/api/settings/posting-plan',
    { method: 'PUT' },
    'legacy-save-pending',
  );
  const data = response.json();
  context.mock.timers.tick(2000);
  assert.equal(response.status, 409);
  assert.equal((await data).error, 'preview_read_only');
});

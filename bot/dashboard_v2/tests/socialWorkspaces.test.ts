import test from 'node:test';
import assert from 'node:assert/strict';
import {
  socialViewFromSearch,
  socialViewSearch,
  socialWorkspaceDepartureAllowed,
} from '../src/components/socialmedia/studioNavigation';
import {
  previewAiJob,
  previewWorkspaceState,
} from '../src/preview/socialWorkspaceFixtures';
import { getPreviewSocialResponse } from '../src/preview/fixtures';

test('grouped workspaces keep old aliases and numeric channel context', () => {
  for (const [alias, view] of [
    ['autopilot', 'plan'],
    ['schedule', 'plan'],
    ['library', 'library'],
    ['import', 'library'],
    ['review', 'ai'],
    ['clips', 'pool'],
    ['settings', 'konten'],
    ['vod', 'archiv'],
  ] as const) {
    assert.equal(socialViewFromSearch(`?view=${alias}`), view);
  }
  const search = socialViewSearch(
    '?twitch_user_id=123456789&streamer=alpha&fixture=ai-rendering',
    'ai',
    42,
  );
  const params = new URLSearchParams(search);
  assert.equal(params.get('twitch_user_id'), '123456789');
  assert.equal(params.get('clip_db_id'), '42');
  assert.equal(params.get('fixture'), 'ai-rendering');
  assert.equal(
    socialViewFromSearch(`?${search}&oauth_error=provider_error`),
    'konten',
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

test('synthetic review distinguishes unknown billing and retains earlier versions', () => {
  const review = previewAiJob(42, null);
  assert.equal(review.clip_db_id, 42);
  assert.equal(review.measured_provider_cost_usd, null);
  assert.equal(review.cost_basis, 'usage_priced_estimate');
  assert.equal(review.original_url, null);
  assert.ok(review.versions.every((version) => version.file_url === null));
  const removed = review.decisions.reduce(
    (sum, item) => sum + item.end_ms - item.start_ms,
    0,
  );
  assert.equal(
    review.original_duration_ms! - removed,
    review.output_duration_ms,
  );
  for (const fixture of ['ai-rendering', 'ai-last-version']) {
    const job = previewAiJob(42, fixture);
    assert.equal(job.versions[0].status, 'review');
    assert.equal(job.versions[1].parent_version_id, job.versions[0].id);
    assert.equal(job.versions[1].duration_ms, null);
  }
  const unknown = previewAiJob(42, 'ai-unknown-cost');
  assert.equal(unknown.cost_basis, 'unknown');
  assert.equal(unknown.estimated_provider_cost_usd, null);
  assert.equal(unknown.measured_provider_cost_usd, null);
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

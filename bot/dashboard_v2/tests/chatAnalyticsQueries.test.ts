import assert from 'node:assert/strict';
import test from 'node:test';

import { ApiHttpError, shouldRetryApiQuery } from '../src/api/httpError';
import { shouldFetchChatHypeTimeline } from '../src/pages/chatAnalyticsQueries';

test('chat hype timeline waits for an explicit session', () => {
  assert.equal(shouldFetchChatHypeTimeline('midcore_live', undefined), false);
  assert.equal(shouldFetchChatHypeTimeline('midcore_live', null), false);
  assert.equal(shouldFetchChatHypeTimeline('', 42), false);
  assert.equal(shouldFetchChatHypeTimeline('midcore_live', 42), true);
});

test('api retries skip client errors but keep transient retries', () => {
  assert.equal(shouldRetryApiQuery(0, new ApiHttpError('not found', 404)), false);
  assert.equal(shouldRetryApiQuery(0, new ApiHttpError('server', 500)), true);
  assert.equal(shouldRetryApiQuery(1, new Error('network')), true);
  assert.equal(shouldRetryApiQuery(2, new Error('network')), false);
});

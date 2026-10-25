import { test } from 'node:test';
import assert from 'node:assert/strict';

import { translate } from '../src/i18n/dictionary';
import {
  RECORDING_TEXT,
  shouldAskForConsent,
  shouldRecord,
} from '../src/components/socialmedia/recording/recordingPolicy';
import {
  CHUNK_MAX_BYTES,
  KEEPALIVE_MAX_BYTES,
  RecordingUploader,
} from '../src/components/socialmedia/recording/uploader';

const PATH = '/twitch/social-media';
const META = { viewport_width: 1280, viewport_height: 720 };

test('Recording starts only with granted consent on the social media route', () => {
  assert.equal(shouldRecord(undefined, true, PATH), false);
  assert.equal(shouldRecord({ eligible: true, decision: null }, true, PATH), false);
  assert.equal(shouldRecord({ eligible: true, decision: 'declined' }, true, PATH), false);
  assert.equal(shouldRecord({ eligible: false, decision: 'granted' }, true, PATH), false);
  assert.equal(shouldRecord({ eligible: true, decision: 'granted' }, false, PATH), false);
  assert.equal(shouldRecord({ eligible: true, decision: 'granted' }, true, '/twitch/dashboard'), false);
  assert.equal(shouldRecord({ eligible: true, decision: 'granted' }, true, PATH), true);
  assert.equal(shouldAskForConsent({ eligible: true, decision: null }, true), true);
  assert.equal(shouldAskForConsent({ eligible: true, decision: 'declined' }, true), false);
});

test('A refused chunk stops the uploader and nothing else is sent', async () => {
  const bodies: string[] = [];
  let stopped = 0;
  const uploader = new RecordingUploader('s', async (body) => {
    bodies.push(body);
    return { status: 403 };
  }, META, () => { stopped += 1; });
  uploader.push({ type: 2, timestamp: 1 });
  await uploader.flush(false);
  uploader.push({ type: 3, timestamp: 2 });
  await uploader.flush(false);
  assert.equal(bodies.length, 1);
  assert.equal(stopped, 1);
  assert.equal(uploader.stopped, true);
});

test('Chunks are sequential and stay under the chunk cap', async () => {
  const bodies: string[] = [];
  const uploader = new RecordingUploader('s', async (body) => {
    bodies.push(body);
    return { status: 200 };
  }, META, () => {});
  const filler = 'x'.repeat(300 * 1024);
  for (let index = 0; index < 10; index += 1) uploader.push({ type: 3, timestamp: index, data: filler });
  await uploader.flush(false);
  assert.ok(bodies.length >= 2);
  bodies.forEach((body, index) => {
    const parsed = JSON.parse(body) as { seq: number; events: unknown[] };
    assert.equal(parsed.seq, index);
    assert.ok(new TextEncoder().encode(body).length <= CHUNK_MAX_BYTES);
  });
});

test('Keepalive is only used for bodies the browser accepts', async () => {
  const calls: boolean[] = [];
  const uploader = new RecordingUploader('s', async (_body, keepalive) => {
    calls.push(keepalive);
    return { status: 200 };
  }, META, () => {});
  uploader.push({ type: 3, timestamp: 1 });
  await uploader.flush(true);
  uploader.push({ type: 3, timestamp: 2, data: 'x'.repeat(KEEPALIVE_MAX_BYTES) });
  await uploader.flush(true);
  assert.deepEqual(calls, [true, false]);
});

test('Every recording text has an English translation', () => {
  for (const text of Object.values(RECORDING_TEXT)) {
    assert.notEqual(translate('en', text), text, `Missing English copy: ${text}`);
  }
});

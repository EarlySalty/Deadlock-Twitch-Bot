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

test('A failed upload is retried with the same chunk before the next one', async () => {
  const outcomes: Array<number | 'throw'> = [503, 'throw', 200, 200];
  const seqs: number[] = [];
  const waits: number[] = [];
  let stopped = 0;
  const uploader = new RecordingUploader('s', async (body) => {
    seqs.push((JSON.parse(body) as { seq: number }).seq);
    const outcome = outcomes.shift();
    if (outcome === 'throw') throw new Error('offline');
    return { status: outcome ?? 200 };
  }, META, () => { stopped += 1; }, async (ms) => { waits.push(ms); });
  uploader.push({ type: 2, timestamp: 1 });
  await uploader.flush(false);
  uploader.push({ type: 3, timestamp: 2 });
  await uploader.flush(false);
  assert.deepEqual(seqs, [0, 0, 0, 1]);
  assert.deepEqual(waits, [1_000, 4_000]);
  assert.equal(stopped, 0);
});

test('A chunk that keeps failing ends the recording instead of leaving a gap', async () => {
  const seqs: number[] = [];
  let stopped = 0;
  const uploader = new RecordingUploader('s', async (body) => {
    seqs.push((JSON.parse(body) as { seq: number }).seq);
    return { status: 500 };
  }, META, () => { stopped += 1; }, async () => {});
  uploader.push({ type: 2, timestamp: 1 });
  await uploader.flush(false);
  uploader.push({ type: 3, timestamp: 2 });
  await uploader.flush(false);
  assert.deepEqual(seqs, [0, 0, 0, 0]);
  assert.equal(stopped, 1);
  assert.equal(uploader.stopped, true);
});

test('An already stored chunk counts as delivered', async () => {
  let calls = 0;
  let stopped = 0;
  const uploader = new RecordingUploader('s', async () => {
    calls += 1;
    return { status: 409 };
  }, META, () => { stopped += 1; }, async () => {});
  uploader.push({ type: 2, timestamp: 1 });
  await uploader.flush(false);
  assert.equal(calls, 1);
  assert.equal(stopped, 0);
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

test('A final keepalive flush waits behind a chunk that is still retrying', async () => {
  const outcomes = [503, 200, 200];
  const seqs: number[] = [];
  let release: () => void = () => {};
  const firstWait = new Promise<void>((resolve) => { release = resolve; });
  const uploader = new RecordingUploader('s', async (body) => {
    seqs.push((JSON.parse(body) as { seq: number }).seq);
    return { status: outcomes.shift() ?? 200 };
  }, META, () => {}, () => firstWait);
  uploader.push({ type: 2, timestamp: 1 });
  const pending = uploader.flush(false);
  await Promise.resolve();
  uploader.push({ type: 3, timestamp: 2 });
  const final = uploader.flush(true);
  await new Promise((resolve) => { setTimeout(resolve, 0); });
  assert.deepEqual(seqs, [0]);
  release();
  await pending;
  await final;
  assert.deepEqual(seqs, [0, 0, 1]);
});

test('A stalled upload is aborted after the deadline and retried', async () => {
  const seqs: number[] = [];
  let aborted = 0;
  const uploader = new RecordingUploader('s', (body, _keepalive, signal) => {
    seqs.push((JSON.parse(body) as { seq: number }).seq);
    if (seqs.length > 1) return Promise.resolve({ status: 200 });
    return new Promise((_resolve, reject) => {
      signal.addEventListener('abort', () => { aborted += 1; reject(new Error('aborted')); });
    });
  }, META, () => {}, async () => {}, 5);
  uploader.push({ type: 2, timestamp: 1 });
  await uploader.flush(false);
  assert.equal(aborted, 1);
  assert.deepEqual(seqs, [0, 0]);
});

test('Queued bodies count against the session cap before they are sent', async () => {
  let calls = 0;
  let stopped = 0;
  const uploader = new RecordingUploader('s', () => {
    calls += 1;
    return new Promise(() => {});
  }, META, () => { stopped += 1; }, async () => {}, 60_000);
  const filler = 'x'.repeat(1024 * 1024);
  for (let index = 0; index < 25 && !uploader.stopped; index += 1) {
    uploader.push({ type: 3, timestamp: index, data: filler });
    void uploader.flush(false);
  }
  await Promise.resolve();
  assert.equal(uploader.stopped, true);
  assert.equal(stopped, 1);
  assert.ok(calls <= 1);
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

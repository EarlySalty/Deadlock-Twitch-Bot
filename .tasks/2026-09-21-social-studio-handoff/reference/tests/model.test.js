import test from 'node:test';
import assert from 'node:assert/strict';
import { validateSchedule, normalizeSchedule, queueMetrics, formatDuration, escapeHTML, DEFAULT_SCHEDULE, DEMO_CLIPS } from '../src/model.js';
const plan = () => structuredClone(DEFAULT_SCHEDULE);

test('example plan is valid; manual review is the default', () => {
  assert.deepEqual(validateSchedule(plan()), {});
  assert.equal(plan().approval_mode, 'manual');
});
test('queue counts posts per platform, not per clip', () => {
  assert.deepEqual(queueMetrics(DEMO_CLIPS), { review: 3, scheduled: 3, errors: 1 });
});
test('published and archived clips never enter scheduled metrics', () => {
  assert.equal(queueMetrics([{ status: 'published', targets: ['youtube'] }, { status: 'archived', targets: ['youtube'] }]).scheduled, 0);
});
test('invalid and blank weekly limits are rejected', () => {
  for (const value of ['', ' ', null, '1.5', '-1', '71', 'Infinity', 'NaN']) {
    const p = plan(); p.platforms[0].posts_per_week = value;
    assert.ok(validateSchedule(p)['youtube.week'], String(value));
  }
});
test('invalid and blank daily limits are rejected', () => {
  for (const value of ['', ' ', null, '1.5', '-1', '11']) {
    const p = plan(); p.platforms[0].max_posts_per_day = value;
    assert.ok(validateSchedule(p)['youtube.day'], String(value));
  }
});
test('a weekly target must fit within daily capacity', () => {
  const p = plan(); p.platforms[0].posts_per_week = 8;
  assert.match(validateSchedule(p)['youtube.week'], /Tageslimit/);
});
test('time zones are validated, not silently converted', () => {
  const p = plan(); p.timezone = 'Unknown/Fake';
  assert.ok(validateSchedule(p).timezone);
});
test('invalid, duplicate, empty or excessive times are rejected', () => {
  for (const times of [[], ['24:00'], ['2:30'], ['09:99'], ['18:00', '18:00'], Array.from({length:13},(_,i)=>String(i).padStart(2,'0')+':00')]) {
    const p = plan(); p.platforms[0].post_times = times;
    assert.ok(validateSchedule(p)['youtube.times'], JSON.stringify(times));
  }
});
test('paused platform settings are retained', () => {
  const p = plan(); p.platforms[2].post_times = ['21:00', '18:30'];
  const normalized = normalizeSchedule(p);
  assert.equal(normalized.platforms[2].auto_post, false);
  assert.deepEqual(normalized.platforms[2].post_times, ['18:30', '21:00']);
  assert.deepEqual(p.platforms[2].post_times, ['21:00', '18:30']);
});
test('normalization converts numeric inputs without mutating the form', () => {
  const p = plan(); p.platforms[0].posts_per_week = '4';
  assert.equal(normalizeSchedule(p).platforms[0].posts_per_week, 4);
  assert.equal(p.platforms[0].posts_per_week, '4');
});
test('unknown clip duration is not shown as zero seconds', () => {
  assert.equal(formatDuration(null), 'Dauer offen');
  assert.equal(formatDuration(NaN), 'Dauer offen');
  assert.equal(formatDuration(91), '1:31');
});
test('untrusted clip titles are HTML-escaped', () => {
  assert.equal(escapeHTML('<img src=x onerror="alert(1)">'), '&lt;img src=x onerror=&quot;alert(1)&quot;&gt;');
});

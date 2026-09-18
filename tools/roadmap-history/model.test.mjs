import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('./model.js', import.meta.url), 'utf8');
const model = vm.runInNewContext(source + ';({normalize,dateLabel,monthLabel,dateBounds,rangeFor,filterCommits,monthsBetween,featureRows,milestones,representative})');
const plain = value => JSON.parse(JSON.stringify(value));
const event = (id, date, kind, features = ['uplink'], title = id) => ({id, date, kind, features, title, subject: title, timestamp: date + 'T12:00:00Z'});
const data = {
  groups: [{id: 'streaming', title: 'Streaming'}, {id: 'platform', title: 'Plattform'}],
  features: [{id: 'uplink', group: 'streaming', title: 'Uplink & Encoding', description: 'AV1'}, {id: 'caster', group: 'streaming', title: 'Caster-Studio', description: 'Kameras'}, {id: 'auth', group: 'platform', title: 'Login', description: 'Berechtigungen'}],
  commits: [event('first', '2026-01-01', 'feat'), event('boundary', '2026-08-19', 'fix'), event('old', '2026-08-18', 'fix'), event('shared', '2026-09-10', 'feat', ['uplink', 'caster']), event('last', '2026-09-17', 'fix', ['caster']), event('docs', '2026-09-17', 'docs'), event('auth', '2026-09-17', 'fix', ['auth']), event('deps', '2026-09-17', 'deps')]
};
const defaults = {search: '', group: 'all', period: 'all', kind: 'all', maintenance: false};

test('default noise filter does not hide actual fixes', () => {
  const result = model.filterCommits(data, defaults);
  assert.equal(result.length, 6);
  assert.ok(result.some(c => c.id === 'auth'));
  assert.ok(!result.some(c => c.id === 'docs'));
});
test('maintenance can be included without losing history', () => {
  assert.equal(model.filterCommits(data, {...defaults, maintenance: true}).length, 8);
});
test('maintenance kind works even when checkbox is off', () => {
  assert.equal(model.filterCommits(data, {...defaults, kind: 'maintenance'}).length, 2);
});
test('fix filter cannot show extension commits', () => {
  assert.ok(model.filterCommits(data, {...defaults, kind: 'fix'}).every(c => c.kind === 'fix'));
});
test('30 days are inclusive and anchored to data date', () => {
  const result = model.filterCommits(data, {...defaults, period: '30'});
  assert.ok(result.some(c => c.id === 'boundary'));
  assert.ok(!result.some(c => c.id === 'old'));
  assert.equal(model.rangeFor('30', {first: '2026-01-01', last: '2026-09-17'}).from, '2026-08-19');
});
test('custom ranges are inclusive', () => {
  assert.equal(model.filterCommits(data, {...defaults, period: 'custom', from: '2026-09-10', to: '2026-09-10'}).length, 1);
});
test('reversed dates yield an empty result', () => {
  assert.equal(model.filterCommits(data, {...defaults, period: 'custom', from: '2026-09-17', to: '2026-08-01'}).length, 0);
});
test('feature search finds its whole development line', () => {
  const result = model.filterCommits(data, {...defaults, search: 'encoding'});
  assert.equal(result.length, 4);
  assert.ok(result.every(c => c.features.includes('uplink')));
});
test('search also accepts a commit hash', () => {
  assert.equal(model.filterCommits(data, {...defaults, search: 'boundary'}).length, 1);
});
test('group filter only shows relevant features', () => {
  const result = model.filterCommits(data, {...defaults, group: 'platform'});
  assert.equal(result.length, 1);
  assert.equal(result[0].id, 'auth');
});
test('shared commits count once globally, once per feature', () => {
  const result = model.filterCommits(data, {...defaults, search: 'shared'});
  assert.equal(result.length, 1);
  assert.equal(model.featureRows(data, result).length, 2);
});
test('calendar handles year transitions', () => {
  assert.deepEqual(plain(model.monthsBetween('2025-12-15', '2026-02-01')), ['2025-12', '2026-01', '2026-02']);
  assert.equal(model.dateLabel('2026-09-17'), '17.09.2026');
});
test('single commit milestone is not duplicated', () => {
  assert.equal(model.milestones([data.commits[0]]).length, 1);
});
test('milestones retain first and latest with extension in between', () => {
  const values = [event('a', '2026-01-01', 'change'), event('b', '2026-02-01', 'feat'), event('c', '2026-03-01', 'fix'), event('d', '2026-04-01', 'fix')];
  assert.deepEqual(plain(model.milestones(values).map(c => c.id)), ['a', 'b', 'd']);
  assert.equal(model.representative(values).id, 'b');
});
test('empty history remains a valid empty model', () => {
  assert.deepEqual(plain(model.dateBounds([])), {first: '', last: ''});
  assert.equal(model.monthsBetween('', '').length, 0);
  assert.equal(model.milestones([]).length, 0);
});
test('German accents and uppercase normalize for search', () => {
  assert.equal(model.normalize('ÄNDERUNG'), model.normalize('anderung'));
});

import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('./model.js', import.meta.url), 'utf8');
const m = vm.runInNewContext(source + ';({familyIndex,selectFamily,layoutFamily,filterCommits})');
const event = (id, date, features, title = id, kind = 'fix') => ({id, date, features, title, subject: title, kind, timestamp: date + 'T12:00:00Z', pathCount: 1});
const data = {groups: [{id: 'g', title: 'Bereich'}], features: [
  {id: 'main', parentId: 'product', title: 'Hauptfunktion', group: 'g'},
  {id: 'child', parentId: 'main', title: 'Kindfunktion', group: 'g'},
  {id: 'leaf', parentId: 'child', title: 'Blattfunktion', group: 'g'},
  {id: 'unknown', parentId: 'product', title: 'Unbelegt', group: 'g'}
], commits: [event('old', '2026-01-01', ['main']), event('shared', '2026-09-17', ['child', 'leaf'], 'Zielname korrigieren')]};
const defaults = {group: 'all', period: 'all', kind: 'all', maintenance: false, search: ''};
test('index supports multiple generations and unique subtree histories', () => {
  const index = m.familyIndex(data);
  assert.equal(index.get('leaf').depth, 3);
  assert.equal(index.get('main').allCommits.length, 2);
  assert.equal(index.get('child').allCommits.length, 1);
  assert.equal(index.get('unknown').first, '');
});
test('orphan, cycle and duplicate node IDs fail closed', () => {
  for (const features of [
    [{id: 'a', parentId: 'missing'}],
    [{id: 'a', parentId: 'b'}, {id: 'b', parentId: 'a'}],
    [{id: 'a'}, {id: 'a'}]
  ]) assert.throws(() => m.familyIndex({...data, features}));
});
test('date/search matches retain context ancestors but not unknown siblings', () => {
  const matches = m.filterCommits(data, {...defaults, search: 'Zielname', period: 'custom', from: '2026-09-17', to: '2026-09-17'});
  const selection = m.selectFamily(data, matches, {});
  assert.equal(selection.nodes.find(n => n.id === 'main').context, true);
  assert.ok(selection.nodes.some(n => n.id === 'leaf' && !n.context));
  assert.ok(selection.nodes.some(n => n.id === 'product' && n.context));
  assert.ok(!selection.nodes.some(n => n.id === 'unknown'));
  assert.equal(selection.commitCount, 1);
});
test('parent feature search includes descendant history', () => {
  assert.equal(m.filterCommits(data, {...defaults, search: 'Hauptfunktion'}).length, 2);
});
test('collapse is explicit and focus keeps ancestry', () => {
  const collapsed = m.selectFamily(data, data.commits, {collapsed: ['child']});
  assert.ok(!collapsed.nodes.some(n => n.id === 'leaf'));
  const focused = m.selectFamily(data, data.commits, {focus: 'child'});
  assert.deepEqual(Array.from(focused.nodes, n => n.id), ['product', 'main', 'child', 'leaf']);
  assert.equal(focused.commitCount, 1);
});
test('dense histories bundle by time gap without losing a single change', () => {
  const dense = {...data, commits: Array.from({length: 1200}, (_, i) => event(String(i), '2026-09-17', ['leaf'], '<img onerror=alert(1)>'.repeat(100)))};
  const graph = m.layoutFamily(m.selectFamily(dense, dense.commits, {}));
  const total = graph.milestones.reduce((sum, mile) => sum + mile.commitIds.length, 0);
  assert.equal(total, 1200);
  assert.ok(graph.milestones.every(mile => new Set(mile.commitIds).size === mile.commitIds.length));
  assert.ok(graph.branches.every(b => b.id !== 'other'));
  assert.ok(graph.nodes.length < 30);
  const labels = graph.nodes.filter(n => n.type === 'label');
  for (let i = 0; i < labels.length; i++) for (let j = i + 1; j < labels.length; j++) {
    const a = labels[i], b = labels[j];
    assert.ok(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y, 'branch labels must not overlap');
  }
});
test('a branch forks from its parent lane at its first dated change', () => {
  const graph = m.layoutFamily(m.selectFamily(data, data.commits, {}));
  const leaf = graph.branches.find(b => b.id === 'leaf');
  const fork = graph.forks.find(f => f.key === 'f:leaf');
  assert.equal(fork.source, 'f:child');
  assert.equal(fork.y2, leaf.y);
  assert.equal(fork.y1, graph.branches.find(b => b.id === 'child').y);
  assert.equal(Math.round(fork.x), Math.round(leaf.forkX));
});
test('import-era features fork together at the trunk start as preexisting', () => {
  const graph = m.layoutFamily(m.selectFamily(data, data.commits, {}));
  const main = graph.branches.find(b => b.id === 'main');
  assert.equal(main.preexisting, true);
  assert.equal(Math.round(main.forkX), graph.axis.x0);
  const label = graph.nodes.find(n => n.type === 'label' && n.featureId === 'main');
  assert.equal(label.preexisting, true);
});
test('empty selection stays empty; no invented evidence dates', () => {
  const empty = m.layoutFamily(m.selectFamily(data, [], {}));
  assert.equal(empty.nodes.length, 0);
  assert.equal(empty.branches.length, 0);
  assert.equal(empty.milestones.length, 0);
});

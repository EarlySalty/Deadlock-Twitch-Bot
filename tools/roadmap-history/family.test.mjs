import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('./model.js', import.meta.url), 'utf8');
const m = vm.runInNewContext(source + ';({familyIndex,selectFamily,layoutFamily,filterCommits})');
const event = (id, date, features, title = id) => ({id, date, features, title, subject: title, kind: 'fix', timestamp: date});
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
test('dense histories are complete monthly bundles, not three arbitrary milestones', () => {
  const dense = {...data, commits: Array.from({length: 1200}, (_, i) => event(String(i), '2026-09-17', ['leaf'], '<img onerror=alert(1)>'.repeat(100)))};
  const graph = m.layoutFamily(m.selectFamily(dense, dense.commits, {}));
  const bundles = graph.nodes.filter(n => n.type === 'events');
  assert.equal(bundles.reduce((sum, n) => sum + n.events.length, 0), 1200);
  assert.ok(graph.nodes.length < 20);
  for (let i = 0; i < graph.nodes.length; i++) for (let j = i + 1; j < graph.nodes.length; j++) {
    const a = graph.nodes[i], b = graph.nodes[j];
    assert.ok(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y);
  }
  for (const edge of graph.edges) {
    const parent = graph.nodes.find(n => n.key === edge.source);
    assert.equal(edge.x1, parent.x + parent.width);
    assert.equal(edge.y1, parent.y + parent.height / 2);
  }
});
test('empty selection stays empty; no invented evidence dates', () => {
  assert.equal(m.layoutFamily(m.selectFamily(data, [], {})).nodes.length, 0);
});

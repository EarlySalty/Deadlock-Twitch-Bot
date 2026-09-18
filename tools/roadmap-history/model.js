'use strict';
const MAINTENANCE = new Set(['docs', 'test', 'chore', 'deps', 'ci']);
const KINDS = {feat: 'Erweiterung', fix: 'Fehlerbehebung', security: 'Sicherheit', change: 'Überarbeitung', docs: 'Dokumentation', test: 'Tests', chore: 'Pflege', deps: 'Abhängigkeiten', ci: 'Build & CI'};
function normalize(value) {
  return String(value || '').normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLocaleLowerCase('de');
}
function dateLabel(value) {
  return /^\d{4}-\d{2}-\d{2}$/.test(value || '') ? value.split('-').reverse().join('.') : 'Nicht belegt';
}
function monthLabel(key) {
  return new Intl.DateTimeFormat('de-DE', {month: 'long', year: 'numeric', timeZone: 'UTC'}).format(new Date(key + '-01T12:00:00Z'));
}
function dateBounds(commits) {
  const dates = commits.map(c => c.date).sort();
  return {first: dates[0] || '', last: dates.at(-1) || ''};
}
function rangeFor(period, bounds, from = '', to = '') {
  if (period === 'custom') return {from: from || bounds.first, to: to || bounds.last};
  if (period === 'all' || !bounds.last) return {from: bounds.first, to: bounds.last};
  const start = new Date(bounds.last + 'T12:00:00Z');
  start.setUTCDate(start.getUTCDate() - (period === '30' ? 29 : 89));
  return {from: start.toISOString().slice(0, 10), to: bounds.last};
}
function compareDates(a, b) {
  return a.date.localeCompare(b.date) || (a.timestamp || '').localeCompare(b.timestamp || '') || a.id.localeCompare(b.id);
}
function filterCommits(data, filters) {
  const features = new Map(data.features.map(f => [f.id, f]));
  const groups = new Map(data.groups.map(g => [g.id, g]));
  const range = rangeFor(filters.period, dateBounds(data.commits), filters.from, filters.to);
  if (range.from > range.to) return [];
  const query = normalize(filters.search).trim();
  const names = new Map();
  for (const f of data.features) {
    const text = [], seen = new Set();
    let cursor = f;
    while (cursor && !seen.has(cursor.id)) {
      seen.add(cursor.id);
      text.push(cursor.title, cursor.description, groups.get(cursor.group)?.title);
      cursor = features.get(cursor.parentId);
    }
    names.set(f.id, text.join(' '));
  }
  return data.commits.filter(c => {
    if (c.date < range.from || c.date > range.to) return false;
    if (filters.kind === 'maintenance') {
      if (!MAINTENANCE.has(c.kind)) return false;
    } else if (filters.kind && filters.kind !== 'all') {
      if (c.kind !== filters.kind) return false;
    } else if (!filters.maintenance && MAINTENANCE.has(c.kind)) return false;
    if (filters.group !== 'all' && !c.features.some(id => features.get(id)?.group === filters.group)) return false;
    return !query || normalize([c.title, c.subject, c.id, ...c.features.map(id => names.get(id))].join(' ')).includes(query);
  });
}
function monthsBetween(from, to) {
  if (!from || !to || from > to) return [];
  const result = [];
  const cursor = new Date(from.slice(0, 7) + '-01T12:00:00Z');
  const end = new Date(to.slice(0, 7) + '-01T12:00:00Z');
  while (cursor <= end) {
    result.push(cursor.toISOString().slice(0, 7));
    cursor.setUTCMonth(cursor.getUTCMonth() + 1);
  }
  return result;
}
function featureRows(data, commits, group = 'all') {
  const indexed = new Map();
  for (const c of commits) for (const id of new Set(c.features)) {
    if (!indexed.has(id)) indexed.set(id, []);
    indexed.get(id).push(c);
  }
  return data.features.filter(f => (group === 'all' || f.group === group) && indexed.has(f.id)).map(f => ({...f, commits: indexed.get(f.id).slice().sort(compareDates)}));
}
// Retained for consumers of the old model; the graph NEVER limits itself to these samples.
function milestones(commits) {
  if (!commits.length) return [];
  const ordered = commits.slice().sort(compareDates);
  const first = ordered[0], last = ordered.at(-1);
  const middle = ordered.slice(1, -1).filter(c => c.kind === 'feat').at(-1) || ordered.slice(1, -1).at(-1);
  return [first, middle, last].filter((c, i, all) => c && all.findIndex(x => x?.id === c.id) === i);
}
function representative(commits) {
  const ordered = commits.slice().sort(compareDates);
  return ordered.filter(c => c.kind === 'feat').at(-1) || ordered.at(-1);
}
function familyIndex(data) {
  const index = new Map([['product', {id: 'product', title: 'Twitch Bot', description: 'Produktursprung im Repository. Kein Nachweis eines Live-Starts.', parentId: null, children: [], commits: [], depth: 0}]]);
  for (const feature of data.features) {
    if (index.has(feature.id)) throw new Error('Doppelte Funktions-ID: ' + feature.id);
    index.set(feature.id, {...feature, parentId: feature.parentId || 'product', children: [], commits: []});
  }
  for (const node of index.values()) {
    if (!node.parentId) continue;
    if (!index.has(node.parentId)) throw new Error('Unbekanntes Elternfeature: ' + node.parentId);
    index.get(node.parentId).children.push(node.id);
  }
  const visiting = new Set(), visited = new Set();
  function visit(id) {
    if (visiting.has(id)) throw new Error('Zyklus in der Funktionszuordnung: ' + id);
    if (visited.has(id)) return;
    visiting.add(id);
    const node = index.get(id);
    if (node.parentId) {
      visit(node.parentId);
      node.depth = index.get(node.parentId).depth + 1;
    }
    visiting.delete(id); visited.add(id);
  }
  for (const id of index.keys()) visit(id);
  const global = new Map();
  for (const commit of data.commits) {
    if (global.has(commit.id)) continue;
    global.set(commit.id, commit);
    for (const id of new Set(commit.features)) {
      const node = index.get(id);
      if (!node) throw new Error('Änderung mit unbekannter Funktionszuordnung: ' + id);
      node.commits.push(commit);
    }
  }
  for (const node of [...index.values()].sort((a, b) => b.depth - a.depth)) {
    const unique = new Map(node.commits.map(c => [c.id, c]));
    for (const child of node.children) for (const c of index.get(child).allCommits) unique.set(c.id, c);
    node.allCommits = [...unique.values()].sort(compareDates);
    node.commits.sort(compareDates);
    node.first = node.allCommits[0]?.date || '';
    node.last = node.allCommits.at(-1)?.date || '';
    node.firstDirect = node.commits[0]?.date || '';
  }
  return index;
}
function descendants(index, id) {
  const result = new Set();
  function visit(current) {
    if (!index.has(current) || result.has(current)) return;
    result.add(current);
    for (const child of index.get(current).children) visit(child);
  }
  visit(id);
  return result;
}
function selectFamily(data, commits, options = {}, cachedIndex) {
  const index = cachedIndex || familyIndex(data);
  const focus = index.has(options.focus) ? options.focus : 'product';
  const scope = descendants(index, focus);
  const selected = [...new Map(commits.filter(c => c.features.some(id => scope.has(id))).map(c => [c.id, c])).values()];
  const direct = new Map(), keep = new Set();
  for (const c of selected) for (const id of new Set(c.features)) {
    if (!scope.has(id)) continue;
    if (!direct.has(id)) direct.set(id, []);
    direct.get(id).push(c);
    let cursor = index.get(id);
    while (cursor && !keep.has(cursor.id)) {keep.add(cursor.id); cursor = index.get(cursor.parentId);}
  }
  const collapsed = new Set(options.revealMatches ? [] : options.collapsed || []);
  const nodes = [];
  function visit(id) {
    if (!keep.has(id)) return;
    const f = index.get(id);
    nodes.push({...f, context: !direct.has(id), selected: (direct.get(id) || []).slice().sort(compareDates), collapsed: collapsed.has(id), visibleChildren: f.children.filter(child => keep.has(child))});
    if (!collapsed.has(id)) for (const child of f.children) visit(child);
  }
  visit('product');
  return {nodes, index, commits: selected, commitCount: selected.length, focus};
}
/* Two explicitly labelled spaces: generations on the left; monthly event lanes
 * on the right. Feature positions are NOT dates. Event columns are months, not
 * day-scale coordinates. A feature's first evidence is shown as a real date.
 * This prevents collision avoidance from implying a false introduction date.
 */
function layoutFamily(selection) {
  if (!selection.nodes.length) return {nodes: [], edges: [], months: [], width: 800, height: 500, calendarStart: 0};
  const maxDepth = Math.max(...selection.nodes.map(n => n.depth));
  const calendarStart = (maxDepth + 1) * 248 + 36;
  const dates = dateBounds(selection.commits);
  const months = monthsBetween(dates.first, dates.last);
  const nodes = [], edges = [], byFeature = new Map();
  const connect = (a, b, kind) => edges.push({source: a.key, target: b.key, kind, x1: a.x + a.width, y1: a.y + a.height / 2, x2: b.x, y2: b.y + b.height / 2});
  selection.nodes.forEach((feature, row) => {
    const node = {...feature, key: 'f:' + feature.id, type: 'feature', x: 24 + feature.depth * 248, y: 28 + row * 170, width: 216, height: 146};
    nodes.push(node); byFeature.set(feature.id, node);
    let previous = node;
    const grouped = new Map();
    for (const c of feature.selected) {
      const month = c.date.slice(0, 7);
      if (!grouped.has(month)) grouped.set(month, []);
      grouped.get(month).push(c);
    }
    for (const [month, events] of [...grouped].sort(([a], [b]) => a.localeCompare(b))) {
      const eventNode = {key: 'e:' + feature.id + ':' + month, type: 'events', featureId: feature.id, month, events, sample: representative(events), first: events[0].date, last: events.at(-1).date, x: calendarStart + months.indexOf(month) * 264 + 12, y: node.y, width: 236, height: 146};
      nodes.push(eventNode); connect(previous, eventNode, 'history'); previous = eventNode;
    }
  });
  for (const feature of selection.nodes) {
    if (byFeature.has(feature.parentId)) connect(byFeature.get(feature.parentId), byFeature.get(feature.id), 'family');
  }
  return {nodes, edges, months, calendarStart, width: calendarStart + Math.max(1, months.length) * 264 + 32, height: selection.nodes.length * 170 + 40};
}

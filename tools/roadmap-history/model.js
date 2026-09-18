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
  // Git retains the committer's offset; chronological order must use the instant.
  // Date-only legacy fixtures fall back to UTC, never the browser's timezone.
  const instant = commit => {
    const parsed = Date.parse(commit.timestamp || '');
    return Number.isFinite(parsed) ? parsed : Date.parse(commit.date + 'T00:00:00Z');
  };
  return a.date.localeCompare(b.date) || instant(a) - instant(b) || a.id.localeCompare(b.id);
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
function dayNumber(date) {
  return Math.round(Date.parse((date || '') + 'T00:00:00Z') / 86400000);
}
function daySpan(from, to) {
  return dayNumber(to) - dayNumber(from);
}
function weeksBetween(from, to) {
  if (!from || !to || from > to) return [];
  const result = [];
  const cursor = new Date(from + 'T00:00:00Z');
  const shift = (8 - cursor.getUTCDay()) % 7;
  cursor.setUTCDate(cursor.getUTCDate() + shift);
  const end = new Date(to + 'T00:00:00Z');
  while (cursor <= end) {
    result.push(cursor.toISOString().slice(0, 10));
    cursor.setUTCDate(cursor.getUTCDate() + 7);
  }
  return result;
}
const MILESTONE_ORDER = ['security', 'feat', 'change', 'fix'];
function bundleKind(commits) {
  const present = new Set(commits.map(c => c.kind));
  return MILESTONE_ORDER.find(kind => present.has(kind)) || 'change';
}
function bundleLead(commits) {
  const feats = commits.filter(c => c.kind === 'feat').sort(compareDates);
  if (feats.length) return feats.at(-1);
  return commits.slice().sort((a, b) => (b.pathCount || 0) - (a.pathCount || 0) || compareDates(a, b))[0];
}
function bundleCommits(commits, gapDays = 3) {
  const events = commits.filter(c => !MAINTENANCE.has(c.kind)).slice().sort(compareDates);
  const bundles = [];
  for (const c of events) {
    const current = bundles.at(-1);
    if (current && daySpan(current.lastDate, c.date) <= gapDays) {
      current.commits.push(c); current.lastDate = c.date;
    } else {
      bundles.push({commits: [c], firstDate: c.date, lastDate: c.date});
    }
  }
  return bundles.map(b => {
    const lead = bundleLead(b.commits);
    return {...b, kind: bundleKind(b.commits), lead, title: lead.title, scope: b.commits.reduce((sum, c) => sum + (c.pathCount || 1), 0)};
  });
}
const LANE_TOP = 58, LANE_H = 30, AXIS_X0 = 156, RIGHT_PAD = 220, LABEL_W = 172, LANE_GAP = LABEL_W + 26, IMPORT_WINDOW = 7;
function layoutFamily(selection) {
  const blank = {nodes: [], branches: [], forks: [], milestones: [], trunk: null, months: [], weeks: [], axis: {x0: AXIS_X0, dayWidth: 0, months: [], weeks: [], first: '', last: '', width: 0}, calendarStart: AXIS_X0, width: 900, height: 420, focus: selection.focus};
  if (!selection.nodes.length) return blank;
  const bounds = dateBounds(selection.commits);
  const first = bounds.first, last = bounds.last;
  if (!first || !last) return blank;
  const totalDays = Math.max(1, daySpan(first, last));
  const dayWidth = Math.max(4.2, Math.min(9, 1560 / totalDays));
  const timeX = date => AXIS_X0 + Math.max(0, daySpan(first, date)) * dayWidth;
  const importDate = selection.index?.get('product')?.first || first;
  const drawable = selection.nodes.filter(n => n.id !== 'product' && n.id !== 'other');
  const meta = new Map();
  for (const node of drawable) {
    const direct = (node.selected || []).slice().sort(compareDates);
    meta.set(node.id, {node, direct, bundles: bundleCommits(direct), start: direct[0]?.date || '', end: direct.at(-1)?.date || ''});
  }
  for (let i = drawable.length - 1; i >= 0; i--) {
    const m = meta.get(drawable[i].id);
    for (const childId of drawable[i].visibleChildren || []) {
      const cm = meta.get(childId);
      if (!cm || !cm.start) continue;
      if (!m.start || cm.start < m.start) m.start = cm.start;
      if (!m.end || cm.end > m.end) m.end = cm.end;
    }
  }
  const preexOf = id => !meta.get(id).node.context && daySpan(importDate, meta.get(id).start) <= IMPORT_WINDOW;
  const forkXOf = id => preexOf(id) ? AXIS_X0 : timeX(meta.get(id).start);
  const laid = drawable.filter(n => meta.get(n.id).start);
  laid.sort((a, b) => forkXOf(a.id) - forkXOf(b.id) || a.depth - b.depth || a.id.localeCompare(b.id));
  const laneEnd = [], laneOf = new Map();
  for (const node of laid) {
    const m = meta.get(node.id);
    const forkX = forkXOf(node.id);
    const right = Math.max(timeX(m.end), forkX + LABEL_W) + LANE_GAP - LABEL_W;
    let lane = laneEnd.findIndex(x => x <= forkX - 8);
    if (lane === -1) {lane = laneEnd.length; laneEnd.push(0);}
    laneEnd[lane] = right;
    laneOf.set(node.id, lane);
  }
  const laneY = lane => LANE_TOP + (lane + 1) * LANE_H;
  const trunkY = LANE_TOP, trunkX1 = timeX(last);
  const branches = [], forks = [], milestones = [], nodes = [];
  for (const node of laid) {
    const m = meta.get(node.id);
    const lane = laneOf.get(node.id);
    const y = laneY(lane);
    const preexisting = preexOf(node.id);
    const forkX = forkXOf(node.id);
    const endX = Math.max(timeX(m.end), forkX + 3);
    const parentDrawn = laneOf.has(node.parentId);
    const parentY = parentDrawn ? laneY(laneOf.get(node.parentId)) : trunkY;
    const activity = m.direct.length;
    const recency = 1 - daySpan(m.end, last) / totalDays;
    const branch = {
      id: node.id, key: 'f:' + node.id, featureId: node.id, depth: node.depth,
      context: !!node.context, preexisting, y, forkX, endX, parentId: node.parentId, parentY,
      strokeWidth: node.context ? 1.2 : Math.max(1.5, Math.min(5, 1.5 + Math.log2(1 + activity) * 0.7)),
      opacity: node.context ? 0.45 : Math.max(0.5, Math.min(1, 0.58 + recency * 0.42)),
      dormant: m.end < last, first: m.start, last: m.end, directCount: activity, title: node.title,
    };
    branches.push(branch);
    forks.push({key: branch.key, source: parentDrawn ? 'f:' + node.parentId : 'trunk', x: forkX, y1: parentY, y2: y, context: branch.context});
    nodes.push({type: 'label', key: 'l:' + node.id, featureId: node.id, x: forkX + 9, y: y - 20, width: LABEL_W, height: 16, text: node.title, date: m.start, context: branch.context, preexisting, depth: node.depth, dormant: branch.dormant, hasChildren: (node.visibleChildren || []).length, collapsed: !!node.collapsed});
    for (const b of m.bundles) {
      const cx = timeX(b.firstDate);
      const r = Math.max(4.5, Math.min(13, 4 + Math.sqrt(b.scope)));
      milestones.push({type: 'milestone', key: 'm:' + node.id + ':' + b.lead.id, featureId: node.id, cx, cy: y, x: cx - r, y: y - r, width: 2 * r, height: 2 * r, r, kind: b.kind, title: b.title, date: b.firstDate, first: b.firstDate, last: b.lastDate, count: b.commits.length, scope: b.scope, commitIds: b.commits.map(c => c.id), leadId: b.lead.id});
    }
  }
  nodes.push(...milestones);
  const months = monthsBetween(first, last);
  const monthAxis = months.map(key => {
    const start = key + '-01';
    return {key, x: timeX(start < first ? first : start), label: monthLabel(key)};
  });
  for (let i = 0; i < monthAxis.length; i++) monthAxis[i].width = (i + 1 < monthAxis.length ? monthAxis[i + 1].x : trunkX1 + dayWidth) - monthAxis[i].x;
  const weeks = weeksBetween(first, last).map(date => ({x: timeX(date), date}));
  const width = trunkX1 + RIGHT_PAD;
  const height = LANE_TOP + (laneEnd.length + 1) * LANE_H + 46;
  return {
    nodes, branches, forks, milestones,
    trunk: {y: trunkY, x0: AXIS_X0, x1: trunkX1, strokeWidth: 6},
    months, weeks,
    axis: {x0: AXIS_X0, dayWidth, first, last, months: monthAxis, weeks, width},
    calendarStart: AXIS_X0, width, height, focus: selection.focus,
  };
}

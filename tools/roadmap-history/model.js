'use strict';
const MAINTENANCE = new Set(['docs', 'test', 'chore', 'deps', 'ci']);
const KINDS = {feat: 'Erweiterung', fix: 'Fehlerbehebung', security: 'Sicherheit', change: 'Überarbeitung', docs: 'Dokumentation', test: 'Tests', chore: 'Pflege', deps: 'Abhängigkeiten', ci: 'Build & CI'};
function normalize(value) {
  return String(value || '').normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLocaleLowerCase('de');
}
function dateLabel(value) {
  return /^\d{4}-\d{2}-\d{2}$/.test(value || '') ? value.split('-').reverse().join('.') : 'Kein Datum';
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
function filterCommits(data, filters) {
  const features = new Map(data.features.map(f => [f.id, f]));
  const groups = new Map(data.groups.map(g => [g.id, g]));
  const range = rangeFor(filters.period, dateBounds(data.commits), filters.from, filters.to);
  if (range.from > range.to) return [];
  const query = normalize(filters.search).trim();
  return data.commits.filter(c => {
    if (c.date < range.from || c.date > range.to) return false;
    if (filters.kind === 'maintenance') {
      if (!MAINTENANCE.has(c.kind)) return false;
    } else if (filters.kind && filters.kind !== 'all') {
      if (c.kind !== filters.kind) return false;
    } else if (!filters.maintenance && MAINTENANCE.has(c.kind)) return false;
    if (filters.group !== 'all' && !c.features.some(id => features.get(id)?.group === filters.group)) return false;
    const featureText = c.features.flatMap(id => {
      const f = features.get(id);
      return f ? [f.title, f.description, groups.get(f.group)?.title] : [];
    });
    return !query || normalize([c.title, c.subject, c.id, ...featureText].join(' ')).includes(query);
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
function compareDates(a, b) {
  return a.date.localeCompare(b.date) || (a.timestamp || '').localeCompare(b.timestamp || '') || a.id.localeCompare(b.id);
}
function featureRows(data, commits, group = 'all') {
  const indexed = new Map();
  for (const c of commits) for (const id of c.features) {
    if (!indexed.has(id)) indexed.set(id, []);
    indexed.get(id).push(c);
  }
  return data.features.filter(f => (group === 'all' || f.group === group) && indexed.has(f.id)).map(f => ({...f, commits: indexed.get(f.id).slice().sort(compareDates)}));
}
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

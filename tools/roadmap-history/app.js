/* DOM rendering uses textContent for all repository-derived strings. */
const DATA = JSON.parse(document.getElementById('roadmap-data').textContent);
const featureMap = new Map(DATA.features.map(f => [f.id, f]));
const groupMap = new Map(DATA.groups.map(g => [g.id, g]));
const allRows = new Map(featureRows(DATA, DATA.commits).map(f => [f.id, f]));
const bounds = dateBounds(DATA.commits);
const number = value => value.toLocaleString('de-DE');
const $ = id => document.getElementById(id);
const el = (tag, className = '', text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const button = (text, className, handler) => {
  const node = el('button', className, text);
  node.type = 'button';
  node.addEventListener('click', handler);
  return node;
};
const colorize = (node, group) => node.style.setProperty('--group-color', group.color);
const kindLabel = c => KINDS[c.kind] || 'Änderung';
const commitLink = c => {
  const link = el('a', 'commit-link', c.id.slice(0, 8) + ' ↗');
  link.href = 'https://github.com/EarlySalty/Deadlock-Twitch-Bot/commit/' + c.id;
  link.target = '_blank'; link.rel = 'noopener noreferrer';
  link.setAttribute('aria-label', 'Commit ' + c.id.slice(0, 8) + ' auf GitHub öffnen');
  return link;
};
const initial = new URLSearchParams(location.search);
let view = ['roadmap', 'tree', 'list'].includes(initial.get('view')) ? initial.get('view') : 'roadmap';
let selectedCommits = [];
let currentFeature = '';
let currentMonth = '';
let listLimit = 80;
let searchTimer;

for (const g of DATA.groups) {
  const option = el('option', '', g.title); option.value = g.id; $('group').append(option);
}
for (const key of ['group', 'period', 'kind']) {
  const value = initial.get(key);
  if ([...$(key).options].some(option => option.value === value)) $(key).value = value;
}
$('search').value = initial.get('search') || '';
$('maintenance').checked = initial.get('maintenance') === '1';
for (const key of ['from', 'to']) {
  const value = initial.get(key);
  if (/^\d{4}-\d{2}-\d{2}$/.test(value || '')) $(key).value = value;
  $(key).min = bounds.first; $(key).max = bounds.last;
}
function filters() {
  return {search: $('search').value, group: $('group').value, period: $('period').value, kind: $('kind').value,
    maintenance: $('maintenance').checked, from: $('from').value, to: $('to').value};
}
function saveState() {
  const params = new URLSearchParams();
  const state = filters();
  for (const key of ['search', 'group', 'period', 'kind']) if (state[key]) params.set(key, state[key]);
  if (state.period === 'custom') for (const key of ['from', 'to']) if (state[key]) params.set(key, state[key]);
  if (state.maintenance) params.set('maintenance', '1');
  params.set('view', view);
  if (currentFeature) params.set('feature', currentFeature);
  if (currentMonth) params.set('month', currentMonth);
  history.replaceState(null, '', location.pathname + '?' + params.toString());
}
function summary() {
  $('snapshot-date').textContent = dateLabel(bounds.last);
  $('snapshot-ref').textContent = DATA.ref + ' · ' + DATA.revision.slice(0, 8);
  const active = DATA.features.filter(f => f.id !== 'other' && allRows.has(f.id)).length;
  const items = [
    [number(active), 'Features mit Historie', 'Nach Fachgebieten geordnet'],
    [number(DATA.commits.length), 'Codeänderungen', 'Gesamte Historie · ohne Merges'],
    [number(DATA.commits.filter(c => c.kind === 'fix').length), 'Fehlerbehebungen', 'Commit-Typ fix · gesamte Historie'],
    [number(monthsBetween(bounds.first, bounds.last).length), 'Monate Entwicklung', dateLabel(bounds.first) + ' – ' + dateLabel(bounds.last)],
  ];
  for (const [value, label, hint] of items) {
    const node = el('div', 'stat'); node.append(el('strong', '', value), el('span', '', label), el('small', '', hint)); $('stats').append(node);
  }
  const generated = new Date(DATA.generatedAt);
  $('generated-at').textContent = 'Erzeugt: ' + generated.toLocaleString('de-DE', {timeZone: 'Europe/Berlin'}) + ' · Europe/Berlin';
  if (DATA.shallow) $('notices').append(el('p', 'notice', 'Unvollständige Git-Historie: Diese Quelle ist ein flacher Klon. Frühe Änderungen können fehlen.'));
  if (Date.now() - generated.getTime() > 7 * 86400000) $('notices').append(el('p', 'notice', 'Dieser Datenstand wurde seit über sieben Tagen nicht aktualisiert. Neuere Änderungen können fehlen.'));
}
function empty(message) {
  const box = el('div', 'empty');
  box.append(el('strong', '', 'Keine passenden Änderungen'), el('p', '', message));
  $('content').append(box);
}
function orderedGroups(rows) {
  return DATA.groups.map(group => ({...group, rows: rows.filter(f => f.group === group.id).sort((a, b) => compareDates(b.commits.at(-1), a.commits.at(-1)) || a.title.localeCompare(b.title, 'de'))})).filter(g => g.rows.length);
}
function renderRoadmap(rows, range) {
  const months = monthsBetween(range.from, range.to);
  const board = el('div', 'board');
  board.tabIndex = 0;
  board.setAttribute('role', 'region');
  board.setAttribute('aria-label', 'Horizontal scrollbare Zeitachse, Monate von links nach rechts');
  const grid = el('div', 'board-grid');
  const labelWidth = matchMedia('(max-width:600px)').matches ? 145 : 230;
  grid.style.gridTemplateColumns = labelWidth + 'px repeat(' + months.length + ', minmax(230px, 1fr))';
  grid.append(el('div', 'axis-label', 'FEATURE / ENTWICKLUNG'));
  for (const month of months) grid.append(el('div', 'month-label', monthLabel(month)));
  for (const group of orderedGroups(rows)) {
    const heading = el('div', 'group-row'); colorize(heading, group);
    heading.append(el('span', '', '↳ ' + group.title + ' · ' + group.rows.length + ' Features'));
    grid.append(heading);
    for (const feature of group.rows) {
      const label = button('', 'feature-label', () => openDetail(feature.id));
      colorize(label, group);
      label.append(el('span', 'feature-group', 'Feature-Entwicklung'), el('strong', '', feature.title), el('small', '', number(feature.commits.length) + ' Änderungen im Zeitraum'), el('small', '', 'Gesamte Linie öffnen →'));
      label.setAttribute('aria-label', feature.title + ': gefilterte Entwicklung öffnen');
      grid.append(label);
      const first = feature.commits[0].date.slice(0, 7), last = feature.commits.at(-1).date.slice(0, 7);
      for (const month of months) {
        const cell = el('div', 'month-cell' + (month >= first && month <= last ? ' active' : ''));
        colorize(cell, group);
        const events = feature.commits.filter(c => c.date.startsWith(month));
        if (events.length) {
          const sample = representative(events);
          const card = button('', 'milestone ' + sample.kind, () => openDetail(feature.id, month));
          const meta = el('span', 'event-meta');
          meta.append(el('span', 'badge', kindLabel(sample)), el('span', '', dateLabel(sample.date).slice(0, 6)));
          const additions = events.filter(c => c.kind === 'feat').length, fixes = events.filter(c => c.kind === 'fix').length;
          const counts = [additions ? additions + ' Erweiterungen' : '', fixes ? fixes + ' Fixes' : ''].filter(Boolean).join(' · ');
          card.append(meta, el('span', 'milestone-title', sample.title), el('span', 'counts', counts || number(events.length) + ' Änderungen'), el('span', 'counts', number(events.length) + ' Änderungen ansehen →'));
          card.title = feature.title + ' · ' + monthLabel(month) + '\n' + sample.title;
          card.setAttribute('aria-label', feature.title + ', ' + monthLabel(month) + ', ' + events.length + ' Änderungen anzeigen');
          cell.append(card);
        }
        grid.append(cell);
      }
    }
  }
  board.append(grid); $('content').append(board);
}
function renderTree(rows) {
  const container = el('div', 'tree-content');
  const root = el('div', 'tree-root');
  root.append(el('span', 'brand-mark', 'D'));
  const info = el('div'); info.append(el('strong', '', 'Twitch-Plattform'), el('small', '', 'Bereich → Feature → datierte Entwicklung · fachlich gruppiert, keine technischen Abhängigkeiten'));
  root.append(info); container.append(root);
  for (const group of orderedGroups(rows)) {
    const family = el('section', 'family'); colorize(family, group);
    const heading = el('div', 'family-heading'); heading.append(el('h3', '', group.title), el('span', '', group.description)); family.append(heading);
    const cards = el('div', 'feature-cards');
    for (const feature of group.rows) {
      const card = el('article', 'feature-card');
      card.append(el('h3', '', feature.title), el('p', 'description', feature.description));
      const dates = el('div', 'dates');
      const full = allRows.get(feature.id).commits;
      for (const [label, value] of [['Erster Nachweis', full[0].date], ['Zuletzt geändert', full.at(-1).date]]) {
        const entry = el('span', '', label); entry.append(el('strong', '', dateLabel(value))); dates.append(entry);
      }
      card.append(dates);
      const history = el('div', 'mini-history');
      for (const c of milestones(feature.commits)) {
        const node = el('div', 'mini-node ' + c.kind);
        const time = el('time', '', dateLabel(c.date) + ' · ' + kindLabel(c)); time.dateTime = c.date;
        node.append(time, el('p', '', c.title)); history.append(node);
      }
      card.append(history, button(number(feature.commits.length) + ' Änderungen im Filter öffnen →', 'open-feature', () => openDetail(feature.id)));
      cards.append(card);
    }
    family.append(cards); container.append(family);
  }
  $('content').append(container);
}
function renderList() {
  const container = el('div', 'list-content');
  const events = selectedCommits.slice().sort(compareDates).reverse();
  const days = new Map();
  for (const c of events.slice(0, listLimit)) {
    if (!days.has(c.date)) days.set(c.date, []);
    days.get(c.date).push(c);
  }
  for (const [date, commits] of days) {
    const day = el('section', 'list-day');
    const time = el('h3', 'list-date', dateLabel(date)); day.append(time);
    const entries = el('div', 'list-events');
    for (const c of commits) {
      const entry = el('article', 'event-line ' + c.kind);
      entry.append(el('span', 'badge', kindLabel(c)), el('h3', '', c.title));
      const links = el('div', 'event-links');
      for (const id of c.features) if (featureMap.has(id)) links.append(button(featureMap.get(id).title, 'feature-link', () => openDetail(id)));
      links.append(commitLink(c)); entry.append(links); entries.append(entry);
    }
    day.append(entries); container.append(day);
  }
  if (events.length > listLimit) container.append(button('Weitere Änderungen laden (' + number(events.length - listLimit) + ' verbleibend)', 'more', () => {
    listLimit += 80; $('content').replaceChildren(); renderList();
  }));
  $('content').append(container);
}
function historyEvent(c) {
  const event = el('article', 'history-event ' + c.kind);
  const meta = el('div', 'event-meta');
  const date = el('time', '', dateLabel(c.date)); date.dateTime = c.date;
  meta.append(date, el('span', 'badge', kindLabel(c)), commitLink(c));
  event.append(meta, el('h3', '', c.title));
  if (c.note) event.append(el('p', '', c.note));
  const details = el('details');
  const basis = {subject: 'Automatisch · Commit-Titel', path: 'Automatisch · Dateipfade', crosscut: 'Automatisch · bereichsübergreifend', curated: 'Redaktionell zugeordnet', unassigned: 'Noch nicht zugeordnet'};
  details.append(el('summary', '', (basis[c.basis] || 'Automatische Zuordnung') + ' · Belege anzeigen'));
  details.append(el('p', '', 'Originaler Commit-Titel'), el('pre', '', c.subject));
  if (c.evidence.length) details.append(el('p', '', 'Zuordnungsgrund'), el('pre', '', c.evidence.join('\n')));
  details.append(el('p', '', number(c.pathCount) + ' geänderte Dateien' + (c.pathCount > c.paths.length ? ' · erste ' + c.paths.length + ' angezeigt; alle im Commit-Link' : '')));
  if (c.paths.length) details.append(el('pre', '', c.paths.join('\n')));
  event.append(details);
  return event;
}
function openDetail(id, month = '', full = false) {
  const feature = allRows.get(id);
  if (!feature) return;
  currentFeature = id;
  currentMonth = month;
  const group = groupMap.get(feature.group);
  const total = feature.commits;
  const matching = selectedCommits.filter(c => c.features.includes(id) && (!month || c.date.startsWith(month))).sort(compareDates);
  const events = full ? total : matching;
  colorize($('detail'), group);
  $('detail-group').textContent = group.title + ' / FEATURE-ENTWICKLUNG';
  $('detail-title').textContent = feature.title;
  const body = $('detail-body'); body.replaceChildren();
  body.append(el('p', 'detail-intro', feature.description));
  const facts = el('div', 'detail-facts');
  for (const [label, value] of [['Erster Nachweis im Repository', dateLabel(total[0].date)], ['Zuletzt im Repository geändert', dateLabel(total.at(-1).date)], ['Gesamte Historie inkl. Technik & Pflege', number(total.length) + ' Änderungen'], ['Davon Fehlerbehebungen', number(total.filter(c => c.kind === 'fix').length)]]) {
    const fact = el('div', '', label); fact.append(el('strong', '', value)); facts.append(fact);
  }
  body.append(facts);
  const scope = el('div', 'detail-scope');
  scope.append(el('p', '', full ? 'Gesamte Entwicklung einschließlich Doku, Tests und Pflege. Zeitlich von früher nach heute.' : number(matching.length) + ' Änderungen passend zu deinen Filtern' + (month ? ' · ' + monthLabel(month) : '') + '. Zeitlich von früher nach heute.'));
  scope.append(button(full ? 'Zur gefilterten Auswahl' : 'Vollständige Entwicklung zeigen (' + number(total.length) + ')', 'more', () => openDetail(id, month, !full)));
  body.append(scope);
  const history = el('div', 'history'); body.append(history);
  let loaded = 0;
  const more = button('Weitere Änderungen laden', 'more', () => appendPage());
  function appendPage() {
    for (const c of events.slice(loaded, loaded + 40)) history.append(historyEvent(c));
    loaded = Math.min(loaded + 40, events.length);
    more.textContent = 'Weitere Änderungen laden (' + number(events.length - loaded) + ' verbleibend)';
    more.hidden = loaded >= events.length;
  }
  appendPage(); body.append(more);
  if (!events.length) history.append(el('p', 'detail-intro', 'Keine Änderungen in dieser Auswahl. Öffne die vollständige Entwicklung oder passe die Filter an.'));
  if (!$('detail').open) $('detail').showModal();
  document.body.style.overflow = 'hidden';
  $('detail').scrollTop = 0;
  saveState();
}
function render() {
  const state = filters();
  selectedCommits = filterCommits(DATA, state);
  const range = rangeFor(state.period, bounds, state.from, state.to);
  const rows = featureRows(DATA, selectedCommits, state.group);
  $('custom-dates').hidden = state.period !== 'custom';
  for (const node of document.querySelectorAll('[data-view]')) node.setAttribute('aria-pressed', String(node.dataset.view === view));
  $('result-count').textContent = number(rows.length) + ' Feature-Linien · ' + number(selectedCommits.length) + ' eindeutige Änderungen · ' + dateLabel(range.from) + ' – ' + dateLabel(range.to);
  $('view-hint').textContent = {
    roadmap: 'Zeit läuft von links nach rechts. Jede Karte bündelt einen Monat und zeigt eine Erweiterung oder die jüngste Änderung. Anklicken öffnet alle Änderungen. Die Zeitachse lässt sich horizontal scrollen.',
    tree: 'Jeder Bereich verzweigt sich in Features und deren datierte Entwicklung. Gezeigt werden bis zu drei Stationen aus deiner Auswahl; ein Klick öffnet die vollständige Linie.',
    list: 'Alle passenden Änderungen, neueste zuerst. Feature-Namen öffnen die Entwicklungslinie; der Commit-Link führt zum tatsächlichen Code-Diff.'
  }[view];
  $('content').replaceChildren();
  if (range.from > range.to) empty('Das Startdatum liegt nach dem Enddatum. Bitte korrigiere den Zeitraum.');
  else if (!rows.length) empty('Probiere einen anderen Suchbegriff, einen längeren Zeitraum oder blende Technik & Pflege ein.');
  else if (view === 'roadmap') renderRoadmap(rows, {from: range.from > bounds.first ? range.from : bounds.first, to: range.to < bounds.last ? range.to : bounds.last});
  else if (view === 'tree') renderTree(rows);
  else renderList();
  saveState();
}
$('filters').addEventListener('submit', event => event.preventDefault());
$('search').addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {listLimit = 80; render();}, 160);
});
for (const id of ['group', 'period', 'kind', 'maintenance', 'from', 'to']) $(id).addEventListener('change', () => {
  listLimit = 80; render();
});
for (const node of document.querySelectorAll('[data-view]')) node.addEventListener('click', () => {
  view = node.dataset.view; listLimit = 80; render();
});
$('reset-filters').addEventListener('click', () => {
  clearTimeout(searchTimer);
  $('filters').reset(); $('from').value = ''; $('to').value = ''; listLimit = 80; render();
});
$('close-detail').addEventListener('click', () => $('detail').close());
$('detail').addEventListener('close', () => {
  currentFeature = ''; currentMonth = ''; document.body.style.overflow = ''; saveState();
});
$('detail').addEventListener('click', event => {
  const rect = $('detail').getBoundingClientRect();
  if (event.target === $('detail') && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) $('detail').close();
});
const compactLayout = matchMedia('(max-width:600px)');
compactLayout.addEventListener('change', () => {if (view === 'roadmap') render();});
summary();
render();
if (initial.has('feature')) {
  if (allRows.has(initial.get('feature'))) {
    const month = /^\d{4}-\d{2}$/.test(initial.get('month') || '') ? initial.get('month') : '';
    openDetail(initial.get('feature'), month);
  } else $('notices').append(el('p', 'notice', 'Das verlinkte Feature ist in diesem Datenstand nicht enthalten.'));
}

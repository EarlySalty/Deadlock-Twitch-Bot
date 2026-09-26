/* Repository-derived content is always inserted as text, never as HTML. */
const DATA = JSON.parse(document.getElementById('roadmap-data').textContent);
const INDEX = familyIndex(DATA);
const bounds = dateBounds(DATA.commits);
const $ = id => document.getElementById(id);
const number = value => value.toLocaleString('de-DE');
const el = (tag, className = '', text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const button = (text, className, handler) => {
  const node = el('button', className, text); node.type = 'button';
  node.addEventListener('click', handler); return node;
};
const svgEl = (tag, attributes = {}) => {
  const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [name, value] of Object.entries(attributes)) node.setAttribute(name, String(value));
  return node;
};
const externalLink = (label, suffix, className = '') => {
  const node = el('a', className, label);
  node.href = 'https://github.com/EarlySalty/Deadlock-Twitch-Bot/' + suffix;
  node.target = '_blank'; node.rel = 'noopener noreferrer'; return node;
};
const commitLink = c => externalLink(c.id.slice(0, 8) + ' · Code-Diff ↗', 'commit/' + encodeURIComponent(c.id), 'commit-link');
const sourceLink = path => externalLink(path, 'blob/' + DATA.revision + '/' + path.split('/').map(encodeURIComponent).join('/'));
const initial = new URLSearchParams(location.search);
let view = initial.get('view') === 'list' ? 'list' : 'tree';
let focusId = initial.get('focus') || (initial.has('search') || initial.has('feature') ? 'product' : INDEX.has('uplink') ? 'uplink' : 'product');
if (!INDEX.has(focusId)) focusId = 'product';
let collapsed = new Set((initial.get('collapsed') || '').split(',').filter(id => INDEX.has(id)));
let currentFeature = '', currentEvent = '', currentMonth = '', currentBundle = null;
let selectedCommits = [], selection, graph;
let listLimit = 80, detailPage = 0, detailFull = false, includeChildren = true;
let returnFocus = null, ignoreClose = false, searchTimer, cameraTimer, paintFrame;
const mobileLayout = matchMedia('(max-width:760px)');
const camera = {x: 0, y: 0, zoom: 1};
for (const [key, param] of [['x', 'x'], ['y', 'y'], ['zoom', 'zoom']]) {
  const value = Number(initial.get(param));
  if (initial.has(param) && Number.isFinite(value)) camera[key] = value;
}
camera.zoom = Math.max(.08, Math.min(1.75, camera.zoom));
const nodeElements = new Map();
for (const group of DATA.groups) {const option = el('option', '', group.title); option.value = group.id; $('group').append(option);}
for (const node of INDEX.values()) {
  if (node.id === 'product') continue;
  const option = el('option', '', '　'.repeat(Math.max(0, node.depth - 1)) + node.title + (!node.first ? ' (ohne Git-Nachweis)' : ''));
  option.value = node.id; $('focus-feature').append(option);
}
for (const id of ['group', 'period', 'kind']) {
  if ([...$(id).options].some(o => o.value === initial.get(id))) $(id).value = initial.get(id);
}
$('search').value = initial.get('search') || '';
$('maintenance').checked = initial.get('maintenance') === '1';
for (const id of ['from', 'to']) {
  if (/^\d{4}-\d{2}-\d{2}$/.test(initial.get(id) || '')) $(id).value = initial.get(id);
  $(id).min = bounds.first; $(id).max = bounds.last;
}
function filters() {
  return {search: $('search').value, group: $('group').value, period: $('period').value, kind: $('kind').value, maintenance: $('maintenance').checked, from: $('from').value, to: $('to').value};
}
function saveState() {
  const params = new URLSearchParams();
  const state = filters();
  for (const key of ['search', 'group', 'period', 'kind']) if (state[key]) params.set(key, state[key]);
  if (state.period === 'custom') for (const key of ['from', 'to']) if (state[key]) params.set(key, state[key]);
  if (state.maintenance) params.set('maintenance', '1');
  params.set('view', view); params.set('focus', focusId);
  if (collapsed.size) params.set('collapsed', [...collapsed].sort().join(','));
  if (currentFeature) params.set('feature', currentFeature);
  if (currentEvent) params.set('event', currentEvent);
  if (currentMonth) params.set('month', currentMonth);
  if (detailFull) params.set('full', '1');
  if (!includeChildren) params.set('children', '0');
  for (const key of ['x', 'y', 'zoom']) params.set(key, String(Math.round(camera[key] * 100) / 100));
  history.replaceState(null, '', location.pathname + '?' + params.toString());
}
function summary() {
  $('snapshot-date').textContent = dateLabel(bounds.last);
  $('snapshot-ref').textContent = DATA.ref + ' · ' + DATA.revision.slice(0, 8);
  const active = [...INDEX.values()].filter(f => f.id !== 'product' && f.id !== 'other' && f.first).length;
  for (const [value, label] of [[active, 'Funktionen mit Git-Nachweis'], [new Set(DATA.commits.map(c => c.id)).size, 'eindeutige Codeänderungen'], [DATA.commits.filter(c => ['ambiguous', 'unassigned'].includes(c.basis)).length, 'offene Zuordnungen']]) {
    const stat = el('span'); stat.append(el('strong', '', number(value)), document.createTextNode(label)); $('stats').append(stat);
  }
  const generated = new Date(DATA.generatedAt);
  $('generated-at').textContent = 'Erzeugt: ' + generated.toLocaleString('de-DE', {timeZone: 'Europe/Berlin'}) + ' · Europe/Berlin';
  if (DATA.shallow) $('notices').append(el('p', 'notice', 'Flacher Klon: Frühere Git-Nachweise können fehlen.'));
  if (Date.now() - generated.getTime() > 7 * 86400000) $('notices').append(el('p', 'notice', 'Dieser Datenstand ist über sieben Tage alt. Neuere Änderungen können fehlen.'));
  const unverified = DATA.features.filter(f => f.id !== 'other' && !f.relation?.verified);
  if (unverified.length) $('notices').append(el('p', 'notice', number(unverified.length) + ' Funktionszuordnungen haben in diesem Git-Stand keinen vollständigen Komponentenbeleg. Die Details benennen die fehlenden Quellen.'));
}
function selectedKey() {
  if (currentEvent) {
    const found = graph?.nodes.find(n => n.type === 'milestone' && n.featureId === currentFeature && n.commitIds.includes(currentEvent));
    if (found) return found.key;
  }
  return currentFeature ? 'l:' + currentFeature : '';
}
function highlightBranch() {
  const active = currentFeature;
  for (const path of document.querySelectorAll('#edges .branch-line')) path.classList.toggle('is-active', path.dataset.feature === active);
}
function clampCamera() {
  if (!graph) return;
  const width = $('viewport').clientWidth, height = $('viewport').clientHeight;
  camera.x = Math.max(Math.min(24, width - graph.width * camera.zoom - 24), Math.min(24, camera.x));
  camera.y = Math.max(Math.min(12, height - graph.height * camera.zoom - 16), Math.min(12, camera.y));
}
function paintWindow() {
  paintFrame = null;
  if (view !== 'tree' || !graph) return;
  clampCamera();
  $('stage').style.transform = `translate(${camera.x}px,${camera.y}px) scale(${camera.zoom})`;
  $('axis').style.transform = `translateX(${camera.x}px)`;
  const axisLabels = [...$('axis').children];
  if (axisLabels[0]) {
    axisLabels[0].style.width = graph.axis.x0 * camera.zoom + 'px';
    axisLabels[0].textContent = graph.axis.x0 * camera.zoom >= 118 ? 'ZEITACHSE' : '';
  }
  for (const label of axisLabels.slice(1)) {
    const x = Number(label.dataset.x), width = Number(label.dataset.w);
    label.style.left = x * camera.zoom + 'px';
    label.style.width = width * camera.zoom + 'px';
    label.hidden = width * camera.zoom < 44;
  }
  const left = -camera.x / camera.zoom - 120, top = -camera.y / camera.zoom - 120;
  const right = left + $('viewport').clientWidth / camera.zoom + 240, bottom = top + $('viewport').clientHeight / camera.zoom + 240;
  const activeKey = document.activeElement?.closest('.branch-label, .milestone')?.dataset.key;
  const keep = new Set();
  for (const node of graph.nodes) {
    if (!(node.x < right && node.x + node.width > left && node.y < bottom && node.y + node.height > top) && node.key !== activeKey && node.key !== selectedKey()) continue;
    keep.add(node.key);
    if (!nodeElements.has(node.key)) {
      const element = graphNode(node);
      Object.assign(element.style, {left: node.x + 'px', top: node.y + 'px', width: node.width + 'px', height: node.height + 'px'});
      element.dataset.key = node.key;
      nodeElements.set(node.key, element); $('nodes').append(element);
    }
    nodeElements.get(node.key).classList.toggle('is-selected', node.key === selectedKey());
  }
  for (const [key, node] of nodeElements) if (!keep.has(key)) {node.remove(); nodeElements.delete(key);}
  $('zoom-reset').textContent = Math.round(camera.zoom * 100) + ' %';
  $('graph-status').textContent = number(nodeElements.size) + ' / ' + number(graph.nodes.length) + ' Knoten im Ausschnitt';
}
function schedulePaint() {if (!paintFrame) paintFrame = requestAnimationFrame(paintWindow);}
function reveal(key) {
  const node = graph?.nodes.find(n => n.key === key);
  if (!node || view !== 'tree') return;
  const margin = 22, width = $('viewport').clientWidth, height = $('viewport').clientHeight;
  const left = node.x * camera.zoom + camera.x, right = (node.x + node.width) * camera.zoom + camera.x;
  const top = node.y * camera.zoom + camera.y, bottom = (node.y + node.height) * camera.zoom + camera.y;
  if (right > width - margin) camera.x -= right - width + margin;
  if (left < margin) camera.x += margin - left;
  if (bottom > height - margin) camera.y -= bottom - height + margin;
  if (top < margin) camera.y += margin - top;
  paintWindow();
}
function resetCamera() {Object.assign(camera, {x: 0, y: 0, zoom: 1});}
function zoomTo(value, x = $('viewport').clientWidth / 2, y = $('viewport').clientHeight / 2) {
  const next = Math.max(.08, Math.min(1.75, value));
  camera.x = x - (x - camera.x) / camera.zoom * next;
  camera.y = y - (y - camera.y) / camera.zoom * next;
  camera.zoom = next; paintWindow(); saveState();
}
function focusFeature(id, detail = false) {
  if (!INDEX.has(id)) return;
  focusId = id; view = 'tree';
  let node = INDEX.get(id);
  while (node) {collapsed.delete(node.id); node = INDEX.get(node.parentId);}
  resetCamera(); render();
  reveal('l:' + id); saveState();
  if (detail) openDetail(id);
}
function graphNode(node) {
  const feature = INDEX.get(node.featureId);
  if (node.type === 'milestone') {
    const card = button('', 'milestone kind-' + node.kind, () => openDetail(node.featureId, node.leadId, '', node.commitIds));
    card.style.setProperty('--r', node.r + 'px');
    card.setAttribute('aria-label', feature.title + ', ' + dateLabel(node.date) + ', ' + (KINDS[node.kind] || 'Änderung') + ': ' + node.title + '. ' + node.count + (node.count === 1 ? ' Änderung öffnen' : ' Änderungen im Bündel öffnen'));
    const tip = el('span', 'milestone-tip');
    tip.append(el('strong', '', node.title));
    tip.append(el('time', '', dateLabel(node.first) + (node.last !== node.first ? ' bis ' + dateLabel(node.last) : '')));
    tip.append(el('span', '', (KINDS[node.kind] || 'Änderung') + ' · ' + number(node.count) + (node.count === 1 ? ' Änderung' : ' Änderungen')));
    card.append(tip);
    return card;
  }
  const label = el('div', 'branch-label' + (node.context ? ' context' : '') + (node.dormant ? ' dormant' : ''));
  label.dataset.feature = node.featureId;
  const title = button('', 'branch-title', () => openDetail(node.featureId));
  title.setAttribute('aria-label', feature.title + '. Zweigbeginn: ' + dateLabel(node.date) + (node.preexisting ? ', Bestand beim Start' : '') + '. ' + feature.description + (node.context ? ' Kontext-Elternteil.' : ''));
  title.append(el('span', 'branch-name', node.text));
  const stamp = el('time', 'branch-date', (node.preexisting ? 'Bestand beim Start' : dateLabel(node.date)));
  stamp.dateTime = node.date; title.append(stamp);
  label.append(title);
  if (node.hasChildren) {
    const caret = button(node.collapsed ? '+' : '−', 'branch-caret', () => {
      if (collapsed.has(node.featureId)) collapsed.delete(node.featureId); else collapsed.add(node.featureId);
      render(); reveal('l:' + node.featureId);
      nodeElements.get('l:' + node.featureId)?.querySelector('.branch-caret')?.focus({preventScroll: true});
    });
    caret.setAttribute('aria-expanded', String(!node.collapsed));
    caret.setAttribute('aria-label', feature.title + ': Unterzweige ' + (node.collapsed ? 'aufklappen' : 'zuklappen'));
    label.append(caret);
  }
  return label;
}
function renderGraph() {
  graph = layoutFamily(selection);
  nodeElements.clear(); $('nodes').replaceChildren(); $('edges').replaceChildren(); $('axis').replaceChildren();
  $('stage').style.width = graph.width + 'px'; $('stage').style.height = graph.height + 'px';
  $('edges').setAttribute('width', graph.width); $('edges').setAttribute('height', graph.height);
  $('axis').style.width = graph.width + 'px';
  const structure = el('span', 'axis-label axis-structure', 'ZEITACHSE');
  structure.style.width = graph.axis.x0 + 'px'; $('axis').append(structure);
  for (const month of graph.axis.months) {
    const label = el('span', 'axis-label', month.label);
    Object.assign(label.style, {left: month.x + 'px', width: month.width + 'px'});
    label.dataset.x = month.x; label.dataset.w = month.width; $('axis').append(label);
  }
  const gridTop = 40, gridBottom = graph.height - 8;
  for (const week of graph.weeks) $('edges').append(svgEl('line', {x1: week.x, x2: week.x, y1: gridTop, y2: gridBottom, class: 'week-line'}));
  for (const month of graph.axis.months) $('edges').append(svgEl('line', {x1: month.x, x2: month.x, y1: gridTop, y2: gridBottom, class: 'month-line'}));
  if (graph.trunk) {
    $('edges').append(svgEl('line', {x1: graph.trunk.x0, x2: graph.trunk.x1, y1: graph.trunk.y, y2: graph.trunk.y, class: 'trunk-line', 'vector-effect': 'non-scaling-stroke'}));
    const trunkLabel = svgEl('text', {x: graph.axis.x0, y: graph.trunk.y - 15, class: 'trunk-label'});
    trunkLabel.textContent = INDEX.get('product').title; $('edges').append(trunkLabel);
  }
  for (const fork of graph.forks) {
    const lead = Math.min(16, Math.max(0, fork.x - fork.sourceX));
    const drop = fork.y2 > fork.y1 ? 14 : -14;
    const d = `M${fork.x - lead},${fork.y1} C${fork.x - 2},${fork.y1} ${fork.x},${fork.y1 + drop} ${fork.x},${fork.y2}`;
    $('edges').append(svgEl('path', {d, class: 'fork-edge' + (fork.context ? ' context' : ''), 'data-target': fork.key, 'vector-effect': 'non-scaling-stroke'}));
  }
  for (const branch of graph.branches) {
    const line = svgEl('path', {d: `M${branch.forkX},${branch.y} L${branch.endX},${branch.y}`, class: 'branch-line' + (branch.dormant ? ' dormant' : '') + (branch.context ? ' context' : ''), 'data-feature': branch.id, 'vector-effect': 'non-scaling-stroke'});
    line.style.strokeWidth = branch.strokeWidth; line.style.opacity = branch.opacity;
    $('edges').append(line);
  }
  highlightBranch();
  $('empty').hidden = selection.commitCount > 0;
  const range = rangeFor(filters().period, bounds, filters().from, filters().to);
  $('empty-message').textContent = range.from > range.to ? 'Das Startdatum liegt nach dem Enddatum. Bitte korrigiere den Zeitraum.' : !INDEX.get(focusId).first ? 'Für diese redaktionelle Funktion gibt es im ausgewerteten Git-Stand noch keinen zugeordneten Nachweis. Es wird kein Datum erfunden.' : 'Suchbegriff, Zeitraum oder fokussierten Zweig ändern. Doku, Tests und Pflege lassen sich zusätzlich einblenden.';
  paintWindow();
}
function historyEvent(c) {
  const event = el('article', 'history-event ' + c.kind + (currentEvent === c.id ? ' selected-event' : ''));
  event.dataset.commit = c.id;
  if (currentEvent === c.id) event.append(el('div', 'selected-label', 'AUSGEWÄHLTE ÄNDERUNG'));
  const meta = el('div', 'event-meta');
  const time = el('time', '', dateLabel(c.date)); time.dateTime = c.date;
  meta.append(time, el('span', 'badge', KINDS[c.kind] || 'Änderung'), commitLink(c));
  event.append(meta, el('h3', '', c.title));
  if (c.note) event.append(el('p', '', c.note));
  const details = el('details');
  const labels = {subject: 'Automatisch · Commit-Titel', path: 'Automatisch · Dateipfade', crosscut: 'Bereichsübergreifend', ambiguous: 'Unklare Zuordnung', curated: 'Redaktionell korrigiert', unassigned: 'Noch nicht zugeordnet'};
  details.append(el('summary', '', (labels[c.basis] || 'Zuordnung') + ' · Belege'));
  details.append(el('p', '', 'Originaler Commit-Titel'), el('pre', '', c.subject));
  if (c.evidence?.length) details.append(el('p', '', 'Zuordnungsgrund'), el('pre', '', c.evidence.join('\n')));
  details.append(el('p', '', number(c.pathCount || 0) + ' geänderte Dateien' + (c.pathCount > c.paths.length ? ' · vollständige Liste im Code-Diff' : '')));
  if (c.paths?.length) details.append(el('pre', '', c.paths.join('\n')));
  const links = el('div', 'relation-links');
  for (const id of c.features) links.append(button(INDEX.get(id).title, 'feature-link', () => {focusFeature(id); openDetail(id, c.id);}));
  details.append(links); event.append(details); return event;
}
function detailEvents(node) {
  if (currentBundle) {
    const bundle = new Set(currentBundle);
    return DATA.commits.filter(c => bundle.has(c.id)).slice().sort(compareDates);
  }
  const ids = new Set((includeChildren ? node.allCommits : node.commits).map(c => c.id));
  return (detailFull ? DATA.commits : selectedCommits).filter(c => ids.has(c.id) && (!currentMonth || c.date.startsWith(currentMonth))).slice().sort(compareDates);
}
function paintDetail() {
  const node = INDEX.get(currentFeature);
  if (!node) return;
  $('detail-group').textContent = node.id === 'product' ? 'PRODUKTURSPRUNG / GIT-NACHWEISE' : 'FUNKTION / ' + (node.relation?.kind === 'historical' && node.relation?.verified ? 'HISTORISCH BELEGT' : 'FACHLICH ZUGEORDNET');
  $('detail-title').textContent = node.title;
  const body = $('detail-body'); body.replaceChildren();
  body.append(el('p', 'detail-intro', node.description));
  const facts = el('div', 'detail-facts');
  for (const [label, value] of [['Erster Git-Nachweis inkl. Kinder', dateLabel(node.first)], ['Letzte Änderung inkl. Kinder', dateLabel(node.last)], ['Erster direkter Git-Nachweis', dateLabel(node.firstDirect)], ['Eindeutig im gesamten Teilbaum', number(node.allCommits.length) + ' Änderungen']]) {
    const item = el('div', '', label); item.append(el('strong', '', value)); facts.append(item);
  }
  body.append(facts);
  const relatives = el('div', 'detail-relations');
  if (node.parentId) {
    relatives.append(el('span', '', 'Elternfeature'), button('← ' + INDEX.get(node.parentId).title, 'feature-link', () => focusFeature(node.parentId, true)));
  }
  relatives.append(el('span', '', node.children.length + ' direkte Unterfunktionen'));
  const children = el('div', 'relation-links');
  for (const id of node.children) children.append(button(INDEX.get(id).title, 'feature-link', () => focusFeature(id, true)));
  relatives.append(children); body.append(relatives);
  if (node.relation) {
    const evidence = el('details', 'evidence');
    evidence.append(el('summary', '', 'Grundlage der Funktion & Elternzuordnung'));
    evidence.append(el('p', '', node.relation.kind === 'historical' ? 'Als historische Beziehung eingetragen. ' + (node.relation.verified ? 'Beleg ist im ausgewerteten Stand enthalten.' : 'Der Beleg ist in diesem Stand NICHT bestätigt.') : 'Redaktionelle, fachliche Einordnung. Kein Beweis einer historischen Abspaltung.'));
    evidence.append(el('p', '', node.relation.reason));
    for (const path of node.relation.verifiedSources || []) evidence.append(sourceLink(path));
    for (const path of node.relation.missingSources || []) evidence.append(el('p', '', 'Im ausgewerteten Stand nicht belegt: ' + path));
    if (node.relation.commit) evidence.append(externalLink('Eingetragener historischer Beleg ↗', 'commit/' + encodeURIComponent(node.relation.commit)));
    if (!node.relation.verified && node.id !== 'other') evidence.append(el('p', '', 'Kein vollständiger Komponentenbeleg in diesem Git-Stand.'));
    body.append(evidence);
  }
  const events = detailEvents(node);
  const scope = el('div', 'detail-scope');
  if (currentBundle) {
    scope.append(el('p', '', number(events.length) + (events.length === 1 ? ' Änderung' : ' Änderungen') + ' in diesem Meilenstein-Bündel. Chronologisch von früher nach später.'));
    scope.append(button('Ganze Funktion zeigen', 'more', () => {currentBundle = null; currentEvent = ''; detailPage = 0; paintDetail(); saveState();}));
  } else {
    scope.append(el('p', '', number(events.length) + ' Änderungen · ' + (detailFull ? 'vollständige Historie' : 'passend zu den Filtern') + (includeChildren ? ' inkl. Unterfunktionen' : ' direkt an dieser Funktion') + (currentMonth ? ' · ' + monthLabel(currentMonth) : '') + '. Chronologisch von früher nach später.'));
    scope.append(button(detailFull ? 'Gefilterte Historie' : 'Vollständige Historie', 'more', () => {detailFull = !detailFull; currentMonth = ''; detailPage = 0; paintDetail(); saveState();}));
    if (node.children.length) scope.append(button(includeChildren ? 'Nur direkte Änderungen' : 'Unterfunktionen einbeziehen', 'more', () => {includeChildren = !includeChildren; detailPage = 0; paintDetail(); saveState();}));
    if (currentMonth) scope.append(button('Alle Monate', 'more', () => {currentMonth = ''; detailPage = 0; paintDetail(); saveState();}));
  }
  body.append(scope);
  detailPage = Math.max(0, Math.min(detailPage, Math.ceil(events.length / 40) - 1));
  const history = el('div', 'history');
  for (const c of events.slice(detailPage * 40, (detailPage + 1) * 40)) history.append(historyEvent(c));
  if (!events.length) history.append(el('p', 'detail-intro', 'Keine Ereignisse in dieser Auswahl. Die vollständige Historie und Unterfunktionen können zusätzlich eingeblendet werden.'));
  body.append(history);
  if (events.length > 40) {
    const pagination = el('div', 'history-pagination');
    const changePage = delta => {detailPage += delta; paintDetail(); $('detail').scrollTop = 0; $('close-detail').focus({preventScroll: true});};
    const previous = button('Frühere Änderungen', 'more', () => changePage(-1)); previous.disabled = detailPage === 0;
    const next = button('Weitere Änderungen', 'more', () => changePage(1)); next.disabled = (detailPage + 1) * 40 >= events.length;
    pagination.append(previous, el('span', '', (detailPage + 1) + ' / ' + Math.ceil(events.length / 40)), next); body.append(pagination);
  }
  body.append(el('p', 'detail-intro', 'Git-Nachweise sind keine Einführungstermine oder Deploy-Belege. Automatische Ereigniszuordnungen können ungenau sein.'));
}
function openDetail(id, eventId = '', month = '', bundle = null) {
  const node = INDEX.get(id);
  if (!node) return;
  if (!$('detail').open) returnFocus = document.activeElement;
  currentFeature = id;
  currentBundle = Array.isArray(bundle) && bundle.length ? bundle.slice() : null;
  currentEvent = node.allCommits.some(c => c.id === eventId) ? eventId : '';
  currentMonth = /^\d{4}-\d{2}$/.test(month) ? month : '';
  includeChildren = true; detailFull = false;
  if (!currentBundle && currentEvent && !detailEvents(node).some(c => c.id === currentEvent)) {detailFull = true; currentMonth = '';}
  highlightBranch();
  detailPage = Math.max(0, Math.floor(detailEvents(node).findIndex(c => c.id === currentEvent) / 40));
  paintDetail();
  if (!$('detail').open) {
    if (mobileLayout.matches) {$('detail').showModal(); document.body.style.overflow = 'hidden';}
    else $('detail').show();
  }
  $('detail').scrollTop = 0;
  requestAnimationFrame(() => {reveal(selectedKey()); saveState();});
  saveState();
}
function closeDetail() {
  if (!$('detail').open) return;
  // The dialog's close event runs later; unlock scrolling in the same input
  // event so Escape and the close button leave no locked page behind.
  document.body.style.overflow = '';
  $('detail').close();
}
$('detail').addEventListener('close', () => {
  if (ignoreClose) {ignoreClose = false; return;}
  currentFeature = ''; currentEvent = ''; currentMonth = ''; currentBundle = null; detailFull = false;
  highlightBranch();
  document.body.style.overflow = ''; schedulePaint(); saveState();
  if (returnFocus?.isConnected) returnFocus.focus({preventScroll: true}); else $('focus-feature').focus({preventScroll: true});
});
$('detail').addEventListener('cancel', event => {event.preventDefault(); closeDetail();});
$('close-detail').addEventListener('click', closeDetail);
document.addEventListener('keydown', event => {if (event.key === 'Escape' && $('detail').open) {event.preventDefault(); closeDetail();}});
mobileLayout.addEventListener('change', () => {
  if ($('detail').open) {
    ignoreClose = true; $('detail').close();
    if (mobileLayout.matches) {$('detail').showModal(); document.body.style.overflow = 'hidden';}
    else {$('detail').show(); document.body.style.overflow = '';}
  }
  schedulePaint();
});
function renderList() {
  $('list').replaceChildren();
  const events = selectedCommits.slice().sort(compareDates).reverse();
  const days = new Map();
  for (const c of events.slice(0, listLimit)) {if (!days.has(c.date)) days.set(c.date, []); days.get(c.date).push(c);}
  for (const [date, commits] of days) {
    const day = el('section', 'list-day'); day.append(el('h3', 'list-date', dateLabel(date)));
    const entries = el('div', 'list-events');
    for (const c of commits) {
      const entry = el('article', 'event-line ' + c.kind); entry.append(el('span', 'badge', KINDS[c.kind]), el('h3', '', c.title));
      const links = el('div', 'event-links');
      for (const id of c.features) links.append(button(INDEX.get(id).title, 'feature-link', () => openDetail(id, c.id)));
      links.append(commitLink(c)); entry.append(links); entries.append(entry);
    }
    day.append(entries); $('list').append(day);
  }
  if (events.length > listLimit) $('list').append(button('Weitere Änderungen laden (' + number(events.length - listLimit) + ')', 'more', () => {listLimit += 80; renderList();}));
  if (!events.length) {const empty = el('div', 'empty'); empty.append(el('strong', '', 'Keine passenden Änderungen'), el('p', '', $('empty-message').textContent)); $('list').append(empty);}
}
function render() {
  const state = filters();
  selection = selectFamily(DATA, filterCommits(DATA, state), {focus: focusId, collapsed: [...collapsed]}, INDEX);
  selectedCommits = selection.commits;
  $('focus-feature').value = focusId;
  $('custom-dates').hidden = state.period !== 'custom';
  for (const node of document.querySelectorAll('[data-view]')) node.setAttribute('aria-pressed', String(node.dataset.view === view));
  $('result-count').textContent = number(selection.commitCount) + ' eindeutige Änderungen · ' + INDEX.get(focusId).title;
  $('graph-pane').hidden = view !== 'tree'; $('list').hidden = view !== 'list';
  for (const id of ['zoom-out', 'zoom-reset', 'zoom-in', 'fit']) $(id).disabled = view !== 'tree';
  renderGraph();
  if (view === 'list') renderList();
  if (currentFeature && $('detail').open) paintDetail();
  saveState();
}
function applyFilters() {
  listLimit = 80; detailPage = 0; collapsed.clear(); resetCamera(); render();
}
$('filters').addEventListener('submit', event => event.preventDefault());
$('search').addEventListener('input', () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {focusId = 'product'; applyFilters();}, 160);
});
for (const id of ['group', 'period', 'kind', 'maintenance', 'from', 'to']) $(id).addEventListener('change', applyFilters);
$('focus-feature').addEventListener('change', () => focusFeature($('focus-feature').value));
$('all-branches').addEventListener('click', () => focusFeature('product'));
$('reset-filters').addEventListener('click', () => {clearTimeout(searchTimer); $('filters').reset(); $('from').value = ''; $('to').value = ''; focusId = 'product'; applyFilters();});
for (const node of document.querySelectorAll('[data-view]')) node.addEventListener('click', () => {view = node.dataset.view; render();});
$('zoom-in').addEventListener('click', () => zoomTo(camera.zoom * 1.2));
$('zoom-out').addEventListener('click', () => zoomTo(camera.zoom / 1.2));
$('zoom-reset').addEventListener('click', () => {resetCamera(); paintWindow(); saveState();});
$('fit').addEventListener('click', () => {
  camera.zoom = Math.max(.08, Math.min(1, ($('viewport').clientWidth - 32) / graph.width, ($('viewport').clientHeight - 28) / graph.height));
  camera.x = 12; camera.y = 8; paintWindow(); saveState();
});
let drag = null;
$('viewport').addEventListener('pointerdown', event => {
  if (event.button !== 0 || event.target.closest('button,a,input,select')) return;
  drag = {id: event.pointerId, x: event.clientX, y: event.clientY, startX: camera.x, startY: camera.y};
  $('viewport').setPointerCapture(event.pointerId); $('viewport').classList.add('dragging');
  $('viewport').focus({preventScroll: true});
});
$('viewport').addEventListener('pointermove', event => {
  if (!drag || drag.id !== event.pointerId) return;
  camera.x = drag.startX + event.clientX - drag.x; camera.y = drag.startY + event.clientY - drag.y; schedulePaint();
});
function finishDrag() {drag = null; $('viewport').classList.remove('dragging'); saveState();}
$('viewport').addEventListener('pointerup', finishDrag);
$('viewport').addEventListener('pointercancel', finishDrag);
$('viewport').addEventListener('wheel', event => {
  event.preventDefault();
  if (event.ctrlKey || event.metaKey) {
    const rect = $('viewport').getBoundingClientRect();
    zoomTo(camera.zoom * (event.deltaY < 0 ? 1.1 : 1 / 1.1), event.clientX - rect.left, event.clientY - rect.top);
  } else {
    camera.x -= event.shiftKey ? event.deltaY : event.deltaX;
    camera.y -= event.shiftKey ? 0 : event.deltaY;
    schedulePaint(); clearTimeout(cameraTimer); cameraTimer = setTimeout(saveState, 120);
  }
}, {passive: false});
$('viewport').addEventListener('keydown', event => {
  if (event.target !== $('viewport')) return;
  const movement = {ArrowLeft: [80, 0], ArrowRight: [-80, 0], ArrowUp: [0, 80], ArrowDown: [0, -80]}[event.key];
  if (movement) {event.preventDefault(); camera.x += movement[0]; camera.y += movement[1]; paintWindow(); saveState();}
  else if (['+', '=', '-'].includes(event.key)) {event.preventDefault(); zoomTo(camera.zoom * (event.key === '-' ? 1 / 1.2 : 1.2));}
  else if (event.key === 'Home') {event.preventDefault(); resetCamera(); paintWindow(); saveState();}
});
new ResizeObserver(schedulePaint).observe($('viewport'));
window.addEventListener('popstate', () => location.reload());
summary(); render();
if (initial.has('feature')) {
  if (INDEX.has(initial.get('feature'))) {
    openDetail(initial.get('feature'), initial.get('event') || '', initial.get('month') || '');
    detailFull = detailFull || initial.get('full') === '1'; includeChildren = initial.get('children') !== '0';
    detailPage = Math.max(0, Math.floor(detailEvents(INDEX.get(currentFeature)).findIndex(c => c.id === currentEvent) / 40));
    paintDetail(); saveState();
  } else $('notices').append(el('p', 'notice', 'Das verlinkte Feature ist in diesem Datenstand nicht enthalten.'));
}

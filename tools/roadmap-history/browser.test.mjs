/* Chromium acceptance against the real generated history. Loopback only; always closed. */
import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {createServer} from 'node:http';
import {createRequire} from 'node:module';
import {resolve} from 'node:path';
const require = createRequire(new URL('../../.roadmap-test-runtime/package.json', import.meta.url));
const {chromium} = require('playwright');
const root = resolve(new URL('../..', import.meta.url).pathname);
const output = resolve(root, 'dist/roadmap-history');
const html = await readFile(resolve(output, 'index.html'), 'utf8');
const dataPattern = /(<script id="roadmap-data" type="application\/json">)([\s\S]*?)(<\/script>)/;
const sourceData = JSON.parse(html.match(dataPattern)[2]);
const hostile = structuredClone(sourceData);
const attack = '<img src=x onerror="window.roadmapInjection=true">';
hostile.features.find(f => f.id === 'uplink').title = attack.repeat(15);
hostile.features.find(f => f.id === 'uplink').description = 'Sehr langer Beschreibungstext '.repeat(100);
hostile.commits.find(c => c.features.includes('uplink-av1')).title = attack.repeat(30);
const hostileJSON = JSON.stringify(hostile).replaceAll('<', '\\u003c').replaceAll('&', '\\u0026');
const hostileHTML = html.replace(dataPattern, (_, open, data, close) => open + hostileJSON + close);
const server = createServer((request, response) => {
  response.writeHead(200, {'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store'});
  response.end(request.url.startsWith('/hostile') ? hostileHTML : html);
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const url = 'http://127.0.0.1:' + server.address().port + '/';
let browser;
const checks = [], errors = [], external = [];
const pause = page => page.waitForTimeout(240);
const screenshot = async (page, name, fullPage = true) => {
  // Capture the settled UI, not transient native touch highlights or a scrolled
  // fixed skip link composited into a full-page screenshot.
  if (fullPage) await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(400);
  await page.screenshot({path: resolve(output, name + '.png'), fullPage, animations: 'disabled'});
};
function watch(page) {
  page.on('pageerror', error => errors.push(String(error)));
  page.on('request', request => {if (!request.url().startsWith(url.slice(0, -1))) external.push(request.url());});
}
try {
  browser = await chromium.launch({headless: true});
  const page = await browser.newPage({viewport: {width: 1440, height: 1120}}); watch(page);
  await page.goto(url);
  await page.locator('.branch-label').first().waitFor();
  assert.equal(await page.locator('[data-view=tree]').getAttribute('aria-pressed'), 'true');
  assert.equal(await page.locator('#zoom-reset').textContent(), '100 %');
  assert.equal(await page.locator('.graph-node,.event-node,.node-main').count(), 0);
  assert.ok(sourceData.commits.length > 1000);
  assert.equal(sourceData.schemaVersion, 2);
  assert.ok(await page.locator('.trunk-line').count() === 1);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), 'rgb(13, 13, 13)');
  assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--gold').trim()), '#C5A059');
  await screenshot(page, 'family-desktop-1440');
  checks.push('1440px: maßstäbliche Zeitachse mit Stamm als Hauptansicht, Dashboard-Farben, lesbarer Start bei 100 Prozent, keine Seitenüberbreite');

  const chronologicalIds = sourceData.commits.slice().sort((a, b) => Date.parse(a.timestamp) - Date.parse(b.timestamp) || a.id.localeCompare(b.id)).map(c => c.id);
  assert.deepEqual(sourceData.commits.map(c => c.id), chronologicalIds);
  assert.deepEqual(await page.evaluate(() => INDEX.get('product').allCommits.map(c => c.id)), chronologicalIds);
  checks.push('Echte Git-Zeitstempel: Generator und Browser sortieren denselben Datenbestand nach Zeitpunkten statt nach Zeitzonen-Text');

  await page.goto(url + '?focus=product');
  await page.locator('.milestone').first().waitFor();
  const layout = await page.evaluate(() => {
    let labelOverlaps = 0, badForks = 0;
    const labels = graph.nodes.filter(n => n.type === 'label');
    for (let i = 0; i < labels.length; i++) for (let j = i + 1; j < labels.length; j++) {
      const a = labels[i], b = labels[j];
      if (a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y) labelOverlaps++;
    }
    const branchY = new Map(graph.branches.map(b => [b.key, b.y]));
    for (const fork of graph.forks) {
      const expected = fork.source === 'trunk' ? graph.trunk.y : branchY.get(fork.source);
      if (Math.abs(fork.y1 - expected) > 0.5 || Math.abs(fork.y2 - branchY.get(fork.key)) > 0.5) badForks++;
      const parent = graph.branches.find(b => b.key === fork.source);
      const start = document.querySelector('.fork-edge[data-target="' + fork.key + '"]').getPointAtLength(0);
      if (start.x < (parent?.forkX ?? graph.trunk.x0) - 0.5 || start.x > (parent?.endX ?? graph.trunk.x1) + 0.5) badForks++;
    }
    const total = graph.milestones.reduce((sum, m) => sum + m.commitIds.length, 0);
    return {labelOverlaps, badForks, forks: graph.forks.length, branches: graph.branches.length, milestones: graph.milestones.length, milestoneCommits: total, depth: INDEX.get('uplink-av1').depth, months: graph.months, weeks: graph.weeks.length, drawnOther: graph.branches.some(b => b.id === 'other'), height: graph.height};
  });
  assert.equal(layout.labelOverlaps, 0);
  assert.equal(layout.badForks, 0);
  assert.equal(layout.depth, 3);
  assert.ok(layout.branches > 20);
  assert.ok(layout.milestones > 100 && layout.milestoneCommits >= layout.milestones);
  assert.equal(layout.drawnOther, false);
  assert.ok(layout.weeks > 8 && layout.months.length >= 6);
  assert.equal(await page.evaluate(() => document.querySelectorAll('#edges .branch-line[data-feature]').length), layout.branches);
  console.log('REAL_DATA_LAYOUT', JSON.stringify(layout));
  checks.push('Alle Äste sitzen auf einer echten Zeitachse; Astwurzeln treffen die Elternbahn, „other“ wird nicht als Ast gezeichnet, Labels überdecken sich nicht');

  await page.setViewportSize({width: 1920, height: 1120});
  await page.locator('#fit').click();
  assert.ok(await page.evaluate(() => camera.zoom <= 1));
  await screenshot(page, 'family-all-fit');
  checks.push('„Alle Zweige“ passt sich über „Ansicht einpassen“ ohne vertikales Dauer-Scrollen auf 1920 ein');

  await page.goto(url + '?focus=uplink');
  await page.getByRole('button', {name: 'Uplink & Encoding: Unterzweige zuklappen', exact: true}).click();
  assert.equal(await page.evaluate(() => selection.nodes.some(n => n.id === 'uplink-av1')), false);
  await page.getByRole('button', {name: 'Uplink & Encoding: Unterzweige aufklappen', exact: true}).click();
  assert.equal(await page.evaluate(() => selection.nodes.some(n => n.id === 'uplink-av1')), true);
  await page.locator('#focus-feature').selectOption('rank-steam');
  assert.equal(new URL(page.url()).searchParams.get('focus'), 'rank-steam');
  assert.deepEqual(await page.evaluate(() => selection.nodes.map(n => n.id)), ['product', 'chat', 'rank', 'rank-steam']);
  checks.push('Unterzweige auf-/zuklappen und vollständigen Teilbaum mit Kontext-Eltern fokussieren');

  const steam = sourceData.commits.find(c => c.features.includes('rank-steam'));
  assert.ok(steam, 'Main enthält den tatsächlichen Steam-Verknüpfungs-Commit');
  await page.locator('#search').fill(steam.id); await pause(page);
  await page.locator('#period').selectOption('custom');
  await page.locator('#from').fill(steam.date); await page.locator('#from').dispatchEvent('change');
  await page.locator('#to').fill(steam.date); await page.locator('#to').dispatchEvent('change');
  assert.equal(await page.evaluate(() => selection.commitCount), 1);
  assert.equal(await page.evaluate(() => selection.nodes.find(n => n.id === 'chat').context), true);
  const contextCaret = page.locator('.branch-label[data-feature="chat"] .branch-caret');
  await contextCaret.click();
  assert.equal(await contextCaret.getAttribute('aria-expanded'), 'false');
  assert.equal(await page.locator('.branch-label[data-feature="rank-steam"]').count(), 0);
  await contextCaret.click();
  assert.equal(await contextCaret.getAttribute('aria-expanded'), 'true');
  assert.equal(await page.locator('.branch-label[data-feature="rank-steam"]').count(), 1);
  checks.push('Kontext-Eltern behalten beim Zuklappen ihren sichtbaren Aufklappknopf und stellen den gefilterten Unterbaum wieder her');

  assert.ok(await page.locator('.branch-label[data-feature="rank-steam"]').count());
  await page.locator('.branch-label[data-feature="rank-steam"] .branch-title').click();
  assert.equal(await page.locator('#detail').evaluate(d => d.open && !d.matches(':modal')), true);
  assert.ok((await page.locator('#viewport').boundingBox()).width > 500);
  await page.locator('#zoom-in').click();
  assert.equal(await page.locator('#zoom-reset').textContent(), '120 %');
  await page.locator('#zoom-reset').click();
  assert.equal(await page.locator('#detail-title').textContent(), 'Steam-Verknüpfung');
  await page.locator('.detail-relations').getByRole('button', {name: '← Rang & Spielerabfragen', exact: true}).click();
  assert.equal(await page.locator('#detail-title').textContent(), 'Rang & Spielerabfragen');
  await page.locator('.detail-relations').getByRole('button', {name: 'Steam-Verknüpfung', exact: true}).click();
  await page.locator('.evidence summary').click();
  assert.ok((await page.locator('.evidence').textContent()).includes('Redaktionelle'));
  assert.ok(await page.locator('.evidence a').count() > 0);
  checks.push('Suche und Datum erhalten Kontext-Eltern; rechte Desktop-Details bleiben nichtmodal, Graph bedienbar, Eltern/Kinder und Quellen klickbar');
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#detail').evaluate(d => d.open), false);

  await page.goto(url + '?focus=uplink-av1');
  await page.locator('.milestone').first().waitFor();
  await page.locator('.milestone').first().click();
  assert.ok(await page.locator('.selected-event').count() >= 1);
  const selected = new URL(page.url()).searchParams.get('event');
  assert.equal(await page.locator('.selected-event').getAttribute('data-commit'), selected);
  assert.ok(await page.locator('.selected-event .commit-link').getAttribute('href').then(h => h.includes(selected)));
  assert.ok((await page.locator('.detail-scope').textContent()).includes('Meilenstein-Bündel'));
  await page.locator('.selected-event summary').click();
  assert.ok((await page.locator('.selected-event').textContent()).includes('Originaler Commit-Titel'));
  await page.getByRole('button', {name: 'Ganze Funktion zeigen', exact: true}).click();
  assert.ok(!(await page.locator('.detail-scope').textContent()).includes('Meilenstein-Bündel'));
  const direct = url + '?focus=uplink-av1&feature=uplink-av1&event=' + selected;
  await page.goto(direct);
  assert.equal(await page.locator('#detail').evaluate(d => d.open), true);
  assert.equal(await page.locator('.selected-event').getAttribute('data-commit'), selected);
  assert.equal(new URL(page.url()).searchParams.get('focus'), 'uplink-av1');
  checks.push('Meilenstein öffnet das vollständige Bündel, markiert die konkrete Änderung, blendet auf die ganze Funktion zurück und stellt den Direktlink samt Auswahl wieder her');

  await page.goto(url + '?focus=product&feature=product&full=1');
  assert.equal(await page.locator('.history-event').count(), 40);
  const firstPage = await page.locator('.history-event').evaluateAll(nodes => nodes.map(n => n.dataset.commit));
  await page.getByRole('button', {name: 'Weitere Änderungen', exact: true}).click();
  assert.equal(await page.locator('.history-event').count(), 40);
  const secondPage = await page.locator('.history-event').evaluateAll(nodes => nodes.map(n => n.dataset.commit));
  assert.ok(secondPage.every(id => !firstPage.includes(id)));
  await page.getByRole('button', {name: 'Frühere Änderungen', exact: true}).click();
  assert.deepEqual(await page.locator('.history-event').evaluateAll(nodes => nodes.map(n => n.dataset.commit)), firstPage);
  await page.keyboard.press('Escape');
  checks.push('Lange vollständige Historie ist vorwärts und rückwärts paginiert, keine doppelten Ereignisse');

  await page.getByRole('button', {name: 'Änderungsliste', exact: true}).click();
  assert.equal(await page.locator('.event-line').count(), 80);
  await page.getByRole('button', {name: /Weitere Änderungen laden/}).click();
  assert.equal(await page.locator('.event-line').count(), 160);
  await page.locator('#kind').selectOption('maintenance');
  assert.ok(await page.locator('.event-line').count() > 0);
  assert.ok(await page.evaluate(() => selectedCommits.every(c => MAINTENANCE.has(c.kind))));
  await page.locator('#search').fill('no-such-feature-abc-123'); await pause(page);
  assert.ok(await page.locator('#list').getByText('Keine passenden Änderungen', {exact: true}).isVisible());
  await page.locator('#reset-filters').click();
  await page.getByRole('button', {name: 'Feature-Stammbaum', exact: true}).click();
  await page.locator('#period').selectOption('custom');
  await page.locator('#from').fill(boundsDate(sourceData, 'last'));
  await page.locator('#to').fill(boundsDate(sourceData, 'first')); await page.locator('#to').dispatchEvent('change');
  assert.ok(await page.getByText(/Das Startdatum liegt nach/).isVisible());
  checks.push('Ergänzende Änderungsliste, Pagination, Technik-Filter, leere Suche und ungültige Datumsgrenzen');

  await page.goto(url + '?focus=product&collapsed=uplink');
  assert.ok(await page.evaluate(() => collapsed.has('uplink')));
  const viewport = page.locator('#viewport'); await viewport.focus();
  await page.keyboard.press('ArrowDown');
  assert.ok(await page.evaluate(() => camera.y < 0));
  const box = await viewport.boundingBox();
  const before = await page.evaluate(() => camera.x);
  await page.mouse.move(box.x + box.width / 2, box.y + 10); await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 - 150, box.y + 10, {steps: 5}); await page.mouse.up(); await pause(page);
  assert.ok(await page.evaluate(() => camera.x) < before);
  await page.locator('#fit').click(); assert.ok(await page.evaluate(() => camera.zoom < 1));
  assert.ok(await page.evaluate(() => document.querySelectorAll('#nodes > *').length <= graph.nodes.length));
  await page.locator('#zoom-reset').click(); assert.equal(await page.locator('#zoom-reset').textContent(), '100 %');
  assert.ok(await page.evaluate(() => document.querySelectorAll('#nodes > *').length < graph.nodes.length));
  for (const selector of ['.branch-label .branch-title', '.milestone']) {
    const focused = page.locator(selector).first();
    await focused.focus();
    const focusResult = await page.evaluate(() => {
      const active = document.activeElement;
      const previous = {...camera};
      camera.x -= graph.width + 5000;
      paintWindow();
      const result = active.isConnected && document.activeElement === active;
      Object.assign(camera, previous); paintWindow();
      return result;
    });
    assert.equal(focusResult, true, 'Virtualisierung muss das fokussierte Element behalten: ' + selector);
  }
  const cameraURL = page.url(); await page.goto(cameraURL);
  assert.ok(await page.evaluate(() => collapsed.has('uplink')));
  checks.push('Mausziehen, Pfeiltasten, Zoom, Einpassen, lesbarer Reset, virtuelle Knoten und wiederhergestellter Klappzustand');

  await page.goto(url + 'hostile?focus=uplink-av1');
  await page.locator('.branch-label').first().waitFor();
  assert.equal(await page.evaluate(() => window.roadmapInjection), undefined);
  assert.equal(await page.locator('#nodes img').count(), 0);
  assert.ok((await page.locator('.branch-label[data-feature=uplink] .branch-name').textContent()).includes('<img'));
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await screenshot(page, 'family-hostile-long-text');
  checks.push('HTML-Angriffe bleiben sichtbarer Text; sehr lange Titel/Beschreibungen erzeugen keine Überbreite');

  await page.setViewportSize({width: 1920, height: 1220});
  await page.goto(url + '?focus=uplink');
  await page.locator('.branch-label').first().waitFor();
  await screenshot(page, 'family-desktop');
  await page.locator('#focus-feature').selectOption('uplink-av1');
  await page.locator('.milestone').first().click();
  await screenshot(page, 'family-detail');
  checks.push('Echte Desktop-Screenshots mit Zeitachse und ausgewähltem Meilenstein-Bündel erstellt');

  const mobile = await browser.newPage({viewport: {width: 390, height: 844}, isMobile: true, hasTouch: true}); watch(mobile);
  await mobile.goto(url + '?focus=product');
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  for (const id of ['group', 'period', 'kind']) {
    assert.ok((await mobile.locator('#' + id).boundingBox()).width >= 150, 'Mobile Filter benötigen lesbare Breite: ' + id);
  }
  assert.equal(await mobile.locator('#kind').evaluate(node => getComputedStyle(node).webkitTapHighlightColor), 'rgba(197, 160, 89, 0.18)');
  await mobile.locator('.branch-title').first().tap();
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open && d.matches(':modal')), true);
  await mobile.keyboard.press('Tab');
  assert.equal(await mobile.locator('#detail').evaluate(d => d.contains(document.activeElement)), true);
  await screenshot(mobile, 'family-mobile-detail', false);
  await mobile.locator('#close-detail').tap();
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open), false);
  await mobile.locator('#zoom-out').tap();
  assert.equal(await mobile.locator('#zoom-reset').textContent(), '83 %');
  await mobile.locator('#focus-feature').selectOption('rank-steam');
  await mobile.locator('.branch-label[data-feature=rank-steam] .branch-title').tap();
  await mobile.keyboard.press('Escape');
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open), false);
  assert.equal(await mobile.evaluate(() => document.body.style.overflow), '');
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await screenshot(mobile, 'family-mobile', true);
  checks.push('390px: Touch, Zoom, Teilbaumfokus, modale Details, Tastaturfokus, Schließen und Escape ohne Seitenüberbreite');
  assert.deepEqual(errors, []); assert.deepEqual(external, []);
  checks.push('Keine JavaScript-Laufzeitfehler und keine externen Font-/Diagramm-/CDN-Anfragen');
  const report = {passed: checks.length, checks, revision: sourceData.revision, commits: sourceData.commits.length, features: sourceData.features.length, layout, screenshots: output};
  await writeFile(resolve(output, 'browser-report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally {
  if (browser) await browser.close();
  await new Promise(resolve => server.close(resolve));
}
function boundsDate(data, boundary) {
  const values = data.commits.map(c => c.date).sort();
  return boundary === 'first' ? values[0] : values.at(-1);
}

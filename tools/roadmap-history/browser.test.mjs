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
  await page.screenshot({path: resolve(output, name + '.png'), fullPage});
};
function watch(page) {
  page.on('pageerror', error => errors.push(String(error)));
  page.on('request', request => {if (!request.url().startsWith(url.slice(0, -1))) external.push(request.url());});
}
try {
  browser = await chromium.launch({headless: true});
  const page = await browser.newPage({viewport: {width: 1440, height: 1120}}); watch(page);
  await page.goto(url);
  await page.locator('.graph-node').first().waitFor();
  assert.equal(await page.locator('[data-view=tree]').getAttribute('aria-pressed'), 'true');
  assert.equal(await page.locator('#zoom-reset').textContent(), '100 %');
  assert.equal(await page.locator('.feature-card,.family,.mini-history').count(), 0);
  assert.ok(sourceData.commits.length > 1000);
  assert.equal(sourceData.schemaVersion, 2);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.equal(await page.evaluate(() => getComputedStyle(document.body).backgroundColor), 'rgb(13, 13, 13)');
  assert.equal(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--gold').trim()), '#C5A059');
  await screenshot(page, 'family-desktop-1440');
  checks.push('1440px: echter Graph als Hauptansicht, Dashboard-Farben, lesbarer Start bei 100 Prozent, keine Seitenüberbreite');

  const geometry = await page.evaluate(() => {
    let intersections = 0, badEndpoints = 0, checkedEndpoints = 0;
    for (let i = 0; i < graph.nodes.length; i++) for (let j = i + 1; j < graph.nodes.length; j++) {
      const a = graph.nodes[i], b = graph.nodes[j];
      if (a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y) intersections++;
    }
    for (const path of document.querySelectorAll('.family-edge')) {
      const parent = nodeElements.get(path.dataset.source);
      if (!parent) continue;
      const point = path.getPointAtLength(0), matrix = path.getScreenCTM();
      const start = new DOMPoint(point.x, point.y).matrixTransform(matrix);
      const rect = parent.getBoundingClientRect();
      if (Math.abs(start.x - rect.right) > 2 || Math.abs(start.y - (rect.top + rect.height / 2)) > 2) badEndpoints++;
      checkedEndpoints++;
    }
    return {intersections, badEndpoints, checkedEndpoints, depth: INDEX.get('uplink-av1').depth, nodes: graph.nodes.length, months: graph.months, functions: selection.nodes.map(f => ({id: f.id, first: f.first, direct: f.commits.length}))};
  });
  assert.equal(geometry.intersections, 0); assert.equal(geometry.badEndpoints, 0); assert.ok(geometry.checkedEndpoints >= 3); assert.equal(geometry.depth, 3);
  console.log('REAL_DATA_LAYOUT', JSON.stringify(geometry));
  checks.push('Echte Mehrgenerationen, keine Knotenüberlappungen; SVG-Verbindungen beginnen am sichtbaren Elternknoten');

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
  assert.ok(await page.locator('.graph-node[data-feature="rank-steam"]').count());
  await page.locator('.graph-node[data-feature="rank-steam"] .node-main').click();
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
  await page.locator('.event-node').first().waitFor();
  await page.locator('.event-node').first().click();
  assert.ok(await page.locator('.selected-event').count() === 1);
  const selected = new URL(page.url()).searchParams.get('event');
  assert.equal(await page.locator('.selected-event').getAttribute('data-commit'), selected);
  assert.ok(await page.locator('.selected-event .commit-link').getAttribute('href').then(h => h.includes(selected)));
  await page.locator('.selected-event summary').click();
  assert.ok((await page.locator('.selected-event').textContent()).includes('Originaler Commit-Titel'));
  const direct = page.url();
  await page.goto(direct);
  assert.equal(await page.locator('#detail').evaluate(d => d.open), true);
  assert.equal(await page.locator('.selected-event').getAttribute('data-commit'), selected);
  assert.equal(new URL(page.url()).searchParams.get('focus'), 'uplink-av1');
  checks.push('Datierte Monatsstation öffnet vollständige Einzelereignisse, markiert konkrete Änderung und stellt Direktlink samt Auswahl wieder her');

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
  assert.ok(await page.locator('.graph-node').count() < 700);
  await page.locator('#zoom-reset').click(); assert.equal(await page.locator('#zoom-reset').textContent(), '100 %');
  assert.ok(await page.locator('.graph-node').count() < 100);
  const cameraURL = page.url(); await page.goto(cameraURL);
  assert.ok(await page.evaluate(() => collapsed.has('uplink')));
  checks.push('Mausziehen, Pfeiltasten, Zoom, Einpassen, lesbarer Reset, virtuelle Knoten und wiederhergestellter Klappzustand');

  await page.goto(url + 'hostile?focus=uplink-av1');
  assert.equal(await page.evaluate(() => window.roadmapInjection), undefined);
  assert.equal(await page.locator('#nodes img').count(), 0);
  assert.ok((await page.locator('[data-feature=uplink] strong').textContent()).includes('<img'));
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.ok(await page.evaluate(() => [...document.querySelectorAll('.graph-node')].every(n => n.getBoundingClientRect().width <= 240)));
  await screenshot(page, 'family-hostile-long-text');
  checks.push('HTML-Angriffe bleiben sichtbarer Text; sehr lange Titel/Beschreibungen erzeugen keine Überbreite');

  await page.setViewportSize({width: 1920, height: 1220});
  await page.goto(url + '?focus=uplink');
  await page.locator('.graph-node').first().waitFor();
  await screenshot(page, 'family-desktop');
  await page.locator('#focus-feature').selectOption('uplink-av1');
  await page.locator('.event-node').first().click();
  await screenshot(page, 'family-detail');
  checks.push('Echte Desktop-Screenshots mit Graph und ausgewählter Ereignis-Detailleiste erstellt');

  const mobile = await browser.newPage({viewport: {width: 390, height: 844}, isMobile: true, hasTouch: true}); watch(mobile);
  await mobile.goto(url + '?focus=product');
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await mobile.locator('[data-feature=product] .node-main').tap();
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open && d.matches(':modal')), true);
  await mobile.keyboard.press('Tab');
  assert.equal(await mobile.locator('#detail').evaluate(d => d.contains(document.activeElement)), true);
  await screenshot(mobile, 'family-mobile-detail', false);
  await mobile.locator('#close-detail').tap();
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open), false);
  assert.equal(await mobile.evaluate(() => document.activeElement.classList.contains('node-main')), true);
  await mobile.locator('#zoom-out').tap();
  assert.equal(await mobile.locator('#zoom-reset').textContent(), '83 %');
  await mobile.locator('#focus-feature').selectOption('rank-steam');
  await mobile.locator('[data-feature=rank-steam] .node-main').tap();
  await mobile.keyboard.press('Escape');
  assert.equal(await mobile.locator('#detail').evaluate(d => d.open), false);
  assert.equal(await mobile.evaluate(() => document.body.style.overflow), '');
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await screenshot(mobile, 'family-mobile', true);
  checks.push('390px: Touch, Zoom, Teilbaumfokus, modale Details, Tastaturfokus, Schließen und Escape ohne Seitenüberbreite');
  assert.deepEqual(errors, []); assert.deepEqual(external, []);
  checks.push('Keine JavaScript-Laufzeitfehler und keine externen Font-/Diagramm-/CDN-Anfragen');
  const report = {passed: checks.length, checks, revision: sourceData.revision, commits: sourceData.commits.length, features: sourceData.features.length, geometry, screenshots: output};
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

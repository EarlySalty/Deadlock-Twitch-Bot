/* Acceptance against the complete generated Git history; loopback only. */
import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {createServer} from 'node:http';
import {createRequire} from 'node:module';
import {resolve} from 'node:path';
const require = createRequire(new URL('../../bot/dashboard_v2/package.json', import.meta.url));
const {chromium} = require('playwright-core');
const root = resolve(new URL('../..', import.meta.url).pathname);
const output = resolve(root, 'dist/roadmap-history');
const html = await readFile(resolve(output, 'index.html'), 'utf8');
const pattern = /(<script id="roadmap-data" type="application\/json">)([\s\S]*?)(<\/script>)/;
const original = JSON.parse(html.match(pattern)[2]);
assert.equal(original.schemaVersion, 3);
assert.equal(original.rootId, 'genesis');
assert.ok(original.commits.length > 4000);
assert.ok(original.nodes.length > 800);
const rootNode = original.nodes.find(n => n.id === 'genesis');
assert.equal(rootNode.commitHash, '3654f6c73be53fc569da673fb307e7e0c79f2b87');
assert.equal(rootNode.date, '2025-09-21');
assert.equal(rootNode.title, 'Twitch Bot Genesis / Core Init');
const index = new Map(original.nodes.map(n => [n.id, n]));
for (const node of original.nodes) {
  const seen = new Set(); let current = node;
  while (current.parentId) {assert.ok(!seen.has(current.id)); seen.add(current.id); current=index.get(current.parentId); assert.ok(current);}
  assert.equal(current.id, 'genesis');
}
const attack = '<img src=x onerror="window.roadmapInjection=true">';
const hostile = structuredClone(original);
hostile.nodes.find(n => n.role === 'feature').title = attack;
hostile.nodes.find(n => n.role === 'feature').description = attack;
hostile.commits.find(c => c.id !== rootNode.commitHash).title = attack;
const hostileHtml = html.replace(pattern, (_,a,_b,c) => a + JSON.stringify(hostile).replaceAll('<','\\u003c').replaceAll('&','\\u0026') + c);
const server = createServer((request,response) => {response.writeHead(200,{'Content-Type':'text/html; charset=utf-8','Cache-Control':'no-store'}); response.end(request.url.startsWith('/hostile') ? hostileHtml : html);});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
const base = 'http://127.0.0.1:'+server.address().port;
let browser; const checks=[]; const errors=[]; const external=[];
function watch(page) {page.on('pageerror',error=>errors.push(String(error)));page.on('request',request=>{if(!request.url().startsWith(base)) external.push(request.url());});}
async function screenshot(page,name) {await page.screenshot({path:resolve(output,name+'.png'),fullPage:true,animations:'disabled'});}
try {
  browser = await chromium.launch({headless:true});
  const page = await browser.newPage({viewport:{width:1440,height:1000},reducedMotion:'reduce'}); watch(page);
  await page.goto(base); await page.locator('.node').first().waitFor();
  assert.equal(await page.locator('.node.root').count(),1);
  assert.equal(await page.locator('.node.root .label').textContent(),'Twitch-Bot: erster belegter Stand');
  assert.equal(await page.locator('[data-view=overview]').getAttribute('aria-pressed'),'true');
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  const overview = await page.locator('.node').count(); assert.ok(overview<original.nodes.length/4);
  await screenshot(page,'feature-tree-desktop'); checks.push('Desktop: echter Ursprung, reduzierte Übersicht, keine Seitenüberbreite');
  await page.locator('[data-view=features]').click();
  assert.ok(await page.locator('.node.feature').count()>=original.stats.features);
  await page.locator('[data-view=all]').click();
  assert.ok(await page.locator('.node').count()>700);
  const geometry = await page.evaluate(() => {
    const positions=[...layout.positions.values()], seen=new Set(); let overlap=0,badEdge=0;
    for(const p of positions) {for(const other of positions) {if(p===other||seen.has(other.node.id)) continue; if(p.x<other.x+other.width&&p.x+p.width>other.x&&p.y<other.y+other.height&&p.y+p.height>other.y) overlap++;} seen.add(p.node.id);}
    for(const path of document.querySelectorAll('#edges path')) {const a=layout.positions.get(path.dataset.parent),b=layout.positions.get(path.dataset.child); const start=path.getPointAtLength(0),end=path.getPointAtLength(path.getTotalLength()); if(!a||!b||!Number.isFinite(start.x)||!Number.isFinite(end.x)) badEdge++;}
    const sameDate=new Map(); for(const p of positions) {const existing=sameDate.get(p.node.date); if(existing!==undefined&&Math.abs(existing-p.x)>.01) return {overlap,badEdge,sameDate:false}; sameDate.set(p.node.date,p.x);} return {overlap,badEdge,sameDate:true};
  });
  assert.deepEqual(geometry,{overlap:0,badEdge:0,sameDate:true}); checks.push('Alle Änderungen: Knoten ohne Kollisionen, echte Elternkanten, dasselbe Datum auf derselben X-Position');
  const bundle=original.nodes.find(n=>n.role==='event'&&n.commitIds.length>1&&!n.maintenance&&n.commitIds.some(id=>original.commits.find(c=>c.id===id)?.title?.length>18));
  assert.ok(bundle); const tail=original.commits.find(c=>c.id===bundle.commitIds.at(-1));
  await page.locator('#search').fill(tail.id); assert.ok(await page.locator('.node[data-id="'+bundle.id+'"]').count()===1); checks.push('Suche findet auch nicht führende Commits eines Bündels');
  await page.locator('#reset').click(); await page.locator('[data-view=all]').click();
  const spanning=original.nodes.find(n=>n.role==='event'&&n.spanEnd>n.date&&!n.maintenance);
  assert.ok(spanning); await page.locator('#from').fill(spanning.spanEnd); await page.locator('#from').dispatchEvent('change');
  await page.locator('#to').fill(spanning.spanEnd); await page.locator('#to').dispatchEvent('change');
  assert.equal(await page.locator('.node[data-id="'+spanning.id+'"]').count(),1); checks.push('Datumsfilter berücksichtigt überlappende Bündelintervalle');
  await page.locator('#reset').click(); await page.locator('.node.root .node-open').click();
  assert.equal(await page.locator('#detail').evaluate(d=>d.open),true);
  assert.ok(await page.locator('#detail-body .commit-list li').count()<=30);
  await page.keyboard.press('Escape'); assert.equal(await page.locator('#detail').evaluate(d=>d.open),false);
  assert.equal(await page.evaluate(()=>document.body.style.overflow),''); checks.push('Dialog: paginierter Verlauf, Escape und Scroll-Freigabe');
  await page.goto(base+'/?focus=owner-does-not-exist'); assert.ok(await page.locator('.node.root').count()===1);
  const feature=original.nodes.find(n=>n.role==='feature'); await page.goto(base+'/?feature='+encodeURIComponent(feature.featureId));
  assert.equal(await page.locator('[data-view=features]').getAttribute('aria-pressed'),'true'); checks.push('Direktlink auf bestehendes Feature');
  await page.locator('.node[data-id="'+feature.id+'"] .node-open').click();
  await page.locator('#detail-body .more').filter({hasText:'Diesen Zweig ansehen'}).click();
  assert.ok(new URL(page.url()).searchParams.get('focus')===feature.featureId);
  assert.ok(await page.locator('.node').count()<original.nodes.length); checks.push('Zweigfokus behält Elternkontext und blendet fremde Äste aus');
  const mobile = await browser.newPage({viewport:{width:390,height:844},isMobile:true,hasTouch:true}); watch(mobile);
  await mobile.goto(base); await mobile.locator('.node').first().waitFor();
  assert.equal(await mobile.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  await screenshot(mobile,'feature-tree-mobile');
  await mobile.locator('#fit').tap(); await mobile.locator('#origin').tap(); await mobile.locator('.node.root .node-open').tap(); assert.equal(await mobile.locator('#detail').evaluate(d=>d.open),true);
  await mobile.locator('#detail-close').tap(); assert.equal(await mobile.evaluate(()=>document.body.style.overflow),'');
  checks.push('Mobil: lesbarer Einstieg, keine Seitenüberbreite, Knoten und Dialog per Touch nutzbar');
  const evil = await browser.newPage({viewport:{width:1200,height:800}}); watch(evil);
  await evil.goto(base+'/hostile'); await evil.locator('.node').first().waitFor();
  await evil.locator('[data-view=features]').click(); await evil.locator('.node.feature .node-open').first().click();
  assert.equal(await evil.evaluate(()=>window.roadmapInjection),undefined);
  assert.ok(await evil.locator('#detail-title').textContent());
  assert.equal(await evil.locator('img[src=x]').count(),0); checks.push('Git-Titel/Beschreibung bleiben Text; kein Script- oder HTML-Einbruch');
  assert.deepEqual(errors,[]); assert.deepEqual(external,[]);
  await writeFile(resolve(output,'browser-report.json'),JSON.stringify({revision:original.revision,rendererRevision:original.rendererRevision,commits:original.commits.length,nodes:original.nodes.length,checks},null,2));
  console.log(checks.join('\n'));
} finally {await browser?.close(); await new Promise(resolve=>server.close(resolve));}

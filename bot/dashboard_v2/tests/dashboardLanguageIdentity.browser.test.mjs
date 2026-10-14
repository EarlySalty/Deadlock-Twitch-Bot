import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createWriteStream } from 'node:fs';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as createPortProbe } from 'node:net';
import path from 'node:path';
import test from 'node:test';
import { build } from 'vite';
import { chromium } from 'playwright-core';

const frontend = path.resolve(process.env.TTB_IDENTITY_FRONTEND ?? path.join(import.meta.dirname, '..'));
const artifacts = await mkdtemp('/tmp/ttb-dashboard-language-identity-');
const entry = path.join(frontend, 'tests/dashboard-language-identity.fixture.tsx');
const source = `
import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LanguageProvider, useLanguage } from '@/context/LanguageContext';
import { PlanProvider } from '@/context/PlanContext';
import { Comparison } from '@/pages/Comparison';
import { Audience } from '@/pages/Audience';
import { InternalHomeLanding } from '@/pages/InternalHomeLanding';
import '@/index.css';
const client = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });
window.identityQueries = () => client.getQueryCache().getAll().map(query => ({ key: query.queryKey, status: query.state.status, updates: query.state.dataUpdateCount }));
function Fixture() {
  const { language, setLanguage } = useLanguage();
  const scenario = new URLSearchParams(location.search).get('scenario');
  return <><nav>{['de', 'en'].map(value => <button key={value} lang={value} aria-pressed={value === language} onClick={() => setLanguage(value)}>{value}</button>)}</nav><main>{scenario === 'comparison' ? <Comparison streamer="synthetic_creator" days={30} /> : scenario === 'audience-errors' ? <Audience streamer="synthetic_creator" days={30} /> : <InternalHomeLanding />}</main></>;
}
createRoot(document.getElementById('root')).render(<QueryClientProvider client={client}><LanguageProvider><PlanProvider plan={null} isAdmin={true} isLocalhost={false} isDemoMode={false}><Fixture /></PlanProvider></LanguageProvider></QueryClientProvider>);
`;
const stats = { avgViewers: 142, peakViewers: 220, retention10m: 62, chatHealth: 75 };
const payloads = {
  '/auth-status': { authenticated: true, isAdmin: true, isLocalhost: false, twitchLogin: 'synthetic_creator', twitchUserId: 'synthetic-id', canAccessAnalyticsDashboard: true },
  '/streamers': [{ login: 'synthetic_creator', twitchUserId: 'synthetic-id', isPartner: true }],
  '/internal-home': { profile: { twitch_login: 'synthetic_creator', display_name: 'Synthetic Creator' }, health_score: { overall: 72, sub_scores: { growth: 70, retention: 60, engagement: 80, community: 75 } } },
  '/category-comparison': { yourStats: stats, categoryAvg: stats, percentiles: stats, categoryTotal: 20, peerGroup: { tier: 'mid', tierLabel: 'Etabliert', tierSize: 8, peerAvg: stats, peerPercentiles: { avgViewers: 70, peakViewers: 65, retention10m: 75, chatHealth: 60 } } },
  '/viewer-overlap': [],
  '/audience-sharing': null,
  '/lurker-analysis': null,
  '/viewer-profiles': null,
};
const failedPaths = ['/watch-time-distribution', '/follower-funnel', '/audience-demographics'];
const cases = [
  { name: 'comparison', selector: 'main > div > :first-child > .space-y-4 > div', count: 4, requests: ['/category-comparison', '/viewer-overlap', '/audience-sharing'] },
  { name: 'home', selector: '[data-tour-id="tour-health"] > div.mt-5.space-y-3 > div', count: 4, requests: ['/auth-status', '/streamers', '/internal-home'] },
  { name: 'audience-errors', selector: 'main ul > li', count: 3, requests: failedPaths },
];

test('compiled dashboard preserves nested entity identity across language changes', { timeout: 180000 }, async (t) => {
  const dist = path.join(artifacts, 'dist');
  const built = await build({
    root: frontend,
    configFile: path.join(frontend, 'vite.config.ts'),
    mode: 'production',
    base: '/',
    plugins: [{ name: 'identity-fixture', resolveId: (id) => id === entry ? entry : undefined, load: (id) => id === entry ? source : undefined }],
    build: { outDir: dist, rollupOptions: { input: entry, output: { entryFileNames: 'identity.js' } } },
  });
  const styles = built.output.filter((output) => output.fileName.endsWith('.css')).map((output) => `<link rel="stylesheet" href="/${output.fileName}">`).join('');
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname.includes('/api/')) { response.writeHead(500).end(); return; }
    if (url.pathname === '/') {
      response.writeHead(200, { 'content-type': 'text/html' }).end(`<!doctype html><html><head>${styles}</head><body><div id="root"></div><script type="module" src="/identity.js"></script></body></html>`);
      return;
    }
    const font = url.pathname.startsWith('/brand/fonts/');
    const root = font ? '/home/nathanael/repos/Website/dl-brand/fonts' : dist;
    const file = path.resolve(root, font ? path.basename(url.pathname) : `.${decodeURIComponent(url.pathname)}`);
    if (!file.startsWith(`${root}/`)) { response.writeHead(403).end(); return; }
    try {
      const types = { '.js': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.svg': 'image/svg+xml' };
      response.writeHead(200, { 'content-type': types[path.extname(file)] ?? 'application/octet-stream' }).end(await readFile(file));
    } catch { response.writeHead(404).end(); }
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())));
  const probe = createPortProbe();
  probe.listen(0, '127.0.0.1');
  await once(probe, 'listening');
  const port = probe.address().port;
  await new Promise((resolve) => probe.close(resolve));
  const fontsConfig = path.join(artifacts, 'fonts.conf');
  await writeFile(fontsConfig, `<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig><dir>/usr/share/fonts</dir><dir>/usr/local/share/fonts</dir><cachedir>${artifacts}/font-cache</cachedir><alias><family>sans-serif</family><prefer><family>DejaVu Sans</family></prefer></alias></fontconfig>`);
  const log = createWriteStream(path.join(artifacts, 'moli.log'));
  const moli = spawn('/home/nathanael/.local/bin/moli', ['serve', '--host', '127.0.0.1', '--port', String(port), '--timeout', '60', '--layout', '--image', '--font'], { env: { PATH: '/usr/local/bin:/usr/bin:/bin', HOME: artifacts, XDG_CACHE_HOME: artifacts, FONTCONFIG_FILE: fontsConfig, FONTCONFIG_PATH: artifacts }, stdio: ['ignore', 'pipe', 'pipe'] });
  moli.stdout.pipe(log);
  moli.stderr.pipe(log);
  t.after(async () => {
    if (moli.exitCode === null) { moli.kill('SIGTERM'); await once(moli, 'exit'); }
    log.end();
  });
  let ready = false;
  for (let attempt = 0; attempt < 100 && !ready; attempt++) {
    if (moli.exitCode !== null) throw new Error(`Moli exited with ${moli.exitCode}`);
    try { ready = (await fetch(`http://127.0.0.1:${port}/json/version`)).ok; } catch { await new Promise((resolve) => setTimeout(resolve, 50)); }
  }
  assert.ok(ready, 'owned Moli CDP startup');
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  t.after(() => browser.close());
  const results = [];
  t.after(async () => {
    await writeFile(path.join(artifacts, 'result.json'), JSON.stringify({ frontend, moli: '/home/nathanael/.local/bin/moli', results }, null, 2));
    t.diagnostic(`Identity evidence: ${artifacts}`);
  });
  for (const scenario of cases) {
    await t.test(scenario.name, async () => {
      const page = await browser.contexts()[0].newPage();
      page.setDefaultTimeout(10000);
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      try {
        await page.setViewportSize({ width: 1440, height: 1000 });
        await page.addInitScript(({ payloads, failedPaths }) => {
          const NativeDate = Date;
          window.Date = class extends NativeDate {
            constructor(...args) { super(...(args.length ? args : ['2026-10-14T08:00:00Z'])); }
            static now() { return 1791964800000; }
          };
          localStorage.setItem('dashboard.language', 'de');
          window.identityRequests = [];
          const original = fetch.bind(window);
          window.fetch = (input, init) => {
            const url = new URL(typeof input === 'string' ? input : input.url, location.href);
            const method = init?.method ?? input?.method ?? 'GET';
            if (url.origin !== location.origin) return Promise.reject(new Error('External request blocked'));
            if (!url.pathname.includes('/api/')) return original(input, init);
            window.identityRequests.push({ path: url.pathname, search: url.search, method });
            if (method !== 'GET') return Promise.reject(new Error('Mutation blocked'));
            const key = url.pathname.replace(/^.*\/api\/v2/, '');
            const failed = failedPaths.includes(key);
            return Promise.resolve(new Response(JSON.stringify(failed ? { error: `synthetic:${key}` } : payloads[key] ?? null), { status: failed ? 503 : 200, headers: { 'content-type': 'application/json' } }));
          };
        }, { payloads, failedPaths });
        await page.goto(`http://127.0.0.1:${server.address().port}/?scenario=${scenario.name}&streamer=synthetic_creator`, { waitUntil: 'load' });
        await page.waitForFunction(({ selector, count }) => document.querySelectorAll(selector).length === count, scenario, { timeout: 10000 });
        await page.evaluate(async () => { await document.fonts.ready; });
        await page.screenshot({ path: path.join(artifacts, `${scenario.name}-de.png`) });
        const before = await page.evaluate(({ selector }) => {
          const rows = [...document.querySelectorAll(selector)];
          const nodes = rows.flatMap((row) => [row, ...row.querySelectorAll('*')]);
          const removed = [];
          const observer = new MutationObserver((records) => {
            for (const record of records) for (const node of record.removedNodes) if (nodes.some((tracked) => node === tracked || node.contains(tracked))) removed.push(node.nodeName);
          });
          observer.observe(document.querySelector('main'), { childList: true, subtree: true });
          window.identitySnapshot = { rows, nodes, removed, observer };
          return { url: location.href, requests: window.identityRequests, queries: window.identityQueries(), text: rows.map((row) => row.textContent), nodes: nodes.length };
        }, scenario);
        for (const endpoint of scenario.requests) assert.ok(before.requests.some((request) => request.path.endsWith(endpoint)));
        const transitions = [];
        for (const language of ['en', 'de']) {
          await page.locator(`button[lang="${language}"]`).evaluate((button) => button.click());
          await page.waitForFunction((language) => document.documentElement.lang === language, language, { timeout: 10000 });
          const after = await page.evaluate(({ selector }) => {
            const snapshot = window.identitySnapshot;
            return { url: location.href, requests: window.identityRequests, queries: window.identityQueries(), text: [...document.querySelectorAll(selector)].map((row) => row.textContent), disconnected: snapshot.nodes.filter((node) => !node.isConnected).length, rowsPreserved: [...document.querySelectorAll(selector)].every((row, index) => row === snapshot.rows[index]), removed: [...snapshot.removed] };
          }, scenario);
          transitions.push({ language, ...after });
          if (language === 'en') await page.screenshot({ path: path.join(artifacts, `${scenario.name}-en.png`) });
        }
        await page.screenshot({ path: path.join(artifacts, `${scenario.name}-after.png`) });
        const result = { scenario: scenario.name, rows: scenario.count, before, transitions, errors };
        results.push(result);
        for (const after of transitions) {
          assert.equal(after.disconnected, 0, `${scenario.name}: nested nodes disconnected`);
          assert.equal(after.rowsPreserved, true, `${scenario.name}: entity rows replaced`);
          assert.deepEqual(after.removed, [], `${scenario.name}: tracked subtrees removed`);
          assert.equal(after.url, before.url);
          assert.deepEqual(after.requests, before.requests);
          assert.deepEqual(after.queries, before.queries);
        }
        assert.notDeepEqual(transitions[0].text, before.text);
        assert.deepEqual(transitions[1].text, before.text);
        assert.deepEqual(errors, []);
      } catch (error) {
        results.push({ scenario: scenario.name, failure: error.message, errors, diagnostic: await page.evaluate(({ selector }) => ({ text: document.body.textContent, count: document.querySelectorAll(selector).length, requests: window.identityRequests, queries: window.identityQueries?.() }), scenario) });
        await page.screenshot({ path: path.join(artifacts, `${scenario.name}-failure.png`) });
        throw error;
      } finally { await page.close(); }
    });
  }
});

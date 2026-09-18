import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function waitFor(check, message, timeout = 10_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await sleep(30);
  }
  throw new Error(message);
}

async function freePort() {
  const server = net.createServer();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  await new Promise(resolve => server.close(resolve));
  return port;
}

async function openBrowser(t) {
  const executable = process.env.BROWSER_BIN || ['/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/brave-browser'].find(existsSync);
  assert.ok(executable, 'Chromium installieren oder BROWSER_BIN setzen.');
  const profile = await mkdtemp(path.join(os.tmpdir(), 'viewer-directory-browser-'));
  const port = await freePort();
  const browser = spawn(executable, [
    '--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage',
    '--disable-background-networking', '--no-first-run', '--no-default-browser-check',
    '--remote-debugging-address=127.0.0.1', `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`, 'about:blank',
  ], { stdio: 'ignore' });
  let spawnError;
  browser.on('error', error => { spawnError = error; });
  t.after(async () => {
    browser.kill('SIGTERM');
    await new Promise(resolve => {
      if (browser.exitCode !== null) return resolve();
      browser.once('exit', resolve);
      setTimeout(() => { browser.kill('SIGKILL'); resolve(); }, 2_000).unref();
    });
    await rm(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  });
  const base = `http://127.0.0.1:${port}`;
  await waitFor(async () => {
    if (spawnError) throw spawnError;
    return fetch(`${base}/json/list`).then(response => response.ok).catch(() => false);
  }, 'Browser wurde nicht bereit');
  const targets = await fetch(`${base}/json/list`).then(response => response.json());
  const socket = new WebSocket(targets.find(target => target.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  t.after(() => socket.close());
  let id = 0;
  const pending = new Map();
  const errors = [];
  socket.onmessage = event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails.text);
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    clearTimeout(request.timer);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  };
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => { pending.delete(requestId); reject(new Error(`CDP-Timeout: ${method}`)); }, 15_000);
    pending.set(requestId, { resolve, reject, timer });
    socket.send(JSON.stringify({ id: requestId, method, params }));
  });
  await send('Runtime.enable');
  await send('Page.enable');
  return {
    send,
    errors,
    async evaluate(expression) {
      const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    },
  };
}

function directoryPayload(url) {
  const search = url.searchParams.get('search') || '';
  const page = Number(url.searchParams.get('page') || 1);
  const differentScope = url.searchParams.get('streamer') !== 'test_channel' || url.searchParams.get('days') !== '30';
  const login = differentScope ? 'scope_viewer' : search || (page === 1 ? 'alpha_viewer' : 'second_page_viewer');
  return {
    viewers: search === 'no_match' ? [] : [{
      login, totalSessions: 4, totalMessages: 58, firstSeen: '2026-09-01', lastSeen: '2026-09-18',
      daysSinceLastSeen: 0, otherChannels: 2, topOtherChannels: [], category: 'dedicated',
      avgMessagesPerSession: 14.5, isLurker: false,
    }],
    total: search === 'no_match' ? 0 : search ? 1 : 100, page, perPage: 50,
    summary: { totalViewers: 100, activeViewers: 90, lurkers: 10, exclusiveViewers: 20, sharedViewers: 80, avgSessionsPerViewer: 4, avgOtherChannels: 2 },
  };
}

const segmentPayload = {
  days: 30,
  segments: { dedicated: { count: 23, pct: 23, avgMessages: 100, avgSessions: 10 } },
  churnRisk: { atRisk: 0, recentlyChurned: 0, atRiskViewers: [] },
  crossChannelStats: { exclusiveViewersPct: 20, avgOtherChannels: 2, topSharedChannels: [{ streamer: 'shared_channel', sharedCount: 80 }] },
};

test('Viewer-Verzeichnis bleibt bei langsamen Antworten bedienbar', { timeout: 120_000 }, async t => {
  let requests = [];
  let held = [];
  let hold = () => false;
  let fail = () => false;
  const release = () => { for (const respond of held.splice(0)) respond(); };
  const server = await createServer({
    root: ROOT, mode: 'test', base: '/', logLevel: 'error',
    server: { host: '127.0.0.1', port: 0, strictPort: false },
    plugins: [{
      name: 'viewer-directory-test-api',
      configureServer(vite) {
        vite.middlewares.use((req, res, next) => {
          const url = new URL(req.url, 'http://127.0.0.1');
          const kind = url.pathname.endsWith('/viewer-directory') ? 'directory' : url.pathname.endsWith('/viewer-segments') ? 'segments' : null;
          if (!kind) return next();
          const request = { kind, search: url.searchParams.get('search') || '', page: Number(url.searchParams.get('page') || 1), url, aborted: false };
          requests.push(request);
          res.on('close', () => { if (!res.writableEnded) request.aborted = true; });
          const respond = () => {
            if (res.destroyed) return;
            const failed = fail(request);
            res.writeHead(failed ? 503 : 200, { 'content-type': 'application/json' });
            res.end(JSON.stringify(failed ? { error: 'Testfehler' } : kind === 'segments' ? segmentPayload : directoryPayload(url)));
          };
          if (hold(request)) held.push(respond);
          else respond();
        });
      },
    }],
  });
  await server.listen();
  t.after(async () => { release(); await server.close(); });
  const browser = await openBrowser(t);
  const { evaluate, send } = browser;
  const port = server.httpServer.address().port;
  let caseId = 0;
  const input = `document.querySelector('input[placeholder="Viewer suchen..."]')`;
  const textIncludes = text => evaluate(`document.body.innerText.includes(${JSON.stringify(text)})`);
  const searchRequests = () => requests.filter(request => request.kind === 'directory' && request.search);
  async function open(options = {}) {
    release();
    requests = [];
    hold = options.hold || (() => false);
    fail = options.fail || (() => false);
    browser.errors.length = 0;
    const url = `http://127.0.0.1:${port}/tests/browser/viewerDirectory.html?case=${++caseId}`;
    await send('Page.navigate', { url });
    await waitFor(() => evaluate(`location.href === ${JSON.stringify(url)} && document.body?.dataset.fixture === ${JSON.stringify(String(caseId))}`), 'Testansicht fehlt');
    await waitFor(() => requests.some(request => request.kind === 'directory'), 'Initiale Anfrage fehlt');
  }
  async function loaded() {
    await waitFor(() => textIncludes('alpha_viewer'), 'Initiale Viewer fehlen');
    await waitFor(() => textIncludes('shared_channel'), 'Segmente fehlen');
    assert.equal(await textIncludes('100 Viewer'), true);
    assert.equal(await textIncludes('20% exklusiv'), true);
  }
  async function type(value) {
    return evaluate(`(() => {
      const input = ${input}; if (!input) return false;
      input.focus();
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, ${JSON.stringify(value)});
      input.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    })()`);
  }
  async function click(label) {
    assert.equal(await evaluate(`(() => {
      const button = [...document.querySelectorAll('button')].find(button => button.textContent.trim() === ${JSON.stringify(label)});
      if (!button || button.disabled) return false; button.click(); return true;
    })()`), true, `Button nicht bedienbar: ${label}`);
  }

  await t.test('Suchfeld und fertige Segmente erscheinen vor der langsamen Tabelle', async () => {
    await open({ hold: request => request.kind === 'directory' });
    await waitFor(() => textIncludes('shared_channel'), 'Fertige Segmente werden vom Tabellen-Spinner verdeckt', 2_000);
    assert.equal(await evaluate(`Boolean(${input})`), true);
    release();
    await loaded();
  });

  await t.test('Schnelles Tippen erzeugt genau eine Suche', async () => {
    await open(); await loaded();
    await evaluate(`new Promise(resolve => {
      const input = ${input}; input.focus();
      const values = ['t', 'ta', 'tal', 'tali', 'taliv', 'taliva', 'talival'];
      values.forEach((value, index) => setTimeout(() => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, value);
        input.dispatchEvent(new Event('input', { bubbles: true }));
        if (index === values.length - 1) resolve();
      }, index * 25));
    })`);
    await waitFor(() => searchRequests().some(request => request.search === 'talival'), 'Letzte Suche fehlt');
    await sleep(100);
    assert.deepEqual(searchRequests().map(request => request.search), ['talival']);
    assert.equal(await evaluate(`${input} === document.activeElement`), true);
  });

  await t.test('Suchfeld, Fokus und alte Zeilen bleiben beim Nachladen erhalten', async () => {
    await open({ hold: request => Boolean(request.search) }); await loaded();
    await evaluate(`window.__oldInput = ${input}; window.__oldShared = [...document.querySelectorAll('span')].find(element => element.textContent === 'shared_channel'); true;`);
    assert.equal(await type('talival'), true);
    await waitFor(() => searchRequests().length > 0, 'Suchanfrage fehlt');
    assert.equal(await evaluate(`window.__oldInput.isConnected && ${input} === window.__oldInput && document.activeElement === window.__oldInput`), true);
    assert.equal(await textIncludes('alpha_viewer'), true);
    assert.equal(await evaluate('window.__oldShared.isConnected'), true);
    assert.equal(await evaluate(`${input}.value`), 'talival');
    release();
    await waitFor(() => textIncludes('talival'), 'Suchergebnis fehlt');
  });

  await t.test('Überholte Suche wird abgebrochen und kann das Ergebnis nicht überschreiben', async () => {
    await open({ hold: request => request.search === 'slow' }); await loaded();
    await type('slow');
    await waitFor(() => searchRequests().some(request => request.search === 'slow'), 'Langsame Suche fehlt');
    assert.equal(await type('talival'), true, 'Weitertippen während der Anfrage muss möglich sein');
    await waitFor(() => searchRequests().some(request => request.search === 'slow' && request.aborted), 'Überholte Anfrage bleibt aktiv', 3_000);
    await waitFor(() => textIncludes('talival'), 'Aktuelles Ergebnis fehlt');
    release(); await sleep(100);
    assert.equal(await evaluate(`document.querySelector('tbody').innerText.includes('slow')`), false);
  });

  await t.test('Fehler lässt die Suche stehen und eine neue Eingabe erholt sich', async () => {
    await open({ fail: request => request.search === 'failure' }); await loaded();
    await type('failure');
    await waitFor(() => textIncludes('Erneut laden'), 'Fehlerhinweis fehlt');
    assert.equal(await evaluate(`Boolean(${input})`), true);
    assert.equal(await type('talival'), true);
    await waitFor(() => evaluate(`document.querySelector('tbody')?.innerText.includes('talival')`), 'Neue Suche erholt sich nicht');
    assert.equal(await textIncludes('shared_channel'), true);
  });

  await t.test('Suche von Seite zwei setzt erst zusammen mit dem Suchbegriff auf Seite eins zurück', async () => {
    await open(); await loaded();
    await click('Weiter');
    await waitFor(() => textIncludes('second_page_viewer'), 'Zweite Seite fehlt');
    const before = requests.length;
    await type('talival');
    await waitFor(() => searchRequests().length > 0, 'Neue Suche fehlt');
    assert.deepEqual(requests.slice(before).filter(request => request.kind === 'directory').map(request => [request.search, request.page]), [['talival', 1]]);
  });

  for (const label of ['Kanal wechseln', 'Zeitraum wechseln']) {
    await t.test(`${label}: keine alten Viewer im neuen Kontext`, async () => {
      await open({ hold: request => request.kind === 'directory' && (request.url.searchParams.get('streamer') !== 'test_channel' || request.url.searchParams.get('days') !== '30') });
      await loaded(); await click(label);
      await waitFor(() => held.length > 0, 'Neue Kontext-Anfrage fehlt');
      assert.equal(await textIncludes('alpha_viewer'), false);
      assert.equal(await evaluate(`Boolean(${input})`), true);
      release();
      await waitFor(() => textIncludes('scope_viewer'), 'Neuer Kontext fehlt');
    });
  }

  await t.test('Leere Ergebnisse lassen die Suche und Übersicht stehen', async () => {
    await open(); await loaded();
    await type('no_match');
    await waitFor(() => textIncludes('Keine Viewer gefunden'), 'Leerzustand fehlt');
    assert.equal(await evaluate(`${input}.value`), 'no_match');
    assert.equal(await textIncludes('shared_channel'), true);
    assert.deepEqual(browser.errors, []);
  });
});

import { readFileSync, writeFileSync } from 'node:fs';
import http from 'node:http';

const UPSTREAM = { host: '127.0.0.1', port: 8769 };
const PROXY_PORT = Number(process.env.SAI_PROXY_PORT || 18769);
const DASH = `http://127.0.0.1:${PROXY_PORT}`;
const token = process.env.SAI_TOKEN || '';
delete process.env.SAI_TOKEN;
const [stepsFile, outDir] = process.argv.slice(2);
const cdpBase = process.env.MOLI_CDP || 'http://127.0.0.1:9558';
const steps = JSON.parse(readFileSync(stepsFile, 'utf8'));
const t0 = Date.now();
const log = (m) => console.log(((Date.now() - t0) / 1000).toFixed(1), String(m).replaceAll(token || '\u0000', '***'));

const DIST = process.env.SAI_DIST || '/opt/deadlock/twitch/current/bot/analytics/dashboard_v2/dist';
const RUNTIME = '<script>window.__TWITCH_DASHBOARD_RUNTIME__=Object.freeze({"apiBase":"/twitch/api/v2","demoMode":false,"allowedDemoProfiles":[]});</script>';
const isShell = (url) => /^\/twitch\/social-media(\/|\?|$)/.test(url) && !url.startsWith('/twitch/social-media/api/');
const proxy = http.createServer((req, res) => {
  if (req.method === 'GET' && isShell(req.url)) {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(readFileSync(`${DIST}/index.html`, 'utf8').replace('</head>', `${RUNTIME}\n  </head>`));
    return;
  }
  const headers = { ...req.headers, host: `${UPSTREAM.host}:${UPSTREAM.port}`, 'x-internal-token': token };
  const upstream = http.request({ ...UPSTREAM, method: req.method, path: req.url, headers }, (up) => {
    res.writeHead(up.statusCode, up.headers);
    up.pipe(res);
  });
  upstream.on('error', () => { res.writeHead(502); res.end(); });
  req.pipe(upstream);
});
await new Promise((r) => proxy.listen(PROXY_PORT, '127.0.0.1', r));

const version = await (await fetch(cdpBase + '/json/version')).json();
const ws = new WebSocket(version.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
let session;

function send(method, params = {}, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, sessionId: method.startsWith('Target.') ? undefined : session }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error('timeout ' + method)); } }, timeoutMs);
  });
}

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(msg.error.message)); else resolve(msg.result);
    return;
  }
};

async function evaluate(expression, timeoutMs = 30000) {
  const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, timeoutMs);
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || 'eval failed');
  return result.result.value;
}

const helpers = `
window.__sai = {
  visible(el) { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; },
  byText(text, selector) {
    const all = [...document.querySelectorAll(selector || 'button, a, [role=button], [role=tab], summary, label')];
    return all.find((el) => this.visible(el) && (el.innerText || el.getAttribute('aria-label') || '').trim().includes(text));
  },
  click(text, selector) { const el = this.byText(text, selector); if (!el) return 'missing: ' + text; el.scrollIntoView({block: 'center'}); el.click(); return 'clicked: ' + (el.innerText || el.getAttribute('aria-label')).trim().slice(0, 80); },
  type(selector, value) {
    const el = [...document.querySelectorAll(selector)].find((e) => this.visible(e));
    if (!el) return 'missing: ' + selector;
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return 'typed';
  },
  texts() { return [...document.querySelectorAll('button, a, [role=tab], h1, h2, h3, [role=dialog] p, [role=alert], [role=status]')].filter((e) => this.visible(e)).map((e) => (e.innerText || e.getAttribute('aria-label') || '').trim().replace(/\\s+/g, ' ').slice(0, 140)).filter(Boolean); },
};
'ok'`;

ws.onopen = async () => {
  const results = [];
  try {
    const target = await send('Target.createTarget', { url: 'about:blank' });
    session = (await send('Target.attachToTarget', { targetId: target.targetId, flatten: true })).sessionId;
    await send('Page.enable');
    await send('Runtime.enable');
    await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
    for (const step of steps) {
      let out;
      if (step.nav) { await send('Page.navigate', { url: DASH + step.nav }, 90000); await new Promise((r) => setTimeout(r, step.wait ?? 6000)); await evaluate(helpers); out = 'nav ' + step.nav; }
      else if (step.wait) { await new Promise((r) => setTimeout(r, step.wait)); out = 'waited ' + step.wait; }
      else if (step.click) { await evaluate(helpers); out = await evaluate(`__sai.click(${JSON.stringify(step.click)}, ${JSON.stringify(step.selector || null)})`); }
      else if (step.type) { await evaluate(helpers); out = await evaluate(`__sai.type(${JSON.stringify(step.type)}, ${JSON.stringify(step.value)})`); }
      else if (step.eval) { await evaluate(helpers); out = await evaluate(step.eval, step.timeout || 30000); }
      else if (step.texts) { await evaluate(helpers); out = await evaluate('__sai.texts()'); }
      else if (step.until) {
        const deadline = Date.now() + (step.timeout || 600000);
        out = false;
        while (Date.now() < deadline) {
          await evaluate(helpers).catch(() => {});
          out = await evaluate(step.until).catch(() => false);
          if (out) break;
          await new Promise((r) => setTimeout(r, 5000));
        }
      }
      else if (step.shot) { const shot = await send('Page.captureScreenshot', { format: 'png' }, 60000); writeFileSync(`${outDir}/${step.shot}.png`, Buffer.from(shot.data, 'base64')); out = 'shot ' + step.shot; }
      log(JSON.stringify(out).slice(0, 4000));
      results.push({ step, out });
    }
  } catch (error) {
    log('error ' + error.message);
    results.push({ error: error.message });
  }
  writeFileSync(`${outDir}/ui-result.json`, JSON.stringify(results, null, 1).replaceAll(token || '\u0000', '***'));
  process.exit(0);
};

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DIST = path.resolve(HERE, '../../analytics/dashboard_v2/dist');

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

function authPayload(adminMode) {
  if (adminMode) {
    return {
      authenticated: true,
      level: 'admin',
      authLevel: 'admin',
      demoMode: false,
      isAdmin: true,
      adminEligible: true,
      adminMode: true,
      isLocalhost: false,
      canViewAllStreamers: true,
      twitchLogin: null,
      adminDefaultStreamer: 'earlysalty',
      displayName: null,
      partnerStatus: 'active',
      technicalPauseReason: null,
      operationalState: 'active',
      canAccessAnalyticsDashboard: true,
      tokenErrorGraceExpiresAt: null,
      csrfToken: null,
      csrf_token: null,
      plan: {
        planId: 'analysis_dashboard',
        planName: 'Erweitert (Admin)',
        tier: 'extended',
        isExtended: true,
        expiresAt: null,
        source: 'admin',
        entitlements: [
          'analytics.basic',
          'analytics.ai_full',
          'analytics.extended',
          'chat.lurker_tax',
          'chat.promos.disable',
          'raid.priority',
        ],
      },
      access: { landing: true, analytics: true },
      permissions: {
        viewAllStreamers: true,
        viewComparison: true,
        viewChatAnalytics: true,
        viewOverlap: true,
      },
    };
  }

  return {
    authenticated: true,
    level: 'partner',
    authLevel: 'partner',
    demoMode: false,
    isAdmin: false,
    adminEligible: true,
    adminMode: false,
    isLocalhost: false,
    canViewAllStreamers: false,
    twitchLogin: 'earlysalty',
    adminDefaultStreamer: null,
    displayName: null,
    partnerStatus: 'active',
    technicalPauseReason: null,
    operationalState: 'active',
    canAccessAnalyticsDashboard: true,
    tokenErrorGraceExpiresAt: null,
    csrfToken: null,
    csrf_token: null,
    plan: {
      planId: 'raid_free',
      planName: 'Free',
      tier: 'free',
      isExtended: false,
      expiresAt: null,
      source: 'default',
      entitlements: [],
    },
    access: { landing: true, analytics: true },
    permissions: {
      viewAllStreamers: false,
      viewComparison: true,
      viewChatAnalytics: true,
      viewOverlap: true,
    },
  };
}

function homePayload() {
  return {
    profile: {
      twitch_login: 'earlysalty',
      twitch_user_id: '42',
      display_name: 'earlysalty',
    },
    status: {
      authenticated: true,
      streamer_bound: true,
      period_days: 30,
      oauth: {
        connected: true,
        status: 'connected',
        needs_reauth: false,
        granted_scopes: ['channel:manage:raids'],
        missing_scopes: [],
        reconnect_url: '/twitch/raid/auth',
        profile_url: '/twitch/dashboard',
      },
      discord: { connected: true, status: 'connected' },
      raid_status: { state: 'active', read_only: true },
      partner: {
        status: 'active',
        technical_pause_reason: null,
        operational_state: 'active',
      },
      access: { landing: true, analytics: true },
    },
    kpis: {
      streams_count: 0,
      avg_viewers: 0,
      follower_delta: 0,
      bot_bans_keyword_count: 0,
    },
    recent_streams: [],
    last_stream_summary: null,
    health_score: null,
    week_comparison: null,
    live_status: null,
    bot_impact: { events: [], summary: {} },
    bot_activity: { events: [] },
    links: {},
    changelog: { entries: [] },
  };
}

function json(response, status = 200) {
  return {
    status,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(response),
  };
}

async function createDashboardServer() {
  let adminMode = true;
  const requests = [];
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    requests.push(`${req.method} ${url.pathname}${url.search}`);

    let response;
    if (url.pathname === '/twitch/api/v2/auth-status') {
      response = json(authPayload(adminMode));
    } else if (url.pathname === '/twitch/api/v2/streamers') {
      response = json([{ login: 'earlysalty', isPartner: true }]);
    } else if (url.pathname === '/twitch/api/v2/admin-mode' && req.method === 'POST') {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      adminMode = Boolean(JSON.parse(Buffer.concat(chunks).toString()).enabled);
      response = json({ adminMode });
    } else if (url.pathname === '/twitch/api/v2/internal-home') {
      const streamer = url.searchParams.get('streamer');
      if (adminMode && streamer !== 'earlysalty') {
        response = json(
          {
            error: 'streamer_session_required',
            message: 'Admin requests require a streamer while admin mode is active.',
            loginUrl: '/twitch/auth/login?next=%2Ftwitch%2Fdashboard',
          },
          401,
        );
      } else {
        response = json(homePayload());
      }
    } else if (url.pathname === '/twitch/dashboard') {
      let html = await readFile(path.join(DIST, 'index.html'), 'utf8');
      html = html.replace(
        '</head>',
        `<script>
          window.__browserErrors = [];
          window.addEventListener('error', event => window.__browserErrors.push(String(event.error || event.message)));
          window.addEventListener('unhandledrejection', event => window.__browserErrors.push(String(event.reason)));
        </script></head>`,
      );
      response = { status: 200, headers: { 'content-type': 'text/html' }, body: html };
    } else if (url.pathname.startsWith('/twitch/dashboard-v2/')) {
      const relative = url.pathname.slice('/twitch/dashboard-v2/'.length);
      const file = path.resolve(DIST, relative);
      if (!file.startsWith(`${DIST}${path.sep}`)) {
        response = { status: 404, headers: {}, body: '' };
      } else {
        try {
          const body = await readFile(file);
          const contentType = file.endsWith('.js')
            ? 'text/javascript'
            : file.endsWith('.css')
              ? 'text/css'
              : 'application/octet-stream';
          response = { status: 200, headers: { 'content-type': contentType }, body };
        } catch {
          response = { status: 404, headers: {}, body: '' };
        }
      }
    } else {
      response = { status: 404, headers: {}, body: '' };
    }

    res.writeHead(response.status, response.headers);
    res.end(response.body);
  });

  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}/twitch/dashboard`,
    requests,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

async function webdriverRequest(base, pathname, body) {
  const response = await fetch(`${base}${pathname}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok || payload.value?.error) {
    throw new Error(`WebDriver ${pathname}: ${response.status} ${JSON.stringify(payload)}`);
  }
  return payload.value;
}

async function waitFor(check, message, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    last = await check();
    if (last) return last;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`${message}; letzter Wert: ${JSON.stringify(last)}`);
}

test('Admin-Modus wechselt ohne leeren Bildschirm oder Login-Redirect', async (t) => {
  const dashboard = await createDashboardServer();
  t.after(() => dashboard.close());

  const driverPort = await freePort();
  const driver = spawn('geckodriver', ['--port', String(driverPort)], {
    stdio: 'ignore',
  });
  driver.unref();
  t.after(() => {
    driver.kill('SIGTERM');
  });

  const driverBase = `http://127.0.0.1:${driverPort}`;
  await waitFor(
    async () => fetch(`${driverBase}/status`).then((response) => response.ok).catch(() => false),
    'Geckodriver wurde nicht bereit',
  );

  const session = await webdriverRequest(driverBase, '/session', {
    capabilities: {
      alwaysMatch: {
        browserName: 'firefox',
        acceptInsecureCerts: true,
        'moz:firefoxOptions': { args: ['-headless'] },
      },
    },
  });
  const sessionId = session.sessionId;
  t.after(() =>
    fetch(`${driverBase}/session/${sessionId}`, { method: 'DELETE' }).catch(() => undefined),
  );

  const execute = (script) =>
    webdriverRequest(driverBase, `/session/${sessionId}/execute/sync`, {
      script,
      args: [],
    });

  await webdriverRequest(driverBase, `/session/${sessionId}/url`, { url: dashboard.url });
  await waitFor(
    () =>
      execute(
        `return document.body.innerText.includes('Admin-Modus beenden') &&
          document.getElementById('root')?.childElementCount > 0`,
      ),
    'Admin-Ansicht wurde nicht gerendert',
  );

  await execute(`
    const button = [...document.querySelectorAll('button')]
      .find(element => element.textContent?.includes('Admin-Modus beenden'));
    if (!button) return false;
    button.click();
    return true;
  `);

  await waitFor(
    () => execute(`return document.body.innerText.includes('Admin-Modus aktivieren')`),
    'Nutzeransicht wurde nach Beenden nicht gerendert',
  );
  assert.equal(
    await execute(`return window.location.pathname`),
    '/twitch/dashboard',
    `Unerwartete Navigation; Requests: ${dashboard.requests.join(', ')}`,
  );
  assert.deepEqual(await execute(`return window.__browserErrors`), []);
  assert.equal(
    await execute(`return document.getElementById('root')?.childElementCount > 0`),
    true,
  );

  await execute(`
    const button = [...document.querySelectorAll('button')]
      .find(element => element.textContent?.includes('Admin-Modus aktivieren'));
    if (!button) return false;
    button.click();
    return true;
  `);

  await waitFor(
    () => execute(`return document.body.innerText.includes('Admin-Modus beenden')`),
    'Admin-Ansicht wurde nach Aktivieren nicht gerendert',
  );
  assert.equal(await execute(`return window.location.pathname`), '/twitch/dashboard');
  assert.deepEqual(
    await execute(`return window.__browserErrors`),
    [],
  );
});

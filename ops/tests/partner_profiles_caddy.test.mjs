import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';

async function listen(server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return server.address().port;
}

test('bare profiles and calendar queries reach the backend without stealing existing pages', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'partner-profile-caddy-'));
  // Only isolated loopback listeners; the production Caddy admin is never used.
  const upstream = createServer((request, response) => {
    response.setHeader('Cache-Control', 'no-store, max-age=0');
    response.setHeader('Content-Security-Policy', "default-src 'self'");
    response.statusCode = request.url.includes('@') || request.url.includes('inactive') ? 404 : 200;
    response.end(`profile:${request.url}`);
  });
  const portReservation = createServer();
  let child;
  let output = '';
  try {
    const upstreamPort = await listen(upstream);
    const port = await listen(portReservation);
    await new Promise(resolve => portReservation.close(resolve));
    const snippet = readFileSync(new URL('../caddy/partner-profiles.caddy', import.meta.url), 'utf8')
      .replaceAll('127.0.0.1:8769', `127.0.0.1:${upstreamPort}`);
    const assets = readFileSync(new URL('../caddy/partner-profile-assets.caddy', import.meta.url), 'utf8').replaceAll('127.0.0.1:8769', `127.0.0.1:${upstreamPort}`);
    const config = join(directory, 'Caddyfile');
    writeFileSync(config, `{
 admin off
 auto_https off
}
http://127.0.0.1:${port} {
 ${assets}
 handle /streamer* {
  ${snippet}
  @dynamic path /streamer/commands /streamer/help /streamer/faq
  handle @dynamic {
   respond "legacy-dynamic" 200
  }
  handle {
   respond "landing-or-asset" 200
  }
 }
 handle {
  respond "missing" 404
 }
}
`);
    const adapted = spawnSync('caddy', ['adapt', '--config', config, '--adapter', 'caddyfile'], { encoding: 'utf8' });
    assert.equal(adapted.status, 0, adapted.stderr);
    child = spawn('caddy', ['run', '--config', config, '--adapter', 'caddyfile'], { stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', chunk => { output += chunk; });
    child.stderr.on('data', chunk => { output += chunk; });
    const base = `http://127.0.0.1:${port}`;
    let ready = false;
    for (let i = 0; i < 100; i++) {
      try { const response = await fetch(base, { signal: AbortSignal.timeout(200) }); await response.text(); ready = true; break; }
      catch { await new Promise(resolve => setTimeout(resolve, 40)); }
    }
    assert.ok(ready, output);
    for (const path of ['/streamer/alice', '/streamer/alice/', '/streamer/Alice_1?month=2025-01', '/streamer/alice/?month=2026-10', '/twitch/profile-assets/profile.css']) {
      const response = await fetch(base + path);
      assert.equal(response.status, 200, path);
      assert.equal(await response.text(), `profile:${path}`);
      assert.equal(response.headers.get('cache-control'), 'no-store, max-age=0');
      assert.equal(response.headers.get('content-security-policy'), "default-src 'self'");
    }
    for (const path of ['/streamer/inactive', '/streamer/@alice']) {
      const response = await fetch(base + path);
      assert.equal(response.status, 404, path);
      assert.equal(response.headers.get('cache-control'), 'no-store, max-age=0');
      await response.text();
    }
    for (const route of ['commands', 'help', 'faq']) {
      const response = await fetch(`${base}/streamer/${route}`);
      assert.equal(await response.text(), 'legacy-dynamic', route);
    }
    for (const path of ['/streamer', '/streamer/', '/streamer/vergleich', '/streamer/vergleich/', '/streamer/v1/', '/streamer/v2', '/streamer/v3/', '/streamer/onboarding', '/streamer/vertriebler', '/streamer/affiliate-portal/', '/streamer/assets/a.js', '/streamer/fonts/a.woff2', '/streamer/favicon.ico']) {
      const response = await fetch(base + path);
      assert.equal(await response.text(), 'landing-or-asset', path);
    }
  } finally {
    if (child && child.exitCode === null) { child.kill('SIGTERM'); await once(child, 'exit'); }
    upstream.closeAllConnections();
    await new Promise(resolve => upstream.close(resolve));
    portReservation.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

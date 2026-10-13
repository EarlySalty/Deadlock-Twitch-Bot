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

test('dead-lock.de serves the backend directly and keeps the Host header', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'dead-lock-caddy-'));
  // Only isolated loopback listeners; the production Caddy admin is never used.
  const seen = [];
  const upstream = createServer((request, response) => {
    seen.push({
      url: request.url,
      host: request.headers.host,
      context: request.headers['x-dashboard-context'],
      proto: request.headers['x-forwarded-proto'],
    });
    response.setHeader('Content-Type', 'text/plain; charset=utf-8');
    response.end(`backend:${request.url}`);
  });
  const portReservation = createServer();
  let child;
  let output = '';
  try {
    const upstreamPort = await listen(upstream);
    const port = await listen(portReservation);
    await new Promise(resolve => portReservation.close(resolve));
    const body = readFileSync(new URL('../caddy/dead-lock-domain.caddy', import.meta.url), 'utf8')
      .replaceAll('127.0.0.1:8769', `127.0.0.1:${upstreamPort}`);
    const config = join(directory, 'Caddyfile');
    writeFileSync(config, `{
 admin off
 auto_https off
}
http://127.0.0.1:${port} {
${body}
}
`);
    const validation = spawnSync('caddy', ['validate', '--config', config, '--adapter', 'caddyfile'], { encoding: 'utf8' });
    assert.equal(validation.status, 0, validation.stderr || validation.stdout);
    child = spawn('caddy', ['run', '--config', config, '--adapter', 'caddyfile'], { stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', chunk => { output += chunk; });
    child.stderr.on('data', chunk => { output += chunk; });
    const base = `http://127.0.0.1:${port}`;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try {
        await fetch(`${base}/`, { redirect: 'manual' });
        break;
      } catch {
        await new Promise(resolve => setTimeout(resolve, 50));
      }
    }

    for (const path of [
      '/twitch/dashboard',
      '/twitch/auth/login?next=%2Ftwitch%2Fanalyse',
      '/twitch/api/v2/auth/status',
      '/twitch/raid/auth?scope_profile=base',
      '/callback/twitch?code=c&state=s',
      '/callback/kick?code=c&state=s',
      '/callback/youtube?code=c&state=s',
      '/streamer',
      '/streamer/somepartner',
      '/demo/',
    ]) {
      const response = await fetch(`${base}${path}`, { redirect: 'manual' });
      assert.equal(response.status, 200, `${path}: ${output}`);
      assert.equal(await response.text(), `backend:${path}`);
      const last = seen.at(-1);
      assert.equal(last.host, `127.0.0.1:${port}`, `${path} keeps the Host header`);
      assert.equal(last.context, 'public');
      assert.equal(last.proto, 'https');
    }

    const root = await fetch(`${base}/`, { redirect: 'manual' });
    assert.equal(root.status, 302, `root: ${root.status} ${await root.text()} ${output}`);
    assert.equal(root.headers.get('location'), '/streamer');

    const before = seen.length;
    for (const path of ['/patchnotes/', '/link', '/admin', '/clips/']) {
      const response = await fetch(`${base}${path}`, { redirect: 'manual' });
      assert.equal(response.status, 404, path);
      assert.equal(response.headers.get('location'), null, `${path} must not redirect`);
    }
    assert.equal(seen.length, before, 'main-domain-only paths never reach the backend');
  } finally {
    if (child) {
      child.kill('SIGTERM');
      await once(child, 'exit').catch(() => {});
    }
    upstream.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

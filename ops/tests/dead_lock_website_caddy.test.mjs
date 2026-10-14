import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';

async function listen(server) {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return server.address().port;
}

test('English website mounts retain OAuth precedence and unrelated domain redirects', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'dead-lock-website-'));
  const upstream = createServer((request, response) => response.end(`upstream:${request.url}`));
  const reservation = createServer();
  let child;
  try {
    const upstreamPort = await listen(upstream);
    const port = await listen(reservation);
    await new Promise(resolve => reservation.close(resolve));
    const dist = join(directory, 'dist');
    for (const page of ['', 'faq', 'vergleich', 'clips', 'onboarding']) {
      mkdirSync(join(dist, page), { recursive: true });
      writeFileSync(join(dist, page, 'index.html'), `website:${page || 'landing'}`);
    }
    mkdirSync(join(dist, 'assets'));
    writeFileSync(join(dist, 'assets', 'app.js'), 'website-asset');
    const repo = new URL('../caddy/', import.meta.url).pathname;
    const source = readFileSync(join(repo, 'dead-lock-de.caddy'), 'utf8');
    const snippets = source.slice(0, source.indexOf('http://dead-lock.de'))
      .replaceAll('/opt/deadlock/twitch/current/website/dist', dist)
      .replaceAll('/etc/caddy/partner-profiles.caddy', join(repo, 'partner-profiles.caddy'))
      .replaceAll('127.0.0.1:8769', `127.0.0.1:${upstreamPort}`);
    const config = join(directory, 'Caddyfile');
    writeFileSync(config, `{\n admin off\n auto_https off\n}\n(common_probe_blockers) {\n @probe path /.git/*\n handle @probe {\n  respond 404\n }\n}\n${snippets}\nhttp://127.0.0.1:${port} {\n import dead_lock_english_dashboard\n}\n`);
    child = spawn('caddy', ['run', '--config', config, '--adapter', 'caddyfile'], { stdio: ['ignore', 'ignore', 'pipe'] });
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('isolated Caddy startup timed out')), 15000);
      let output = '';
      child.stderr.on('data', chunk => {
        output += chunk;
        if (output.includes('serving initial configuration')) { clearTimeout(timer); resolve(); }
      });
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('exit', code => { clearTimeout(timer); reject(new Error(`isolated Caddy exited ${code}`)); });
    });
    const base = `http://127.0.0.1:${port}`;
    async function request(path) {
      return fetch(base + path, { redirect: 'manual', signal: AbortSignal.timeout(3000) });
    }
    for (const [path, location, status] of [
      ['/', '/streamer/', 302],
      ['/?lang=en&x=1', '/streamer/?lang=en&x=1', 302],
      ['/streamer', '/streamer/', 308],
      ['/streamer/v2/', '/streamer/', 308],
      ['/streamer/onboarding/', '/twitch/onboarding', 308],
      ['/faq/', 'https://deutsche-deadlock-community.de/faq/', 308],
      ['/clips/', 'https://deutsche-deadlock-community.de/clips/', 308],
      ['/privacy', 'https://deutsche-deadlock-community.de/privacy', 308],
      ['/twitch/auth/login?next=%2Ftwitch%2Fdashboard', 'https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard&lang=en', 302],
      ['/twitch/auth/login?lang=de', 'https://deutsche-deadlock-community.de/twitch/auth/login?lang=de', 302],
    ]) {
      const response = await request(path);
      assert.equal(response.status, status, path);
      assert.equal(response.headers.get('location'), location, path);
      await response.text();
    }
    for (const [path, body] of [
      ['/streamer/', 'website:landing'],
      ['/streamer/faq/', 'website:faq'],
      ['/streamer/vergleich/', 'website:vergleich'],
      ['/streamer/clips/', 'website:clips'],
      ['/streamer/assets/app.js', 'website-asset'],
      ['/website/vergleich/', 'website:vergleich'],
      ['/twitch/onboarding/', 'website:onboarding'],
      ['/twitch/faq/', 'website:faq'],
      ['/twitch/dashboard', 'upstream:/twitch/dashboard'],
      ['/streamer/commands', 'upstream:/streamer/commands'],
      ['/streamer/faq', 'upstream:/streamer/faq'],
    ]) {
      const response = await request(path);
      assert.equal(response.status, 200, path);
      assert.equal(await response.text(), body, path);
      assert.equal(response.headers.get('x-content-type-options'), 'nosniff', path);
      if (!path.endsWith('.js')) assert.ok(response.headers.get('content-security-policy'), path);
    }
    assert.equal((await request('/streamer/assets/missing.js')).status, 404);
    assert.equal((await request('/.git/config')).status, 404);
    assert.match(source, /@english_site path \/ \/streamer\* \/website\* \/twitch \/twitch\/\*/);
    assert.match(source, /redir @english_site https:\/\/\{host\}\{uri\} 308/);
  } finally {
    if (child && child.exitCode === null) { child.kill('SIGTERM'); await once(child, 'exit'); }
    upstream.closeAllConnections();
    await new Promise(resolve => upstream.close(resolve));
    reservation.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

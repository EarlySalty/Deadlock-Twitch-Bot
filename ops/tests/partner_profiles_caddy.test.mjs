import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

test('profile and stylesheet routes win over the landing fallback', () => {
  const directory = mkdtempSync(join(tmpdir(), 'partner-profile-caddy-'));
  try {
    const snippet = fileURLToPath(new URL('../caddy/partner-profiles.caddy', import.meta.url));
    const config = join(directory, 'Caddyfile');
    writeFileSync(config, `http://127.0.0.1:18769 {\n import ${snippet}\n handle /streamer* {\n respond "landing" 200\n }\n handle {\n respond "missing" 404\n }\n}\n`);
    // Adapt only: no listener, certificate requests or production changes.
    const result = spawnSync('caddy', ['adapt', '--config', config, '--adapter', 'caddyfile'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const parsed = JSON.parse(result.stdout);
    const routes = Object.values(parsed.apps.http.servers)[0].routes[0].handle[0].routes;
    const paths = routes.flatMap(route => route.match?.flatMap(m => m.path ?? []) ?? []);
    assert.ok(paths.indexOf('/streamer/@*') < paths.indexOf('/streamer*'));
    assert.ok(paths.includes('/twitch/profile-assets/*'));
    const profile = routes.find(route => route.match?.some(m => m.path?.includes('/streamer/@*')));
    assert.match(JSON.stringify(profile), /127\.0\.0\.1:8769/);
    assert.doesNotMatch(JSON.stringify(profile), /static_response|file_server/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

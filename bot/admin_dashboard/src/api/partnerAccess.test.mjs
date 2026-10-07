import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('./client.ts', import.meta.url), 'utf8');
const formatters = new URL('../utils/formatters.ts', import.meta.url).href;
const javascript = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText.replace('@/utils/formatters', formatters);
const { setPartnerAccess } = await import(`data:text/javascript;base64,${Buffer.from(javascript).toString('base64')}`);

test('Freigabe und Entzug senden die Twitch-ID an den Adminalias', async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response(JSON.stringify(url.endsWith('/auth-status')
      ? { authenticated: true, isAdmin: true, csrfToken: 'test-csrf', loginUrl: '/login', discordLoginUrl: '/login' }
      : { success: true }), { headers: { 'Content-Type': 'application/json' } });
  };
  try {
    for (const granted of [true, false]) {
      const result = await setPartnerAccess('123456789', granted);
      assert.equal(result.ok, true);
      const request = calls.at(-1);
      assert.equal(request.url, '/twitch/api/admin/partner-access');
      assert.equal(request.init.method, 'PUT');
      assert.equal(request.init.headers['X-CSRF-Token'], 'test-csrf');
      assert.deepEqual(JSON.parse(request.init.body), { twitch_user_id: '123456789', granted });
    }
  } finally {
    globalThis.fetch = originalFetch;
  }
});

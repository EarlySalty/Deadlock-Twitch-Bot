import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { test } from 'node:test';
import { LANGUAGE_HEADER, withLanguageHeader } from '../src/api/languageHeader';
import { activeLocale, getActiveLanguage, setActiveLanguage, tr } from '../src/i18n/dictionary';
import { formatNumber } from '../src/utils/formatters';

const globals = globalThis as typeof globalThis & { window: Window };

test('Language headers preserve options, bodies and existing headers without mutating inputs', () => {
  const original = globals.window;
  globals.window = { location: { origin: 'http://127.0.0.1:12345' } } as Window;
  try {
    setActiveLanguage('en');
    const controller = new AbortController();
    for (const headers of [
      new Headers({ 'X-CSRF': 'synthetic', Accept: 'application/json' }),
      { 'X-CSRF': 'synthetic', Accept: 'application/json' },
      [
        ['X-CSRF', 'synthetic'],
        ['Accept', 'application/json'],
      ] as [string, string][],
    ]) {
      const init: RequestInit = {
        method: 'POST',
        credentials: 'same-origin',
        headers,
        body: '{"enabled":false}',
        signal: controller.signal,
        keepalive: true,
        cache: 'no-store',
      };
      const before = [...new Headers(headers)];
      const next = withLanguageHeader('/twitch/api/v2/streamer/example', init);
      assert.notEqual(next, init);
      assert.deepEqual({ ...next, headers: undefined }, { ...init, headers: undefined });
      assert.deepEqual([...new Headers(headers)], before);
      const actual = new Headers(next.headers);
      assert.equal(actual.get(LANGUAGE_HEADER), 'en');
      assert.equal(actual.get('X-CSRF'), 'synthetic');
      assert.equal(actual.get('Accept'), 'application/json');
    }
  } finally {
    globals.window = original;
    setActiveLanguage('de');
  }
});

test('Foreign origins and Request objects keep their exact caller boundary', () => {
  const original = globals.window;
  globals.window = { location: { origin: 'http://127.0.0.1:12345' } } as Window;
  try {
    const init = { headers: { 'X-Caller': 'synthetic' } };
    for (const input of [
      'https://example.invalid/twitch/api/v2',
      '//example.invalid/path',
      'http://127.0.0.1:54321/path',
      new URL('https://example.invalid/path'),
      new Request('http://127.0.0.1:12345/path', { headers: { 'X-Request': 'kept' } }),
    ]) {
      assert.equal(withLanguageHeader(input, init), init);
    }
    assert.equal(
      new Headers(withLanguageHeader(new URL('http://127.0.0.1:12345/path')).headers).get(
        LANGUAGE_HEADER
      ),
      'de'
    );
  } finally {
    globals.window = original;
  }
});

test('Missing browser keeps header helper inert and explicit locale leaves timezone unchanged', () => {
  const original = globals.window;
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  try {
    Reflect.deleteProperty(globalThis, 'window');
    const init = { method: 'GET' };
    assert.equal(withLanguageHeader('/twitch/api/v2', init), init);
    setActiveLanguage('en');
    assert.equal(getActiveLanguage(), 'en');
    assert.equal(activeLocale(), 'en-US');
    assert.equal(
      formatNumber(1234.5, 1),
      (1234.5).toLocaleString('en-US', { minimumFractionDigits: 1, maximumFractionDigits: 1 })
    );
    assert.equal(tr('Einstellungen'), 'Settings');
    setActiveLanguage('de');
    assert.equal(activeLocale(), 'de-DE');
    assert.equal(
      formatNumber(1234.5, 1),
      (1234.5).toLocaleString('de-DE', { minimumFractionDigits: 1, maximumFractionDigits: 1 })
    );
    assert.equal(Intl.DateTimeFormat().resolvedOptions().timeZone, timezone);
  } finally {
    globals.window = original;
    setActiveLanguage('de');
  }
});

test('Real loopback dispatch preserves JSON, HTTP status, abort and presentation-only language', async () => {
  const requests: Array<{
    url?: string;
    method?: string;
    language?: string | string[];
    body: string;
  }> = [];
  const content = {
    title: 'Einstellungen',
    description: 'Verbindungen',
    caption: 'VOD-Archiv',
    answer: 'Speichern',
  };
  const server = createServer(async (req, res) => {
    let body = '';
    for await (const chunk of req) body += chunk;
    requests.push({
      url: req.url,
      method: req.method,
      language: req.headers['x-dashboard-language'],
      body,
    });
    res.setHeader('Content-Type', 'application/json');
    if (req.url === '/failure') {
      res.writeHead(403);
      res.end(JSON.stringify({ error: 'opaque_backend_code', message: 'Original {user} content' }));
    } else {
      res.end(JSON.stringify(content));
    }
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  assert.ok(address && typeof address === 'object');
  const origin = `http://127.0.0.1:${address.port}`;
  const original = globals.window;
  globals.window = {
    location: { origin, pathname: '/twitch/dashboard', hostname: '127.0.0.1' },
  } as Window;
  try {
    const { fetchJson, ApiHttpError } = await import('../src/api/core');
    setActiveLanguage('en');
    const body = JSON.stringify({ message: 'Einstellungen', enabled: false });
    assert.deepEqual(
      await fetchJson(`${origin}/success`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
      }),
      content
    );
    assert.deepEqual(requests[0], { url: '/success', method: 'POST', language: 'en', body });
    await assert.rejects(
      fetchJson(`${origin}/failure`),
      (error) =>
        error instanceof ApiHttpError &&
        error.status === 403 &&
        error.message === 'Original {user} content'
    );
    setActiveLanguage('de');
    assert.deepEqual(await fetchJson(`${origin}/success`), content);
    assert.equal(requests.at(-1)?.language, 'de');
    const before = requests.length;
    await assert.rejects(fetchJson(`${origin}/success`, { signal: AbortSignal.abort() }), {
      name: 'AbortError',
    });
    assert.equal(requests.length, before);
  } finally {
    globals.window = original;
    setActiveLanguage('de');
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
  }
});

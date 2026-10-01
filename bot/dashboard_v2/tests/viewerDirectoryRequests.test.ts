import assert from 'node:assert/strict';
import test from 'node:test';

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: { location: new URL('https://dashboard.example/analyse') },
});
const { fetchApi } = await import('../src/api/core');
const { fetchViewerDirectory } = await import('../src/api/analytics');

function abortableFetch(_input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  return new Promise((_, reject) => {
    const signal = init?.signal;
    if (signal?.aborted) reject(signal.reason);
    else signal?.addEventListener('abort', () => reject(signal.reason), { once: true });
  });
}

test('Viewer-Anfrage reicht das Query-Abbruchsignal bis fetch durch', async t => {
  const controller = new AbortController();
  t.mock.method(globalThis, 'fetch', async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input));
    assert.equal(url.searchParams.get('search'), 'talival');
    assert.equal(url.searchParams.get('page'), '1');
    assert.equal(init?.credentials, 'same-origin');
    assert.equal(init?.signal, controller.signal);
    return new Response('{}', { headers: { 'content-type': 'application/json' } });
  });
  await fetchViewerDirectory('test_channel', 30, 'sessions', 'desc', 'all', 'talival', 1, 50, controller.signal);
});

test('Externer Abbruch greift auch zusammen mit einem Timeout', async t => {
  t.mock.method(globalThis, 'fetch', abortableFetch);
  const controller = new AbortController();
  const reason = new DOMException('Überholte Suche', 'AbortError');
  const request = fetchApi('/viewer-directory', {}, 100, controller.signal);
  controller.abort(reason);
  await assert.rejects(request, error => error === reason);
});

test('Ein bereits abgebrochenes Signal wird nicht ignoriert', async t => {
  const reason = new DOMException('Ansicht geschlossen', 'AbortError');
  t.mock.method(globalThis, 'fetch', async (_input: RequestInfo | URL, init?: RequestInit) => {
    init?.signal?.throwIfAborted();
    return new Response('{}');
  });
  await assert.rejects(fetchApi('/viewer-directory', {}, 100, AbortSignal.abort(reason)), error => error === reason);
});

test('Bestehende Timeout-Aufrufe brechen weiterhin ab', async t => {
  t.mock.method(globalThis, 'fetch', abortableFetch);
  await assert.rejects(fetchApi('/viewer-directory', {}, 5), { name: 'AbortError' });
});

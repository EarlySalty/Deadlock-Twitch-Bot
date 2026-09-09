import test from 'node:test';
import assert from 'node:assert/strict';
Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: new URL('https://example.com/twitch/feedback') } });
const { feedbackHref, feedbackStatuses, submissionAttempt, validFeedbackResultPath } = await import('../src/api/feedback');

test('Timeout-Wiederholung behält Sende-ID; geänderter Text bekommt eigene ID', () => {
  const first = submissionAttempt({ title: 'Kritik' }, undefined, () => 'eins');
  assert.equal(submissionAttempt({ title: 'Kritik' }, first, () => 'zwei').requestId, 'eins');
  assert.equal(submissionAttempt({ title: 'Neuer Wunsch' }, first, () => 'zwei').requestId, 'zwei');
});
test('Feature-Wünsche bieten Beantwortet nicht als Status an', () => {
  assert.ok(feedbackStatuses('feedback').includes('answered'));
  assert.ok(!feedbackStatuses('feature').includes('answered'));
});
test('Box-Link kodiert Bereich und übernimmt keine fremde Zieladresse', () => {
  assert.equal(feedbackHref(), '/twitch/feedback');
  const link = new URL(feedbackHref('feature', 'Chat & Bot'), 'https://example.com');
  assert.equal(link.pathname, '/twitch/feedback');
  assert.equal(link.searchParams.get('bereich'), 'Chat & Bot');
});
test('Funktionslinks erlauben echte Seiten und keine privaten Pfade oder Tokens', () => {
  assert.ok(validFeedbackResultPath('/twitch/verwaltung#overlay'));
  for (const path of ['//evil.org', 'javascript:alert(1)', '/twitch/feedback/2', '/twitch/verwaltung?token=x', '/twitch/verwaltung/../feedback', '/twitch/verwaltung%2f', '/twitch/verwaltung#a#b']) {
    assert.ok(!validFeedbackResultPath(path), path);
  }
});

test('Einreichung nutzt vorhandene Cookies/CSRF und bewahrt dieselbe ID beim erneuten Senden', async () => {
  const { submitFeedback, fetchFeedback } = await import('../src/api/feedback');
  const originalFetch = globalThis.fetch;
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return new Response(JSON.stringify(init?.method === 'POST' ? { id: 7, saved: true } : { items: [], next: null }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  }) as typeof fetch;
  try {
    const payload = { kind: 'feedback' as const, title: 'Mein Hinweis', body: 'Hier fehlt mir eine Erklärung.', area: 'Verwaltung' };
    await submitFeedback(payload, 'stabile-id', 'csrf-test');
    await submitFeedback(payload, 'stabile-id', 'csrf-test');
    assert.equal(calls[0].init?.credentials, 'same-origin');
    assert.equal(new Headers(calls[0].init?.headers).get('X-CSRF-Token'), 'csrf-test');
    assert.equal(JSON.parse(String(calls[0].init?.body)).request_id, JSON.parse(String(calls[1].init?.body)).request_id);
    assert.equal(JSON.parse(String(calls[0].init?.body)).owner_id, undefined);
    await fetchFeedback(false);
    assert.equal(new URL(calls[2].url).searchParams.get('inbox'), 'false');
    assert.equal(new URL(calls[2].url).searchParams.get('streamer'), null);
  } finally { globalThis.fetch = originalFetch; }
});

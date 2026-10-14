import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chooseUsableVersion } from '../src/components/socialmedia/ai/versions';
import { failureExplanation, editorErrorExplanation } from '../src/components/socialmedia/ai/failure';
import { dictionaryFor, translate } from '../src/i18n/dictionary';
import { SOCIAL_EN } from '../src/i18n/en/social';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { NEUTRAL_TEXTS } from '../tools/i18nCoverageData';
import { aiPreviewJob } from '../src/components/socialmedia/ai/preview';

test('social presentation dictionaries cover fixed labels and preserve interpolation parameters', () => {
  const en = dictionaryFor('en');
  for (const key of Object.keys(SOCIAL_EN)) {
    assert.ok(Object.hasOwn(en, key), key);
    const value = en[key];
    assert.ok(value.length > 0, key);
    assert.deepEqual([...key.matchAll(/\{(\w+)\}/g)].map((item) => item[1]).sort(), [...value.matchAll(/\{(\w+)\}/g)].map((item) => item[1]).sort());
  }
  for (const path of [
    'ai/AiEditorPanel.tsx', 'ai/EditorOptionsPanel.tsx', 'ai/RevisionControls.tsx', 'ai/failure.ts',
    'autopilot/import-pool/ImportPoolPanel.tsx', 'autopilot/import-pool/ImportProgress.tsx', 'autopilot/import-pool/presentation.ts',
  ]) {
    const source = readFileSync(new URL(`../src/components/socialmedia/${path}`, import.meta.url), 'utf8');
    const tree = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    const visit = (node: ts.Node) => {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 't' && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) {
        const key = node.arguments[0].text;
        if (/[a-zäöüß]/i.test(key) && key !== 'Clip') assert.ok(Object.hasOwn(en, key), `${path}: ${key}`);
      }
      if (ts.isStringLiteral(node) && !NEUTRAL_TEXTS.has(node.text) && (/^[A-ZÄÖÜ]/.test(node.text) || /[ÄÖÜäöüß]/.test(node.text) || /\b(wird|werden|nicht|keine|bitte|für|und|auf|noch|nur|Die|Der)\b/.test(node.text)) && !node.text.startsWith('social-ai') && !node.text.startsWith('studio-')) {
        assert.ok(Object.hasOwn(en, node.text), `${path}: ${node.text}`);
      }
      ts.forEachChild(node, visit);
    };
    visit(tree);
  }
});

test('editor action errors translate approved codes and do not expose arbitrary server text', () => {
  for (const language of ['de', 'en'] as const) {
    const t = (text: string, params?: Record<string, string | number>) => translate(language, text, params);
    const error = Object.assign(new Error('admin_required'), { code: 'admin_required' });
    assert.equal(editorErrorExplanation(error, t), t('Dafür fehlt deinem Zugang die Berechtigung.'));
    assert.equal(editorErrorExplanation(new Error('opaque-server-text'), t), editorErrorExplanation(null, t));
    for (const code of ['rendering_failed', 'consent_missing', 'provider_unavailable']) {
      const key = failureExplanation(code);
      assert.ok(Object.hasOwn(dictionaryFor('en'), key));
      assert.equal(editorErrorExplanation(Object.assign(new Error(code), { code }), t), t(key));
    }
  }
});

Object.assign(globalThis, { window: { location: { pathname: '/' } } });
const { findChannelClip, isSelectableClip } = await import('../src/components/socialmedia/ai/clipLookup');
const { decodeAiConsent, decodeAiJob, fetchAiConsent, fetchAiJob, fetchAiJobs, retryAiJob, saveAiConsent, startAiJob } = await import('../src/components/socialmedia/ai/api');

function mockFetch(body: unknown, inspect: (url: string, init: RequestInit) => void): () => void {
  const previous = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    inspect(String(input), init);
    return Response.json(body);
  };
  return () => { globalThis.fetch = previous; };
}

test('consent requires an explicit per-channel PUT and never invents a default', async () => {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  const restore = mockFetch({ external_ai_consent: false, global_allowed: false }, (url, init) => calls.push({ url, init }));
  try {
    assert.deepEqual(await fetchAiConsent('11 22'), { external_ai_consent: false, global_allowed: false });
    await saveAiConsent('11 22', true);
    assert.equal(calls[0].url, '/twitch/social-media/api/ai/consent?twitch_user_id=11+22');
    assert.equal(calls[0].init.credentials, 'same-origin');
    assert.equal(calls[1].url, '/twitch/social-media/api/ai/consent');
    assert.equal(calls[1].init.method, 'PUT');
    assert.deepEqual(JSON.parse(String(calls[1].init.body)), { twitch_user_id: '11 22', external_ai_consent: true });
    assert.throws(() => decodeAiConsent({ global_allowed: true }), /unvollständig/);
    assert.throws(() => decodeAiConsent({ external_ai_consent: 'true', global_allowed: true }), /unvollständig/);
  } finally {
    restore();
  }
});

test('manual jobs and failed-stage retry send exactly the scoped API payloads', async () => {
  const validJob = aiPreviewJob('review');
  assert.ok(validJob);
  const calls: Array<{ url: string; init: RequestInit }> = [];
  const previous = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    const url = String(input);
    calls.push({ url, init });
    if (url.endsWith('/jobs') && init.method === 'POST') return Response.json({ job: validJob });
    if (url.endsWith('/retry')) return Response.json({ job: validJob });
    if (url.includes('/jobs/1101?')) return Response.json(validJob);
    return Response.json({ items: [validJob] });
  };
  try {
    assert.equal((await fetchAiJobs('22'))[0].id, 1101);
    assert.equal((await startAiJob('22', 501)).clip_db_id, 501);
    assert.equal((await fetchAiJob('22', 1101)).id, 1101);
    assert.equal((await retryAiJob('22', 1101)).id, 1101);
    assert.deepEqual(calls.map((call) => [call.url, call.init.method ?? 'GET']), [
      ['/twitch/social-media/api/ai/jobs?twitch_user_id=22', 'GET'],
      ['/twitch/social-media/api/ai/jobs', 'POST'],
      ['/twitch/social-media/api/ai/jobs/1101?twitch_user_id=22', 'GET'],
      ['/twitch/social-media/api/ai/jobs/1101/retry', 'POST'],
    ]);
    assert.deepEqual(JSON.parse(String(calls[1].init.body)), { twitch_user_id: '22', clip_db_id: 501 });
    assert.deepEqual(JSON.parse(String(calls[3].init.body)), { twitch_user_id: '22' });
    assert.ok(calls.every((call) => call.init.credentials === 'same-origin'));
  } finally {
    globalThis.fetch = previous;
  }
});

test('malformed status and non-backend media URLs fail closed', () => {
  const valid = aiPreviewJob('review');
  assert.ok(valid);
  assert.equal(decodeAiJob(valid).status, 'review');
  assert.throws(() => decodeAiJob({ ...valid, status: 'published' }), /unvollständig/);
  assert.throws(() => decodeAiJob({ ...valid, cost_basis: 'free' }), /unvollständig/);
  assert.throws(() => decodeAiJob({ ...valid, original_url: '/var/lib/clips/test.mp4' }), /unvollständig/);
  assert.throws(() => decodeAiJob({ ...valid, original_url: '//other.example/video' }), /unvollständig/);
  assert.throws(() => decodeAiJob({ ...valid, versions: [{ ...valid.versions[0], file_url: 'https://other.example/video' }] }), /unvollständig/);
  assert.throws(() => decodeAiJob({ ...valid, highlights: [{ start_ms: 10, end_ms: 5, description: 'Falsche Zeit' }] }), /unvollständig/);
});

test('provider latency decodes independently without inventing a duration', () => {
  const valid = aiPreviewJob('review');
  assert.ok(valid);
  const decoded = decodeAiJob(valid);
  assert.equal(decoded.provider_latency_ms, 78200);
  assert.equal(decoded.processing_ms, 183000);
  assert.equal(decoded.rendering_ms, 42900);
  assert.equal(decodeAiJob({ ...valid, provider_latency_ms: 0 }).provider_latency_ms, 0);
  assert.equal(decodeAiJob({ ...valid, provider_latency_ms: null }).provider_latency_ms, null);
  const legacy = { ...valid };
  delete (legacy as Partial<typeof valid>).provider_latency_ms;
  assert.equal(decodeAiJob(legacy).provider_latency_ms, null);
  for (const value of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, NaN, Infinity, '78200']) {
    assert.throws(() => decodeAiJob({ ...valid, provider_latency_ms: value }), /unvollständig/);
  }
});

test('denied or mismatched channel data cannot become an enabled editor state', async () => {
  const previous = globalThis.fetch;
  const validJob = aiPreviewJob('review');
  assert.ok(validJob);
  globalThis.fetch = async () => new Response(JSON.stringify({ error: 'channel_denied' }), { status: 403 });
  try {
    await assert.rejects(fetchAiConsent('other-channel'));
    await assert.rejects(fetchAiJobs('other-channel'));
  } finally {
    globalThis.fetch = previous;
  }
  const restore = mockFetch({ ...validJob, id: 9999 }, () => {});
  try {
    await assert.rejects(fetchAiJob('other-channel', 1101), /unvollständig/);
  } finally {
    restore();
  }
});

test('channel-scoped pagination resolves a valid handoff beyond the first 30 clips', async () => {
  const previous = globalThis.fetch;
  const calls: string[] = [];
  const controller = new AbortController();
  globalThis.fetch = async (input, init) => {
    const url = new URL(String(input), 'http://localhost');
    calls.push(url.search);
    assert.equal(url.searchParams.get('twitch_user_id'), '123');
    assert.equal(url.searchParams.get('page_size'), '100');
    assert.equal(init?.signal, controller.signal);
    const page = Number(url.searchParams.get('page'));
    return Response.json({ page, page_size: 100, total: 140, items: page === 1 ? Array.from({ length: 100 }, (_, index) => ({ clip_db_id: index + 1, source_kind: 'twitch', title: 'Clip', created_at: '2026-10-11T08:00:00Z' })) : [{ clip_db_id: 135, source_kind: 'twitch', title: 'Übernommener Clip', created_at: '2026-10-11T08:00:00Z' }] });
  };
  try {
    const result = await findChannelClip('123', 135, controller.signal);
    assert.equal(result.kind, 'found');
    if (result.kind === 'found') assert.equal(result.clip.clip_db_id, 135);
    assert.equal(calls.length, 2);
  } finally {
    globalThis.fetch = previous;
  }
});

test('foreign, malformed and unproven handoffs never become selectable', async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async (input) => {
    const url = new URL(String(input), 'http://localhost');
    assert.equal(url.searchParams.get('twitch_user_id'), '123');
    return Response.json({ page: 1, page_size: 100, total: 2, items: [
      { clip_db_id: 77, source_kind: 'upload', title: 'Falsche Quelle', created_at: '2026-10-11T08:00:00Z' },
      { clip_db_id: 78, source_kind: 'twitch', title: 'Kaputt', created_at: 'invalid' },
    ] });
  };
  try {
    assert.deepEqual(await findChannelClip('123', 99), { kind: 'not_found' });
    assert.deepEqual(await findChannelClip('123', 77), { kind: 'not_found' });
    assert.deepEqual(await findChannelClip('123', 78), { kind: 'not_found' });
    assert.deepEqual(await findChannelClip('123', -1), { kind: 'not_found' });
    assert.equal(isSelectableClip({ clip_db_id: 99, source_kind: 'twitch', created_at: 'invalid', title: 'Fremd' } as never), false);
  } finally {
    globalThis.fetch = previous;
  }
  const restore = mockFetch({ page: 5, page_size: 100, total: 100, items: [{ clip_db_id: 99, source_kind: 'twitch', title: 'Fremd', created_at: '2026-10-11T08:00:00Z' }] }, () => {});
  try {
    await assert.rejects(findChannelClip('123', 99), /nicht geprüft/);
  } finally {
    restore();
  }
});

test('bounded handoff search distinguishes a partial archive from a confirmed absence', async () => {
  const previous = globalThis.fetch;
  const pages: number[] = [];
  globalThis.fetch = async (input) => {
    const url = new URL(String(input), 'http://localhost');
    assert.equal(url.searchParams.get('twitch_user_id'), '123');
    const page = Number(url.searchParams.get('page'));
    pages.push(page);
    return Response.json({ page, page_size: 100, total: 2100, items: Array.from({ length: 100 }, (_, index) => ({ clip_db_id: (page - 1) * 100 + index + 1, source_kind: 'twitch', title: 'Clip', created_at: '2026-10-11T08:00:00Z' })) });
  };
  try {
    assert.deepEqual(await findChannelClip('123', 2099), { kind: 'limit_reached' });
    assert.deepEqual(pages, Array.from({ length: 20 }, (_, index) => index + 1));
    pages.length = 0;
    const result = await findChannelClip('123', 2000);
    assert.equal(result.kind, 'found');
    if (result.kind === 'found') assert.equal(result.clip.clip_db_id, 2000);
    assert.equal(pages.length, 20);
  } finally {
    globalThis.fetch = previous;
  }
});

test('failure explanation uses only approved codes and ignores arbitrary provider text', () => {
  assert.match(failureExplanation('rendering_failed'), /Video/);
  assert.match(failureExplanation('render_failed'), /Video/);
  assert.match(failureExplanation('consent_missing'), /Freigabe/);
  assert.match(failureExplanation(null), /erneut/);
  const unsafe = 'Die Prüfung ist fehlgeschlagen. Zugangscode geheim123 wurde an /var/lib/private übertragen.';
  assert.equal(failureExplanation(unsafe), failureExplanation(null));
  assert.equal(failureExplanation('__proto__'), failureExplanation(null));
  assert.doesNotMatch(failureExplanation('Error: /var/lib/private/stack'), /private|stack|Error/);
});

test('selecting an older output does not make a playable newest version unusable', () => {
  const job = aiPreviewJob('review');
  assert.ok(job);
  const playable = job.versions.map((version) => ({ ...version, status: 'review', file_url: job.versions[0].file_url }));
  const newest = [...playable].sort((a, b) => b.version_no - a.version_no)[0];
  assert.equal(chooseUsableVersion(playable, 2201)?.id, 2201);
  assert.equal(chooseUsableVersion([newest])?.id, 2202);
  const unavailable = { ...newest, file_url: null };
  assert.equal(chooseUsableVersion([unavailable]), null);
  assert.equal(chooseUsableVersion([playable[0], unavailable], 2201)?.id, 2201);
});

test('a failed new version leaves the older playable output selected', () => {
  const job = aiPreviewJob('review');
  assert.ok(job);
  assert.equal(chooseUsableVersion(job.versions)?.version_no, 1);
  assert.equal(chooseUsableVersion(job.versions, 2202)?.id, 2201);
  assert.equal(chooseUsableVersion(job.versions, 2201)?.id, 2201);
  assert.equal(chooseUsableVersion([], 2201), null);
});

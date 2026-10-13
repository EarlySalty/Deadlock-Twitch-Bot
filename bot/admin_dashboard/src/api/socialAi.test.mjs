import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';
import { limits, skills, totals, usage } from './socialAiFixtures.mjs';
import { requireLimits, requireSkills, requireUsage } from './socialAiValidation.ts';
import { costTotal, formatCount, formatDuration, formatMicroUsd, parseMicroUsd, usdInput } from '../utils/socialAi.ts';

const source = await readFile(new URL('./client.ts', import.meta.url), 'utf8');
const compile = source => ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText;
const moduleUrl = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`;
const clientUrl = moduleUrl(compile(source).replace('@/utils/formatters', new URL('../utils/formatters.ts', import.meta.url).href));
const apiSource = await readFile(new URL('./socialAi.ts', import.meta.url), 'utf8');
const api = await import(moduleUrl(compile(apiSource).replace('@/api/client', clientUrl).replace('./socialAiValidation', new URL('./socialAiValidation.ts', import.meta.url).href)));

test('USD-Eingaben bleiben ganzzahlige Mikro-USD ohne Rundungsfehler', () => {
  assert.equal(parseMicroUsd('1,234567'), 1234567);
  assert.equal(parseMicroUsd('0.000001'), 1);
  assert.equal(parseMicroUsd('0'), 0);
  for (const value of ['', '-1', '1e6', '1,1234567', '1.000,00', 'Infinity', '9007199255']) assert.equal(parseMicroUsd(value), null);
  assert.equal(parseMicroUsd(usdInput(1234567)), 1234567);
});

test('Unbekannte und ungültige Messwerte werden nicht zu Null', () => {
  for (const value of [null, undefined, -1, Infinity, NaN]) {
    assert.equal(formatMicroUsd(value), formatMicroUsd(null));
    assert.equal(formatDuration(value), formatDuration(null));
    assert.equal(formatCount(value), formatCount(null));
  }
  assert.notEqual(formatMicroUsd(0), formatMicroUsd(null));
});

test('Unvollständige Kostensummen bleiben unbekannt', () => {
  assert.equal(costTotal(totals, 'estimated_micro_usd'), 15320);
  for (const key of ['estimated_micro_usd', 'measured_micro_usd', 'invoiced_micro_usd']) {
    assert.equal(costTotal({ ...totals, incomplete_attempts: 1 }, key), null);
    assert.equal(costTotal({ ...totals, unknown_cost_attempts: 1 }, key), null);
    assert.equal(costTotal({ ...totals, attempts: 0 }, key), null);
  }
});

test('Usage contract rejects unsafe metrics independently', () => {
  for (const value of [-1, Infinity, NaN, Number.MAX_SAFE_INTEGER + 1]) {
    const invalid = usage('job');
    invalid.groups[0].processing_ms = value;
    assert.throws(() => requireUsage(invalid, 'job', 0));
  }
  const invalid = usage();
  delete invalid.totals.charged_micro_usd;
  assert.throws(() => requireUsage(invalid, 'user', 0));
});

test('Usage-Vertrag verwirft Antworten für eine veraltete Gruppierung oder Seite', () => {
  assert.equal(requireUsage(usage('job', 25), 'job', 25).groups[0].rendering_ms, 6800);
  assert.throws(() => requireUsage(usage('provider'), 'user', 0));
  assert.throws(() => requireUsage(usage('user', 25), 'user', 0));
  assert.throws(() => requireUsage({ ...usage(), totals: { ...totals, unknown_cost_attempts: 3 } }, 'user', 0));
});

test('Limits-Vertrag lässt weder erhöhte globale Caps noch aktive Planregeln zu', () => {
  assert.equal(requireLimits(limits).plan_policies_active, false);
  assert.throws(() => requireLimits({ ...limits, global: { ...limits.global, initial_total_cap_micro_usd: 2000001 } }));
  assert.throws(() => requireLimits({ ...limits, plan_policies_active: true }));
  assert.throws(() => requireLimits({ ...limits, users: [{ ...limits.users[0], monthly_cap_micro_usd: 2000001 }] }));
});

test('Client verwendet Adminpfade, CSRF und erwartete Versionen ohne den Cap zu senden', async () => {
  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    const body = init?.body ? JSON.parse(init.body) : null;
    let response = url.endsWith('/auth-status') ? { authenticated: true, isAdmin: true, csrfToken: 'synthetic-csrf', loginUrl: '/synthetic-login', discordLoginUrl: '/synthetic-login' } : structuredClone(limits);
    if (url.endsWith('/limits/global')) response.global = { ...response.global, revision: 2, paused: body.paused };
    if (url.includes('/usage?')) response = usage('job', 25);
    return new Response(JSON.stringify(response), { headers: { 'Content-Type': 'application/json' } });
  };
  try {
    await api.saveGlobalControl({ expected_revision: 1, paused: true, reason: 'synthetic' });
    const global = calls.at(-1);
    assert.equal(global.url, '/twitch/api/admin/social-ai/limits/global');
    assert.equal(global.init.credentials, 'include');
    assert.equal(global.init.headers['X-CSRF-Token'], 'synthetic-csrf');
    assert.deepEqual(JSON.parse(global.init.body), { expected_revision: 1, paused: true, reason: 'synthetic', csrf_token: 'synthetic-csrf' });
    await api.fetchUsage({ group_by: 'job', offset: 25, twitch_user_id: '', provider: 'test & value', job_id: '' });
    assert.equal(new URL(calls.at(-1).url, 'http://fixture.invalid').searchParams.get('provider'), 'test & value');
  } finally { globalThis.fetch = originalFetch; }
});

test('Nicht bestätigte Schreibantworten werden nicht als gespeicherter Stand übernommen', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify(limits), { headers: { 'Content-Type': 'application/json' } });
  try {
    await assert.rejects(api.saveGlobalControl({ expected_revision: 1, paused: true, reason: 'synthetic' }));
    await assert.rejects(api.saveUserLimit({ expected_revision: 1, twitch_user_id: '100000001', paused: false, monthly_cap_micro_usd: 1000000, reason: 'synthetic' }));
  } finally { globalThis.fetch = originalFetch; }
});

test('Skill-Historie verlangt eine vorhandene aktive Version und gültige Diffzeilen', () => {
  assert.equal(requireSkills(structuredClone(skills)).skills[0].current_version, '2');
  const missing = structuredClone(skills);
  missing.skills[0].current_version = '9';
  assert.throws(() => requireSkills(missing));
  const badDiff = structuredClone(skills);
  badDiff.skills[0].versions[0].diff[0].kind = 'changed';
  assert.throws(() => requireSkills(badDiff));
});

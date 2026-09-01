import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const globalState = globalThis as typeof globalThis & {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
const vorherigesFenster = globalState.window;
globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const { rotateUplinkIngestKey } = await import('../src/api/uplink');
globalState.window = vorherigesFenster;

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');
const UPLINK_API = readFileSync(join(DASHBOARD_ROOT, 'src', 'api', 'uplink.ts'), 'utf8');

test('Schlüsselrotation sendet Cookie, CSRF und genau einen leeren JSON-Rumpf', async () => {
  const vorher = globalThis.fetch;
  let aufruf: { input: RequestInfo | URL; init?: RequestInit } | undefined;
  let aufrufe = 0;
  globalThis.fetch = async (input, init) => {
    aufrufe += 1;
    aufruf = { input, init };
    return new Response(JSON.stringify({ srt_hint: 'srt://example.invalid:8899?streamid=dummy' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const antwort = await rotateUplinkIngestKey('csrf-dummy', '11111111-1111-4111-8111-111111111111');
    assert.equal(antwort.srt_hint, 'srt://example.invalid:8899?streamid=dummy');
    assert.equal(aufrufe, 1);
    assert.equal(String(aufruf?.input), '/twitch/api/v2/uplink/key/rotate');
    assert.equal(aufruf?.init?.method, 'POST');
    assert.equal(aufruf?.init?.credentials, 'same-origin');
    assert.equal(new Headers(aufruf?.init?.headers).get('X-CSRF-Token'), 'csrf-dummy');
    assert.equal(
      new Headers(aufruf?.init?.headers).get('Idempotency-Key'),
      '11111111-1111-4111-8111-111111111111',
    );
    assert.equal(new Headers(aufruf?.init?.headers).get('Content-Type'), 'application/json');
    assert.equal(aufruf?.init?.cache, 'no-store');
    assert.deepEqual(JSON.parse(String(aufruf?.init?.body)), {});
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Schlüsselrotation akzeptiert keinen Erfolg ohne neue SRT-Adresse', async () => {
  const vorher = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ srt_hint: '   ' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  try {
    await assert.rejects(
      () => rotateUplinkIngestKey('csrf-dummy', '11111111-1111-4111-8111-111111111111'),
      /keine neue SRT-Adresse/i,
    );
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Schlüsselrotation akzeptiert nur eine vollständige SRT-Adresse', async () => {
  const vorher = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ srt_hint: 'https://example.invalid/nicht-srt' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  try {
    await assert.rejects(
      () => rotateUplinkIngestKey('csrf-dummy', '11111111-1111-4111-8111-111111111111'),
      /keine neue SRT-Adresse/i,
    );
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Schlüsselrotation sendet ohne CSRF-Token keinen Request', async () => {
  const vorher = globalThis.fetch;
  let aufgerufen = false;
  globalThis.fetch = async () => {
    aufgerufen = true;
    throw new Error('darf nicht aufgerufen werden');
  };
  try {
    await assert.rejects(
      () => rotateUplinkIngestKey('  ', '11111111-1111-4111-8111-111111111111'),
      /Sicherheitstoken fehlt/i,
    );
    assert.equal(aufgerufen, false);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Schlüsselrotation sendet ohne Rotationskennung keinen Request', async () => {
  const vorher = globalThis.fetch;
  let aufgerufen = false;
  globalThis.fetch = async () => {
    aufgerufen = true;
    throw new Error('darf nicht aufgerufen werden');
  };
  try {
    await assert.rejects(
      () => rotateUplinkIngestKey('csrf-dummy', '  '),
      /Rotationskennung fehlt/i,
    );
    assert.equal(aufgerufen, false);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Rotationsdialog hat sichere Fokus- und Abbruchregeln', () => {
  assert.match(UPLINK_PAGE, /<dialog[\s\S]*?aria-labelledby="uplink-key-rotate-title"/);
  assert.match(UPLINK_PAGE, /sichereAktionRef\.current\?\.focus\(\)/);
  assert.match(UPLINK_PAGE, /onCancel=\{\(ereignis\) => \{[\s\S]*?preventDefault\(\)[\s\S]*?!rotation\.isPending/);
  assert.match(UPLINK_PAGE, /oeffnerRef\.current\?\.focus\(\)/);
  assert.match(UPLINK_PAGE, /disabled=\{rotation\.isPending\}/);
  assert.match(UPLINK_PAGE, /if \(rotationLaeuftRef\.current\) return/);
  assert.match(UPLINK_PAGE, /window\.crypto\.randomUUID\(\)/);
});

test('Rotation erklärt die Folgen und behandelt einen unklaren Ausgang ohne Wiederholung', () => {
  assert.match(UPLINK_PAGE, /Die laufende SRT-Session läuft weiter/);
  assert.match(UPLINK_PAGE, /alte Adresse kann sich danach nicht neu verbinden/);
  assert.match(UPLINK_PAGE, /neue Adresse danach in OBS eintragen/);
  assert.match(UPLINK_PAGE, /retry:\s*false/);
  assert.match(UPLINK_PAGE, /onError:[\s\S]*?srt_hint: ''[\s\S]*?refetchQueries\([\s\S]*?throwOnError: true/);
  assert.match(UPLINK_PAGE, /setRotationUnklar\(true\)/);
  assert.match(UPLINK_PAGE, /disabled=\{!csrfToken \|\| rotationUnklar\}/);
  assert.match(UPLINK_PAGE, /Ergebnis ist unklar/);
  assert.match(UPLINK_PAGE, /role="status"/);
});

test('nach erfolgreicher Rotation wird nur srt_hint im Query-Cache ersetzt', () => {
  assert.match(
    UPLINK_PAGE,
    /setQueryData\(\['uplink-me'\][\s\S]*?alt \? \{ \.\.\.alt, srt_hint: antwort\.srt_hint \} : alt/,
  );
  assert.match(UPLINK_PAGE, /invalidateQueries\(\{ queryKey: \['uplink-me'\] \}\)/);
  assert.doesNotMatch(UPLINK_PAGE, /ingest_key:\s*antwort/);
});

test('Lese- und Rotationspfad verbieten den Browser-Cache', () => {
  assert.match(UPLINK_API, /fetchUplinkMe[\s\S]*?cache:\s*'no-store'/);
  assert.match(UPLINK_API, /rotateUplinkIngestKey[\s\S]*?cache:\s*'no-store'/);
  assert.doesNotMatch(UPLINK_API, /interface UplinkMe[\s\S]*?ingest_key/);
});

test('CopyField verdeckt einen ausgetauschten Wert und verwirft alte Kopiermeldungen', () => {
  assert.match(
    UPLINK_PAGE,
    /useEffect\(\(\) => \{\s*setOffen\(false\);\s*setStand\('ruhe'\);\s*\}, \[value\]\)/,
  );
});

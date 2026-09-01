import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const globalState = globalThis as typeof globalThis & {
  window?: { __TWITCH_DASHBOARD_RUNTIME__?: Record<string, unknown> };
};
const vorherigesFenster = globalState.window;
globalState.window = { __TWITCH_DASHBOARD_RUNTIME__: {} };
const {
  fetchUplinkMe,
  fetchAutoritativenUplinkStand,
  darfAutoritativenStandNachRotationsfehlerLesen,
  hatPendingUplinkRotation,
  istVollstaendigeSrtObsAdresse,
  istPendingUplinkRotation,
  ladePendingUplinkRotation,
  loeschePendingUplinkRotation,
  rotateUplinkIngestKey,
  rotateUplinkIngestKeyNachLeseabbruch,
  speicherePendingUplinkRotation,
} = await import('../src/api/uplink');
const { ApiHttpError } = await import('../src/api/core');
globalState.window = vorherigesFenster;

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');
const UPLINK_API = readFileSync(join(DASHBOARD_ROOT, 'src', 'api', 'uplink.ts'), 'utf8');
const DUMMY_SRT_OBS_ADRESSE =
  'srt://example.invalid:8899?mode=caller&latency=4000&streamid=rsr_0123456789abcdef0123456789abcdef&passphrase=fedcba9876543210fedcba9876543210&pbkeylen=32';

function testStorage() {
  const werte = new Map<string, string>();
  return {
    getItem: (key: string) => werte.get(key) ?? null,
    setItem: (key: string, wert: string) => void werte.set(key, wert),
    removeItem: (key: string) => void werte.delete(key),
  };
}

test('Schlüsselrotation sendet Cookie, CSRF und genau einen leeren JSON-Rumpf', async () => {
  const vorher = globalThis.fetch;
  let aufruf: { input: RequestInfo | URL; init?: RequestInit } | undefined;
  let aufrufe = 0;
  globalThis.fetch = async (input, init) => {
    aufrufe += 1;
    aufruf = { input, init };
    return new Response(JSON.stringify({ srt_hint: DUMMY_SRT_OBS_ADRESSE }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const antwort = await rotateUplinkIngestKey('csrf-dummy', '11111111-1111-4111-8111-111111111111');
    assert.equal(antwort.srt_hint, DUMMY_SRT_OBS_ADRESSE);
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

test('ein laufender uplink-me GET ist vor dem Rotations-POST beendet', async () => {
  const vorher = globalThis.fetch;
  let postAufrufe = 0;
  let leseabbruchFreigeben!: () => void;
  const leseabbruch = new Promise<void>((resolve) => {
    leseabbruchFreigeben = resolve;
  });
  globalThis.fetch = async () => {
    postAufrufe += 1;
    return new Response(JSON.stringify({ srt_hint: DUMMY_SRT_OBS_ADRESSE }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    const rotation = rotateUplinkIngestKeyNachLeseabbruch(
      'csrf-dummy',
      '11111111-1111-4111-8111-111111111111',
      () => leseabbruch,
    );
    await Promise.resolve();
    assert.equal(postAufrufe, 0, 'POST darf den noch laufenden GET nicht überholen');
    leseabbruchFreigeben();
    await rotation;
    assert.equal(postAufrufe, 1);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('uplink-me reicht das AbortSignal bis fetch durch', async () => {
  const vorher = globalThis.fetch;
  const abort = new AbortController();
  let signal: AbortSignal | null | undefined;
  globalThis.fetch = async (_input, init) => {
    signal = init?.signal;
    return new Response(JSON.stringify({ srt_hint: DUMMY_SRT_OBS_ADRESSE }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    await fetchUplinkMe(abort.signal);
    assert.equal(signal, abort.signal);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('unklarer Ausgang wird ausschließlich mit derselben gespeicherten UUID erneut gesendet', async () => {
  const vorher = globalThis.fetch;
  const storage = testStorage();
  const key = '11111111-1111-4111-8111-111111111111';
  const twitchUserId = '42';
  const gesendeteKeys: string[] = [];
  let aufruf = 0;
  globalThis.fetch = async (_input, init) => {
    gesendeteKeys.push(new Headers(init?.headers).get('Idempotency-Key') ?? '');
    aufruf += 1;
    if (aufruf === 1) {
      return new Response(JSON.stringify({ error: 'Ausgang noch unklar' }), {
        status: 425,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    return new Response(JSON.stringify({ srt_hint: DUMMY_SRT_OBS_ADRESSE }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    speicherePendingUplinkRotation(twitchUserId, key, storage);
    await assert.rejects(() =>
      rotateUplinkIngestKey(
        'csrf-dummy',
        ladePendingUplinkRotation(twitchUserId, storage)!,
      ),
    );
    const antwort = await rotateUplinkIngestKey(
      'csrf-dummy',
      ladePendingUplinkRotation(twitchUserId, storage)!,
    );
    assert.equal(antwort.srt_hint, DUMMY_SRT_OBS_ADRESSE);
    assert.deepEqual(gesendeteKeys, [key, key]);
    loeschePendingUplinkRotation(twitchUserId, key, storage);
    assert.equal(ladePendingUplinkRotation(twitchUserId, storage), null);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Reload übernimmt nur eine gültige pending UUID und hält die Adresse aus dem DOM', () => {
  const storage = testStorage();
  const key = '11111111-1111-4111-8111-111111111111';
  speicherePendingUplinkRotation('42', key, storage);
  assert.equal(hatPendingUplinkRotation(storage), true);
  assert.equal(ladePendingUplinkRotation('42', storage), key);
  assert.match(UPLINK_PAGE, /useState\(\s*\(\) => hatPendingUplinkRotation\(\)/);
  assert.match(UPLINK_PAGE, /enabled: Boolean\(twitchUserId\) && !ingestRotationUnklar/);
  assert.match(UPLINK_PAGE, /ingestRotationUnklar && !data \? \([\s\S]*?<IngestKeyRotation/);
  assert.match(UPLINK_PAGE, /const pending = ladePendingUplinkRotation\(twitchUserId\);[\s\S]*?auftragStarten\(pending\)/);
  assert.match(UPLINK_PAGE, /value=\{ingestRotationUnklar \? '' : data\.srt_hint\}/);
});

test('frischer Reload reconciled die Pending-UUID vor dem ersten me-GET', async () => {
  const vorher = globalThis.fetch;
  const storage = testStorage();
  const key = '11111111-1111-4111-8111-111111111111';
  const reihenfolge: string[] = [];
  speicherePendingUplinkRotation('42', key, storage);
  globalThis.fetch = async (input, init) => {
    reihenfolge.push(`${init?.method ?? 'GET'} ${String(input)}`);
    if (init?.method === 'POST') {
      assert.equal(new Headers(init.headers).get('Idempotency-Key'), key);
      return new Response(JSON.stringify({ srt_hint: DUMMY_SRT_OBS_ADRESSE }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }
    return new Response(JSON.stringify({
      enabled: true,
      waitlisted: false,
      rtmp_url: '',
      srt_hint: DUMMY_SRT_OBS_ADRESSE,
      reconnect_wait_s: 0,
      reconnect_wait_max_s: 30,
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  };
  try {
    assert.equal(hatPendingUplinkRotation(storage), true);
    const pending = ladePendingUplinkRotation('42', storage);
    await rotateUplinkIngestKey('csrf-dummy', pending!);
    loeschePendingUplinkRotation('42', key, storage);
    await fetchUplinkMe();
    assert.deepEqual(reihenfolge, [
      'POST /twitch/api/v2/uplink/key/rotate',
      'GET /twitch/api/v2/uplink/me',
    ]);
  } finally {
    globalThis.fetch = vorher;
  }
});

test('Pending UUID ist an die authentifizierte Twitch-ID gebunden', () => {
  const storage = testStorage();
  const key = '11111111-1111-4111-8111-111111111111';
  speicherePendingUplinkRotation('42', key, storage);
  assert.equal(ladePendingUplinkRotation('99', storage), null);
  assert.equal(hatPendingUplinkRotation(storage), false, 'fremde UUID muss entfernt sein');
  assert.match(
    UPLINK_PAGE,
    /speicherePendingUplinkRotation\(auftrag\.twitchUserId, auftrag\.idempotenz\)/,
  );
  assert.match(
    UPLINK_PAGE,
    /const pending = ladePendingUplinkRotation\(twitchUserId\);\s*setIngestRotationUnklar\(pending !== null\)/,
    'bestätigter Accountwechsel muss den fremden Pending-Zustand entsperren',
  );
});

test('verspäteter A-Callback löscht nach B-Start weder B-Pending noch dessen UI-Generation', () => {
  const storage = testStorage();
  const keyA = '11111111-1111-4111-8111-111111111111';
  const keyB = '22222222-2222-4222-8222-222222222222';
  speicherePendingUplinkRotation('42', keyA, storage);
  speicherePendingUplinkRotation('99', keyB, storage);

  assert.equal(loeschePendingUplinkRotation('42', keyA, storage), false);
  assert.equal(istPendingUplinkRotation('99', keyB, storage), true);
  assert.match(UPLINK_PAGE, /if \(!istAktuell\(auftrag\)\) return;[\s\S]*?setQueryData/);
  assert.match(UPLINK_PAGE, /generationRef\.current === auftrag\.generation/);
  assert.match(UPLINK_PAGE, /key=\{`(?:pending|bereit)-\$\{twitchUserId/);
});

test('gesperrtes sessionStorage bricht Render und terminales Aufräumen nicht ab', () => {
  const vorher = globalState.window;
  globalState.window = Object.defineProperty({}, 'sessionStorage', {
    get() {
      throw new DOMException('dummy', 'SecurityError');
    },
  });
  try {
    assert.equal(hatPendingUplinkRotation(), false);
    assert.equal(ladePendingUplinkRotation('42'), null);
    assert.doesNotThrow(() =>
      loeschePendingUplinkRotation('42', '11111111-1111-4111-8111-111111111111'),
    );
    assert.throws(
      () => speicherePendingUplinkRotation('42', '11111111-1111-4111-8111-111111111111'),
      /Browser-Speicher ist gesperrt/,
    );
  } finally {
    globalState.window = vorher;
  }
});

test('autoritiver Recovery-GET hat Abort-Frist und verlangt eine vollständige SRT-Adresse', async () => {
  const vorher = globalThis.fetch;
  globalThis.fetch = async (_input, init) =>
    new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new DOMException('dummy', 'AbortError')));
    });
  try {
    await assert.rejects(() => fetchAutoritativenUplinkStand(5), /Abort|aborted/i);
  } finally {
    globalThis.fetch = vorher;
  }

  globalThis.fetch = async () =>
    new Response(JSON.stringify({ srt_hint: 'srt://example.invalid:8899?mode=caller' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  try {
    await assert.rejects(
      () => fetchAutoritativenUplinkStand(50),
      /keine vollständige aktuelle OBS-Adresse/i,
    );
  } finally {
    globalThis.fetch = vorher;
  }
});

test('unklare Ausgänge dürfen nach Dashboard-Restart keinen alten GET freigeben', () => {
  for (const status of [408, 425, 500, 502, 503]) {
    assert.equal(
      darfAutoritativenStandNachRotationsfehlerLesen(new ApiHttpError('dummy', status)),
      false,
      `HTTP ${status} muss ausschließlich dieselbe UUID replayen`,
    );
  }
  assert.equal(darfAutoritativenStandNachRotationsfehlerLesen(new Error('Transport')), false);
  for (const status of [400, 403, 404, 409, 422, 429]) {
    assert.equal(
      darfAutoritativenStandNachRotationsfehlerLesen(new ApiHttpError('dummy', status)),
      true,
      `HTTP ${status} ist ein nachweislich terminaler Nicht-Schreibausgang`,
    );
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

test('Schlüsselrotation lehnt eine schlüssellose SRT-Adresse ab', async () => {
  const vorher = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ srt_hint: 'srt://example.invalid' }), {
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

test('Schlüsselrotation verlangt StreamID, Passphrase und Verschlüsselungsparameter', async () => {
  for (const adresse of [
    'srt://example.invalid:8899?mode=caller&latency=4000',
    'srt://example.invalid:8899?mode=caller&latency=4000&streamid=rsr_0123456789abcdef0123456789abcdef',
    'srt://example.invalid:8899?mode=caller&latency=4000&streamid=rsr_0123456789abcdef0123456789abcdef&passphrase=fedcba9876543210fedcba9876543210',
  ]) {
    assert.equal(istVollstaendigeSrtObsAdresse(adresse), false);
  }
  assert.equal(istVollstaendigeSrtObsAdresse(DUMMY_SRT_OBS_ADRESSE), true);
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

test('Rotation erklärt die Folgen und wiederholt bei unklarem Ausgang nur dieselbe UUID', () => {
  assert.match(UPLINK_PAGE, /Die laufende SRT-Session läuft weiter/);
  assert.match(UPLINK_PAGE, /alte Adresse kann sich danach nicht neu verbinden/);
  assert.match(UPLINK_PAGE, /neue Adresse danach in OBS eintragen/);
  assert.match(UPLINK_PAGE, /retry:\s*false/);
  assert.match(
    UPLINK_PAGE,
    /onError:[\s\S]*?darfAutoritativenStandNachRotationsfehlerLesen\(fehler\)[\s\S]*?if \(!darfSicherLesen\)[\s\S]*?return;[\s\S]*?await fetchAutoritativenUplinkStand\(\)/,
  );
  assert.match(UPLINK_PAGE, /setRotationUnklar\(true\)/);
  assert.match(
    UPLINK_PAGE,
    /value=\{ingestRotationUnklar \? '' : data\.srt_hint\}/,
    'bei unklarem Ausgang darf die alte Adresse nicht im DOM bleiben',
  );
  assert.match(UPLINK_PAGE, /disabled=\{!csrfToken \|\| !twitchUserId \|\| rotationUnklar\}/);
  assert.match(UPLINK_PAGE, /Ergebnis ist noch unklar/);
  assert.match(UPLINK_PAGE, /role="status"/);
  assert.match(UPLINK_PAGE, /Denselben Vorgang erneut abgleichen/);
  assert.match(UPLINK_PAGE, /loeschePendingUplinkRotation\(auftrag\.twitchUserId, auftrag\.idempotenz\)/);
  assert.match(UPLINK_PAGE, /cancelQueries\(\{ queryKey: \['uplink-me', twitchUserId\], exact: true \}\)/);
  assert.match(UPLINK_PAGE, /enabled: Boolean\(twitchUserId\) && !ingestRotationUnklar/);
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
  assert.match(UPLINK_PAGE, /if \(gesperrt\) return/);
  assert.match(UPLINK_PAGE, /disabled=\{gesperrt\}/);
  assert.match(UPLINK_PAGE, /gesperrt=\{ingestRotationUnklar\}/);
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lesezeichenAnleitung } from '../src/utils/browserErkennung';
import { canonicalBookmarkLocation, nextStep, stepState } from '../src/components/onboarding/steps';

test('Desktop erklärt die echte Browser-Aktion mit Tastatur statt einer Sternposition', () => {
  for (const platform of ['windows', 'mac'] as const) {
    const text = lesezeichenAnleitung(platform);
    assert.match(text, /Lesezeichen/);
    assert.match(text, platform === 'mac' ? /⌘.*D/ : /Strg.*D/);
    assert.doesNotMatch(text, /links|rechts|kopieren/i);
  }
});
test('Mobil legt ein Lesezeichen an, keine andere Aktion auf dem Startbildschirm', () => {
  for (const platform of ['ios-safari', 'ios-chrome', 'android-chrome', 'android-firefox', 'other'] as const) {
    const text = lesezeichenAnleitung(platform);
    assert.match(text, /Lesezeichen|Favoriten/);
    assert.doesNotMatch(text, /Home-Bildschirm|Startbildschirm|oben rechts/);
  }
});
test('Lesezeichen nur an kanonischer Adresse ohne OAuth- oder Fragmentreste', () => {
  const canonical = {origin: 'https://deutsche-deadlock-community.de', pathname: '/twitch/dashboard', search: '', hash: ''};
  assert.equal(canonicalBookmarkLocation(canonical), true);
  for (const change of [{search:'?ok=1'}, {search:'?state=private'}, {hash:'#feedback'}, {pathname:'/twitch/verwaltung'}, {origin:'http://localhost'}]) {
    assert.equal(canonicalBookmarkLocation({...canonical, ...change}), false);
  }
});
test('Rundgang hält Discord vor Steam; Aufrufen oder überspringen bestätigt keine Verbindung', () => {
  assert.equal(nextStep('discord'), 'steam');
  const status = {completed_step_ids: ['discord','steam','chat'] as any, discord_status: 'missing', steam_status: 'error'};
  assert.equal(stepState('discord', status), 'open');
  assert.equal(stepState('steam', status), 'error');
  assert.equal(stepState('chat', status), 'done');
  assert.equal(stepState('bookmark', status), 'open');
});

test('Fortschritt sendet Session und CSRF; Pause bestätigt keinen Schritt', async () => {
  const oldWindow = globalThis.window;
  const oldFetch = globalThis.fetch;
  try {
    globalThis.window = {location: {origin: 'https://deutsche-deadlock-community.de', hostname: 'deutsche-deadlock-community.de', pathname: '/twitch/dashboard'}} as Window & typeof globalThis;
    let request: RequestInit | undefined;
    let requestUrl = '';
    globalThis.fetch = (async (url: RequestInfo | URL, init?: RequestInit) => {
      requestUrl = String(url); request = init;
      return new Response(JSON.stringify({ok:true, progress:{paused:true}}), {status:200, headers:{'Content-Type':'application/json'}});
    }) as typeof fetch;
    const {saveOnboardingProgress} = await import('../src/api/onboarding');
    await saveOnboardingProgress({paused:true}, 'test-csrf');
    assert.equal(requestUrl, 'https://deutsche-deadlock-community.de/twitch/api/v2/streamer/onboarding');
    assert.equal(request?.credentials, 'same-origin');
    assert.equal(new Headers(request?.headers).get('X-CSRF-Token'), 'test-csrf');
    assert.deepEqual(JSON.parse(String(request?.body)), {paused:true});
    globalThis.fetch = (async () => new Response(JSON.stringify({error:'Speichern fehlgeschlagen'}), {status:503})) as typeof fetch;
    await assert.rejects(() => saveOnboardingProgress({complete_step:'bookmark'}, 'test-csrf'), /Speichern fehlgeschlagen/);
  } finally { globalThis.fetch = oldFetch; globalThis.window = oldWindow; }
});

test('Pause gewinnt gegen laufendes Weiter und wird als letzter Zustand gespeichert', async () => {
  const { ProgressCoordinator } = await import('../src/components/onboarding/progressCoordinator');
  const writes: unknown[] = [];
  let release!: (value: boolean) => void;
  let navigations = 0;
  const coordinator = new ProgressCoordinator(update => {
    writes.push(update);
    return writes.length === 1 ? new Promise(resolve => { release = resolve; }) : Promise.resolve(true);
  });
  const opening = coordinator.resume({ active_step: 'discord', complete_step: 'bookmark', paused: false }, () => { navigations++; });
  await Promise.resolve();
  const pause = coordinator.pause();
  release(true);
  await Promise.all([opening, pause]);
  assert.equal(navigations, 0);
  assert.deepEqual(writes, [{active_step:'discord',complete_step:'bookmark',paused:false},{paused:true}]);
  await coordinator.resume({active_step:'discord',paused:false}, () => { navigations++; });
  assert.equal(navigations, 1);
});

test('Fehlgeschlagene Anfrage blockiert Pause nicht; Kontowechsel entwertet Navigation', async () => {
  const { ProgressCoordinator } = await import('../src/components/onboarding/progressCoordinator');
  let release!: (value: boolean) => void;
  let navigations = 0;
  let calls = 0;
  const coordinator = new ProgressCoordinator(async () => { if (++calls === 1) throw new Error('offline'); return true; });
  await coordinator.resume({active_step:'chat'}, () => { navigations++; });
  assert.equal(await coordinator.pause(), true);
  assert.equal(navigations, 0);
  const other = new ProgressCoordinator(() => new Promise(resolve => { release = resolve; }));
  const pending = other.resume({active_step:'chat'}, () => { navigations++; });
  await Promise.resolve(); other.invalidate(); release(true); await pending;
  assert.equal(navigations, 0);
});

test('Kontowechsel verwirft noch nicht gestartete Schreibvorgänge des alten Kontos', async () => {
  const { ProgressCoordinator } = await import('../src/components/onboarding/progressCoordinator');
  let release!: (value: boolean) => void;
  let writes = 0;
  const coordinator = new ProgressCoordinator(() => { writes++; return new Promise(resolve => { release = resolve; }); });
  const first = coordinator.save({active_step:'discord'});
  await Promise.resolve();
  const queuedPause = coordinator.pause();
  coordinator.dispose();
  release(true);
  assert.equal(await first, true);
  assert.equal(await queuedPause, false);
  assert.equal(writes, 1);
  // React StrictMode darf den Effekt erneut aktivieren, verworfene Arbeit bleibt verworfen.
  coordinator.activate();
  const next = coordinator.save({paused:true});
  await Promise.resolve(); release(true);
  assert.equal(await next, true);
  assert.equal(writes, 2);
});

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { readFile, writeFile, mkdir, mkdtemp, rm, lstat, realpath } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve, extname } from 'node:path';
import { getPreviewApiFixture, getPreviewPathFixture } from '../src/preview/fixtures.ts';

// Kein ausführbares Programm aus CLI, Umgebung oder Browserdaten übernehmen.
// Dieser lokale Bediennachweis verwendet ausschließlich den geprüften Build.
assert.equal(process.argv.length, 2, 'Dieser Nachweis akzeptiert keine CLI-Argumente');
const executable = '/home/nathanael/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell';
const executableInfo = await lstat(executable);
assert.ok(executableInfo.isFile() && !executableInfo.isSymbolicLink()
  && (executableInfo.mode & 0o022) === 0, 'Chromium muss eine geschützte reguläre Datei sein');
assert.equal(await realpath(executable), executable, 'Chromium darf nicht umgeleitet sein');
assert.equal(createHash('sha256').update(await readFile(executable)).digest('hex'),
  'e11fc9ce65c96313476f7ee9844b6fb6a9220fb048693cfe9eee00acf4170a9f',
  'Der Chromium-Build stimmt nicht mit dem geprüften Build überein');
const artifacts = new URL('./artifacts/uplink/', import.meta.url);
await mkdir(artifacts, { recursive: true });
const dist = resolve(new URL('../../analytics/dashboard_v2/dist/', import.meta.url).pathname);
const profile = await mkdtemp(join(tmpdir(), 'uplink-dashboard-browser-'));
let live = false;
let ended = false;
let holdNextSave = false;
let releaseSave;
let destinationPolls = 0;
const destinations = structuredClone(getPreviewPathFixture('/twitch/api/v2/uplink/destinations'));
Object.assign(destinations.destinations.find(target => target.platform === 'twitch'), {twitch_audio_mode:null,effective_audio_mode:'separate_vod',active_audio_mode:null});
const requests = [];
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  const json = (value, status = 200) => { response.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.end(JSON.stringify(value)); };
  if (url.pathname.startsWith('/twitch/api/')) {
    if (url.pathname.endsWith('/streamers')) return json([]);
    if (url.pathname.endsWith('/uplink/help')) return json([]);
    if (request.method === 'PUT' && url.pathname.endsWith('/uplink/destinations')) {
      let body = ''; for await (const chunk of request) { body += chunk; assert.ok(body.length < 4096); }
      const saved = JSON.parse(body); requests.push(saved);
      if (holdNextSave) {
        holdNextSave = false;
        await new Promise(resolve => { releaseSave = resolve; });
      }
      const destination = destinations.destinations.find(item => item.platform === saved.platform);
      if (saved.manuell) destination.requested = saved.manuell;
      if (saved.enabled !== undefined) destination.enabled = saved.enabled;
      if (saved.twitch_audio_mode !== undefined) {
        destination.twitch_audio_mode = saved.twitch_audio_mode;
        destination.effective_audio_mode = saved.twitch_audio_mode;
      }
      return json({ ...destinations, live_quality: { status: 'next_stream', message: 'Gespeichert für den nächsten Stream.' } });
    }
    if (request.method !== 'GET') return json({ error: 'Für diesen lokalen Bediennachweis nicht freigegeben.' }, 409);
    if (url.pathname.endsWith('/uplink/me')) {
      const data = structuredClone(getPreviewPathFixture('/twitch/api/v2/uplink/me'));
      data.live_status = 'aus'; data.service_status = 'ready'; data.capabilities = { reconnect: false };
      data.session = live && !ended ? { active: true, state: 'Medien werden empfangen', ingest_end_reason: null, received_events: 245, received_bytes: 143000,
        source_observation: { codec: 'h264', width: 320, height: 180, fps_numerator: 25, fps_denominator: 1,
          audio: [{wire_track:0,codec:'aac',sample_rate:48000,channels:1},{wire_track:1,codec:'aac',sample_rate:48000,channels:1}],sampled_duration_ms:1000 },
        outputs: { encode_groups: 1, video_decoders: 1 } } : null;
      return json(data);
    }
    if (url.pathname.endsWith('/uplink/destinations')) {
      destinationPolls += 1;
      const data = structuredClone(destinations);
      if (!live) for (const target of data.destinations) { target.output_state = 'unknown'; target.active_profile = null; }
      if (live) data.destinations.find(target => target.platform === 'youtube').active_profile = {
        width: 256, height: 144, fps: 25, codec: 'h264', bitrate_kbps: 500, profile_origin: 'running_graph',
      };
      if (live) Object.assign(data.destinations.find(target => target.platform === 'twitch'), {output_state:'sending',active_audio_mode:'live',reason:null,active_profile:{width:256,height:144,fps:25,codec:'h264',bitrate_kbps:500,profile_origin:'running_graph'}});
      if (ended) for (const target of data.destinations) { target.output_state = 'finished'; target.active_profile = null; target.active_audio_mode = null; }
      return json(data);
    }
    if (url.pathname.endsWith('/auth-status')) {
      const auth = structuredClone(getPreviewApiFixture('/auth-status'));
      Object.assign(auth, { level: 'partner', demoMode: false, isLocalhost: false, isAdmin: false, adminMode: false, adminEligible: false, canViewAllStreamers: false });
      return json(auth);
    }
    return json(getPreviewPathFixture(url.pathname) ?? getPreviewApiFixture(url.pathname.replace('/twitch/api/v2', '')) ?? {});
  }
  if (url.pathname === '/twitch/raid/auth') {
    assert.equal(url.searchParams.get('scope_profile'), 'uplink');
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' }); response.end('<h1>Lokaler Nachweis: bestehender Uplink-OAuth-Weg</h1>'); return;
  }
  const relative = url.pathname.replace(/^\/twitch\/(analytics|dashboard-v2)/, '').replace(/^\//, '');
  let file = resolve(dist, relative || 'index.html');
  if (!file.startsWith(dist + '/')) { response.writeHead(404); response.end(); return; }
  try {
    let data; try { data = await readFile(file); } catch { file = join(dist, 'index.html'); data = await readFile(file); }
    const type = { '.html': 'text/html; charset=utf-8', '.js': 'application/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png' }[extname(file)] ?? 'application/octet-stream';
    response.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'self' data:; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self' data:; object-src 'none'" }); response.end(data);
  } catch { response.writeHead(500); response.end(); }
});
server.listen(0, '127.0.0.1'); await once(server, 'listening');
const origin = `http://127.0.0.1:${server.address().port}`;
const chrome = spawn(executable, ['--no-sandbox', '--disable-gpu', '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
let socket;
try {
  const browserUrl = await new Promise((resolve, reject) => {
    let buffer = ''; const timer = setTimeout(() => reject(new Error('Chromium-Startfrist')), 12000);
    chrome.once('error', reject); chrome.once('exit', () => reject(new Error('Chromium vor Bereitschaft beendet')));
    chrome.stderr.on('data', data => { buffer = (buffer + data).slice(-16384); const match = buffer.match(/DevTools listening on (ws:\/\/[^\s]+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
  });
  socket = new WebSocket(browserUrl); await once(socket, 'open');
  let id = 0; const pending = new Map(); const exceptions = [];
  socket.onmessage = event => { const data = JSON.parse(event.data); if (data.method === 'Runtime.exceptionThrown') exceptions.push(data.params.exceptionDetails.text); const handler = pending.get(data.id); if (handler) { clearTimeout(handler.timer); pending.delete(data.id); data.error ? handler.reject(new Error(data.error.message)) : handler.resolve(data.result); } };
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const current = ++id; const timer = setTimeout(() => { pending.delete(current); reject(new Error(`CDP-Frist: ${method}`)); }, 12000); pending.set(current, { resolve, reject, timer }); socket.send(JSON.stringify({ id: current, method, params, ...(sessionId ? {sessionId} : {}) })); });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const cdp = (method, params) => send(method, params, sessionId);
  await cdp('Page.enable'); await cdp('Runtime.enable');
  const evaluate = async expression => { const result = await cdp('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }); if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text); return result.result.value; };
  const wait = async expression => { for (let retry = 0; retry < 100; retry++) { if (await evaluate(`!!document.body && (${expression})`)) return; await new Promise(resolve => setTimeout(resolve, 50)); } console.error(await evaluate('document.body?.innerText')); throw new Error(`UI nicht bereit: ${expression}`); };
  const screenshot = async name => { await new Promise(resolve => setTimeout(resolve, 550)); const { data } = await cdp('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true }); await writeFile(new URL(name + '.png', artifacts), Buffer.from(data, 'base64')); };
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1080, deviceScaleFactor: 1, mobile: false });
  await cdp('Page.navigate', { url: origin + '/twitch/uplink' });
  await wait("document.body.innerText.includes('Serveradresse')");
  assert.equal(await evaluate("document.body.innerText.includes('SRT')"), false);
  assert.equal(await evaluate(`document.querySelector('input[aria-label="Privater Streamschlüssel für OBS: verdeckt"]').type`), 'password');
  assert.equal(await evaluate("document.body.innerText.includes('Rechte ergänzen')"), true);
  assert.equal(await evaluate("document.documentElement.scrollWidth > innerWidth"), false);
  await screenshot('offline-desktop');
  await writeFile(new URL('dom-offline.json', artifacts), JSON.stringify(await evaluate("({buttons:[...document.querySelectorAll('button')].map(b=>({text:b.innerText,aria:b.getAttribute('aria-label'),disabled:b.disabled})),links:[...document.querySelectorAll('a')].map(a=>({text:a.innerText,href:a.getAttribute('href')}))})"), null, 2));
  await send('Browser.grantPermissions', { origin, permissions: ['clipboardReadWrite','clipboardSanitizedWrite'] });
  await cdp('Page.bringToFront');
  const activate = async expression => {
    await evaluate(`(${expression}).scrollIntoView({block:'center',behavior:'instant'})`);
    await new Promise(resolve => setTimeout(resolve, 100));
    const point = await evaluate(`(() => { const element = (${expression}); const rect = element.getBoundingClientRect(); return {x:rect.x + rect.width/2, y:rect.y + rect.height/2}; })()`);
    await cdp('Input.dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', clickCount: 1 });
    await cdp('Input.dispatchMouseEvent', { type: 'mouseReleased', ...point, button: 'left', clickCount: 1 });
  };
  await activate(`document.querySelector('[aria-label="Serveradresse für OBS kopieren"]')`);
  await wait('document.body.innerText.includes("Kopiert")');
  assert.equal(await evaluate('navigator.clipboard.readText()'), 'rtmps://uplink.example/live');
  await activate(`document.querySelector('[aria-label="Privater Streamschlüssel für OBS kopieren"]')`);
  await wait('(navigator.clipboard.readText()).then(text => text === "rsr_preview")');
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "Zeigen")');
  await wait(`document.querySelector('input[aria-label="Privater Streamschlüssel für OBS: sichtbar"]')?.type === 'text'`);
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "Verdecken")');
  await wait(`document.querySelector('input[aria-label="Privater Streamschlüssel für OBS: verdeckt"]')?.type === 'password'`);
  await activate(`document.querySelector('a[href="/twitch/raid/auth?scope_profile=uplink"]')`);
  await wait('document.body.innerText.includes("bestehender Uplink-OAuth-Weg")');
  await cdp('Page.navigate', { url: origin + '/twitch/uplink' });
  await wait('document.body.innerText.includes("Serveradresse")');
  await activate(`document.querySelector('details[data-platform="youtube"] > summary')`);
  await wait(`document.querySelector('details[data-platform="youtube"]').open`);
  await evaluate(`(() => { const group = document.querySelector('[aria-label="YouTube-Einstellungen"]'); const label = [...group.querySelectorAll('label')].find(label => label.textContent.includes('Bitrate')); const input = label.querySelector('input'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'15000'); input.dispatchEvent(new Event('input',{bubbles:true})); })()`);
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "YouTube speichern")');
  await wait('document.body.innerText.includes("Gespeichert für den nächsten Stream.")');
  assert.equal(requests.at(-1).manuell.bitrate_kbps, 15000);
  const bitrateInput = `[...document.querySelector('[aria-label="YouTube-Einstellungen"]').querySelectorAll('label')].find(label => label.textContent.includes('Bitrate')).querySelector('input')`;
  const typeBitrate = async text => {
    await activate(bitrateInput);
    await cdp('Input.dispatchKeyEvent', {type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await cdp('Input.dispatchKeyEvent', {type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await cdp('Input.insertText', {text});
    await wait(`(${bitrateInput}).value === ${JSON.stringify(text)}`);
  };
  await typeBitrate('14000');
  holdNextSave = true;
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "YouTube speichern")');
  await wait('document.body.innerText.includes("YouTube wird gespeichert")');
  await typeBitrate('13000');
  const previousPolls = destinationPolls;
  const pollDeadline = Date.now() + 8000;
  while (destinationPolls === previousPolls && Date.now() < pollDeadline) await new Promise(resolve => setTimeout(resolve, 100));
  assert.ok(destinationPolls > previousPolls, 'Normaler 5-Sekunden-Statuspoll wurde wirklich empfangen');
  assert.equal(await evaluate(`(${bitrateInput}).value`), '13000');
  assert.equal(await evaluate(`document.querySelector('details[data-platform="youtube"]').open`), true);
  releaseSave(); releaseSave = undefined;
  await wait('!document.body.innerText.includes("YouTube wird gespeichert")');
  assert.equal(await evaluate(`(${bitrateInput}).value`), '13000');
  assert.equal(await evaluate(`document.querySelector('[aria-label="YouTube-Einstellungen"]').innerText.includes('Gespeichert für den nächsten Stream.')`), false, 'Eine alte Antwort darf die inzwischen geänderten Werte nicht als gespeichert markieren');
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "YouTube speichern")');
  await wait('document.body.innerText.includes("Gespeichert für den nächsten Stream.")');
  assert.equal(requests.at(-1).manuell.bitrate_kbps, 13000);
  await screenshot('einstellungen-gespeichert');
  await activate(`document.querySelector('details[data-platform="twitch"] > summary')`);
  await wait(`document.querySelector('details[data-platform="twitch"]').open`);
  const twitchForm = `document.querySelector('[aria-label="Twitch-Einstellungen"]')`;
  assert.equal(await evaluate(`(${twitchForm}).querySelectorAll('input[type="radio"]:checked').length`),0);
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "Twitch speichern")');
  await wait(`(${twitchForm}).innerText.includes('Gespeichert für den nächsten Stream.')`);
  assert.equal(Object.hasOwn(requests.at(-1),'twitch_audio_mode'),false,'Altbestand nicht durch Profilspeichern ändern');
  await activate(`(${twitchForm}).querySelector('input[type="radio"][value="live"]')`);
  holdNextSave = true;
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "Twitch speichern")');
  await wait('document.body.innerText.includes("Twitch wird gespeichert")');
  await activate(`(${twitchForm}).querySelector('input[type="radio"][value="separate_vod"]')`);
  const audioPolls = destinationPolls;
  const audioDeadline = Date.now() + 8000;
  while (destinationPolls === audioPolls && Date.now() < audioDeadline) await new Promise(resolve => setTimeout(resolve,100));
  assert.ok(destinationPolls > audioPolls);
  releaseSave(); releaseSave = undefined;
  await wait('!document.body.innerText.includes("Twitch wird gespeichert")');
  assert.equal(await evaluate(`(${twitchForm}).querySelector('input[type="radio"]:checked').value`),'separate_vod');
  assert.equal(await evaluate(`(${twitchForm}).innerText.includes('Gespeichert für den nächsten Stream.')`),false);
  await cdp('Page.reload');
  await wait(`document.body.innerText.includes('Gespeichert: Live-Ton.')`);
  assert.equal(await evaluate(`(${twitchForm}).querySelector('input[type="radio"]:checked').value`),'live');
  live = true;
  await cdp('Page.reload'); await wait("document.body.innerText.includes('Medien werden gesendet')");
  assert.equal(await evaluate("document.body.innerText.includes('Stream wird empfangen')"), true);
  assert.equal(await evaluate("document.body.innerText.includes('H264 · 320×180 · 25 fps')"), true);
  assert.equal(await evaluate("document.body.innerText.includes('Laufendes Encoderprofil: 256×144 · 25 fps · H264 · 500 kbit/s Zielbitrate')"), true);
  assert.equal(await evaluate("document.body.innerText.includes('Plattform bestätigt live')"), false);
  await activate(`(${twitchForm}).querySelector('input[type="radio"][value="separate_vod"]')`);
  await activate('[...document.querySelectorAll("button")].find(button => button.innerText === "Twitch speichern")');
  await wait(`(${twitchForm}).innerText.includes('Für den nächsten Stream: Separater Twitch-VOD-Ton.')`);
  assert.equal(await evaluate(`(${twitchForm}).innerText.includes('Laufender Twitch-Ton: Live-Ton.')`),true);
  await screenshot('audio-naechster-stream');
  assert.equal(await evaluate(`document.querySelector('input[aria-label="Privater Streamschlüssel für OBS: verdeckt"]').type`), 'password');
  await screenshot('sendend-desktop');
  assert.equal(await evaluate('[...document.querySelectorAll("button")].filter(button => button.innerText === "Zeigen").every(button => button.disabled)'), true, 'Laufender Uplink-Eingang hält private Felder auch bei Twitch offline verdeckt');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  await screenshot('sendend-mobil');
  assert.equal(await evaluate('document.documentElement.scrollWidth > innerWidth'), false);
  ended = true;
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1080, deviceScaleFactor: 1, mobile: false });
  await cdp('Page.reload');
  await wait("document.body.innerText.includes('Kein laufender Streameingang')");
  assert.equal(await evaluate("document.body.innerText.includes('Stream wird empfangen')"), false);
  assert.equal(await evaluate("document.body.innerText.includes('H264 · 320×180 · 25 fps')"), false);
  assert.equal(await evaluate("document.body.innerText.includes('Laufendes Encoderprofil:')"), false);
  await screenshot('beendet-desktop');
  assert.deepEqual(exceptions, []);
  await writeFile(new URL('report.json', artifacts), JSON.stringify({ browser: (await send('Browser.getVersion')).product, network: 'Nur Loopback und synthetische Daten', requests, destinationPolls, exceptions }, null, 2));
  console.log(JSON.stringify({ screenshots: artifacts.pathname, assertions: 'passed' }));
} finally {
  releaseSave?.();
  socket?.close(); chrome.kill('SIGTERM'); server.close();
  if (chrome.exitCode === null && chrome.signalCode === null) { const deadline = setTimeout(() => chrome.kill('SIGKILL'), 2000); await once(chrome, 'exit').catch(() => {}); clearTimeout(deadline); }
  await rm(profile, { recursive: true, force: true });
}

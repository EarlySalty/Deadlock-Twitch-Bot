import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '/home/nathanael/Documents/claude-config/tools/sichtpruefung/node_modules/playwright-core/index.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const screens = path.resolve(here, '../screens');
const htmlPath = path.resolve(here, '../../../rust/crates/tb-dashboard-api/src/handlers/caster_camera.html');
const mockPort = 4199;
const cdpPort = 9339;
const origin = `http://127.0.0.1:${mockPort}`;
const children = [];

function run(cmd, args) {
  const child = spawn(cmd, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child);
  child.stderr.on('data', (chunk) => process.stderr.write(chunk));
  return child;
}

function waitExit(child) {
  return new Promise((resolve) => child.on('exit', resolve));
}

async function waitHttp(url, tries = 50) {
  for (let i = 0; i < tries; i += 1) {
    try {
      const res = await fetch(url, { redirect: 'manual' });
      if (res.ok || res.status === 404) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(`timeout ${url}`);
}

function stopAll() {
  for (const child of children) {
    try { child.kill('SIGTERM'); } catch {}
  }
}

function serveCamera() {
  const html = fs.readFileSync(htmlPath);
  const server = http.createServer((req, res) => {
    const url = new URL(req.url || '/', origin);
    if (url.pathname.startsWith('/twitch/caster-camera/')) {
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
      res.end(html);
      return;
    }
    res.writeHead(404);
    res.end();
  });
  return new Promise((resolve) => {
    server.listen(mockPort, '127.0.0.1', () => resolve(server));
  });
}

const fakeMedia = `(() => {
  function fakeTrack() {
    return {
      kind: 'video',
      id: 'fake-track',
      label: 'Fake Cam 1440p60',
      contentHint: '',
      enabled: true,
      muted: false,
      readyState: 'live',
      getSettings() { return { width: 2560, height: 1440, frameRate: 60, deviceId: 'fake-cam' }; },
      stop() { this.readyState = 'ended'; },
      clone() { return fakeTrack(); },
      addEventListener() {},
      removeEventListener() {},
    };
  }
  function fakeStream() {
    const track = fakeTrack();
    return {
      id: 'fake-stream',
      active: true,
      getTracks() { return [track]; },
      getVideoTracks() { return [track]; },
      getAudioTracks() { return []; },
      addTrack() {},
      removeTrack() {},
      addEventListener() {},
      removeEventListener() {},
    };
  }
  navigator.mediaDevices.getUserMedia = async () => fakeStream();
  navigator.mediaDevices.enumerateDevices = async () => ([
    { deviceId: 'fake-cam', kind: 'videoinput', label: 'Fake Cam 1440p60', groupId: 'fake' },
  ]);
  const senderState = { encodings: [{}], degradationPreference: '' };
  const fakeSender = {
    track: { kind: 'video' },
    getParameters() {
      return { encodings: senderState.encodings, degradationPreference: senderState.degradationPreference };
    },
    async setParameters(params) {
      senderState.degradationPreference = params.degradationPreference || '';
      senderState.encodings = params.encodings || [{}];
      window.__p3Sender = params;
    },
  };
  RTCPeerConnection.prototype.addTrack = function addTrack() { return fakeSender; };
  RTCPeerConnection.prototype.getSenders = function getSenders() { return [fakeSender]; };
  RTCPeerConnection.prototype.createOffer = async function createOffer() { return { type: 'offer', sdp: 'v=0' }; };
  RTCPeerConnection.prototype.setLocalDescription = async function setLocalDescription() {};
  RTCPeerConnection.prototype.close = function close() {};
  function FakeSocket() {
    this.readyState = 1;
    this.sent = [];
    setTimeout(() => {
      this.onopen && this.onopen();
      this.onmessage && this.onmessage({ data: JSON.stringify({ type: 'ready' }) });
      this.onmessage && this.onmessage({ data: JSON.stringify({ type: 'subscriber-joined', peerId: 'obs-1' }) });
    }, 40);
  }
  FakeSocket.prototype.send = function send(data) { this.sent.push(data); };
  FakeSocket.prototype.close = function close() { this.readyState = 3; };
  FakeSocket.OPEN = 1;
  window.WebSocket = FakeSocket;
})();`;

try {
  fs.mkdirSync(screens, { recursive: true });
  const server = await serveCamera();
  children.push({ kill() { server.close(); } });
  const moli = run('/home/nathanael/.local/bin/moli', [
    'serve', '--host', '127.0.0.1', '-p', String(cdpPort), '--layout', '--image', '--font',
  ]);
  await waitHttp(`${origin}/twitch/caster-camera/cam-p3`);
  await waitHttp(`http://127.0.0.1:${cdpPort}/json/version`);
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
  const context = browser.contexts()[0] || await browser.newContext();
  await context.grantPermissions(['camera'], { origin });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.addInitScript(fakeMedia);

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`${origin}/twitch/caster-camera/cam-p3#token=p3-token`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('heading', { name: 'Caster-Kamera freigeben' }).waitFor({ timeout: 10000 });
  await page.locator('#consent').evaluate((el) => {
    el.checked = true;
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.locator('#start').evaluate((el) => el.click());
  await page.waitForFunction(() => {
    const text = document.getElementById('capture')?.textContent || '';
    return /2560x1440/.test(text) && /16 Mbit\/s/.test(text);
  }, undefined, { timeout: 12000 });
  const capture1440 = await page.locator('#capture').innerText();
  await page.screenshot({ path: path.join(screens, 'p3-1440-camera.png'), fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: path.join(screens, 'p3-390-camera.png'), fullPage: true });

  const proof = await page.evaluate(async () => {
    const track = document.getElementById('preview')?.srcObject?.getVideoTracks?.()[0];
    const settings = track?.getSettings?.() || {};
    const hint = track?.contentHint || '';
    const pcs = [];
    return {
      capture: document.getElementById('capture')?.textContent || '',
      status: document.getElementById('status')?.textContent || '',
      settings: { width: settings.width, height: settings.height, frameRate: settings.frameRate },
      contentHint: hint,
      sender: window.__p3Sender || null,
    };
  });
  proof.errors = errors;
  proof.capture1440 = capture1440;
  fs.writeFileSync(path.join(here, 'moli-p3.md'), [
    '# P3 Moli-Nachweis 2026-10-19',
    '',
    `Capture: ${proof.capture}`,
    `getSettings: ${proof.settings.width}x${proof.settings.height} @ ${proof.settings.frameRate} fps`,
    `contentHint: ${proof.contentHint}`,
    `Status: ${proof.status}`,
    `errors: ${proof.errors.length ? proof.errors.join('; ') : 'keine'}`,
    '',
    'Dateien: p3-1440-camera.png, p3-390-camera.png',
    '',
  ].join('\n'));
  console.log(JSON.stringify(proof));
  await browser.close();
  stopAll();
  await Promise.all(children.map((child) => (child.on ? waitExit(child) : Promise.resolve())));
  if (errors.length) process.exit(2);
} catch (error) {
  console.error(error);
  stopAll();
  process.exit(1);
}

import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { normalisiereCaps } from '../src/uplinkEmpfehlung';
import { fetchUplinkHelp, uplinkHelpUrl, UPLINK_HELP_PAGES } from '../src/uplinkHelp';
import { obsZugang } from '../src/uplinkBetrieb';

test('normalisiereCaps liest die neuen Empfehlungsfelder', () => {
  const caps = normalisiereCaps({
    platform: 'twitch',
    recommended_width: 2560,
    recommended_height: 1440,
    recommended_fps: 60,
    recommended_bitrate_kbps: 12000,
    force_cbr: true,
  });
  assert.deepEqual(caps, {
    platform: 'twitch',
    recommended_width: 2560,
    recommended_height: 1440,
    recommended_fps: 60,
    recommended_bitrate_kbps: 12000,
    force_cbr: true,
  });
});

test('alte Obergrenzen sind keine geprüfte Profilempfehlung', () => {
  const caps = normalisiereCaps({
    platform: 'kick',
    max_width: 1920,
    max_height: 1080,
    max_fps: 60,
    max_bitrate_kbps: 8000,
    force_cbr: true,
  });
  assert.equal(caps.recommended_width, null);
  assert.equal(caps.recommended_bitrate_kbps, null);
  assert.equal(caps.force_cbr, true);
});

test('fehlende oder unbrauchbare Werte werden zu null statt zu 0', () => {
  const caps = normalisiereCaps({ platform: 'tiktok', recommended_bitrate_kbps: 0 });
  assert.equal(caps.recommended_width, null);
  assert.equal(caps.recommended_bitrate_kbps, null);
  assert.equal(caps.force_cbr, false);
});

test('OBS-Hilfe trennt Uploadbudget und geprüfte Plattformausgabe', async (t) => {
  t.mock.method(globalThis, 'fetch', async (input: string | URL | Request) => {
    const page = UPLINK_HELP_PAGES.find((entry) => uplinkHelpUrl(entry.file) === String(input));
    assert.ok(page);
    return new Response(readFileSync(new URL(`../public/uplink/${page.file}`, import.meta.url), 'utf8'));
  });
  const pages = await fetchUplinkHelp();
  assert.equal(pages.length, UPLINK_HELP_PAGES.length);
  const embedded = pages.find((page) => page.file === 'obs.html');
  assert.ok(embedded);
  assert.match(embedded.html, /^<div class="uplink-doc" data-doc="obs">/);
  const help = readFileSync(new URL('../public/uplink/obs.html', import.meta.url), 'utf8');
  for (const content of [embedded.html, help]) {
    assert.match(content, /CBR/);
    assert.match(content, /AV1/);
    assert.doesNotMatch(content, /obsBitrateEmpfehlung|VBR|SRT-Adresse|Streamschlüssel leer lassen/);
  }
});

test('OBS-Zugang weist ASCII-Steuerzeichen zurück und erhält gültige Werte unverändert', () => {
  const server = 'rtmps://synthetic.example/live';
  for (let code = 0; code <= 0x7f; code += 1) {
    const key = `synthetic${String.fromCharCode(code)}input`;
    const actual = obsZugang({ service_status: 'ready', public_ingest_url: server, ingest_key: key });
    assert.deepEqual(actual, code <= 0x20 || code === 0x7f ? null : { server, key }, `ASCII ${code}`);
  }
  const key = `synthetic-ä-中-${String.fromCodePoint(0x1f3ae)}`;
  assert.deepEqual(obsZugang({ service_status: 'ready', public_ingest_url: server, ingest_key: key }), { server, key });
});

test('ausdrücklich ungeprüfte Empfehlungen übernehmen keine alten Maximalwerte', () => {
  const caps = normalisiereCaps({ platform: 'youtube', recommended_bitrate_kbps: null, max_bitrate_kbps: 24000 });
  assert.equal(caps.recommended_bitrate_kbps, null);
});

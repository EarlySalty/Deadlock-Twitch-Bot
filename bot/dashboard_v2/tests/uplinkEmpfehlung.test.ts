import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { normalisiereCaps } from '../src/uplinkEmpfehlung';

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
  // Eine Obergrenze allein kennt weder Quelle, Codec noch Konto.
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

test('OBS-Hilfe trennt Uploadbudget und geprüfte Plattformausgabe', () => {
  const page = readFileSync(new URL('../src/pages/Uplink.tsx', import.meta.url), 'utf8');
  const help = readFileSync(new URL('../public/uplink/obs.html', import.meta.url), 'utf8');
  for (const content of [page, help]) {
    assert.match(content, /CBR/);
    assert.match(content, /AV1/);
    assert.doesNotMatch(content, /obsBitrateEmpfehlung|VBR|SRT-Adresse|Streamschlüssel leer lassen/);
  }
});

test('ausdrücklich ungeprüfte Empfehlungen übernehmen keine alten Maximalwerte', () => {
  const caps = normalisiereCaps({ platform: 'youtube', recommended_bitrate_kbps: null, max_bitrate_kbps: 24000 });
  assert.equal(caps.recommended_bitrate_kbps, null);
});

import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { extractUplinkMain } from '../src/uplinkHelp';

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const HELP_ROOT = join(DASHBOARD_ROOT, 'public', 'uplink');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');

const HELP_PAGES = [
  ['index.html', 'index', 'Uplink Hilfe'],
  ['was-ist.html', 'was-ist', 'Uplink nimmt deinen OBS-Stream entgegen'],
  ['obs.html', 'obs', 'Einstellungen → Ausgabe'],
  ['stoerungen.html', 'stoerungen', 'Internet bricht weg'],
] as const;

test('Dashboard bindet die Uplink-Hilfe mit main.uplink-doc ein', () => {
  assert.match(UPLINK_PAGE, /UPLINK_HELP_PAGES/);
  assert.match(UPLINK_PAGE, /dangerouslySetInnerHTML/);
  assert.match(UPLINK_PAGE, /data\.srt_hint/);
  assert.doesNotMatch(UPLINK_PAGE, /RTMP-Server/);
  assert.doesNotMatch(UPLINK_PAGE, /Komplette RTMP-Adresse/);

  for (const [file, doc, passage] of HELP_PAGES) {
    const path = join(HELP_ROOT, file);
    assert.ok(existsSync(path), `Hilfequelle fehlt: ${file}`);
    const html = readFileSync(path, 'utf8');
    assert.match(html, new RegExp(`<main class="uplink-doc" data-doc="${doc}">`));
    assert.match(html, new RegExp(passage.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
  }
});

test('extrahiert nur main.uplink-doc aus einer Streamer-Seite', () => {
  const html = '<body><main class="uplink-doc" data-doc="obs"><h1>OBS</h1></main></body>';
  assert.equal(
    extractUplinkMain(html),
    '<main class="uplink-doc" data-doc="obs"><h1>OBS</h1></main>',
  );
});

test('weist eine HTML-Seite ohne main.uplink-doc zurück', () => {
  assert.throws(
    () => extractUplinkMain('<body><h1>Keine Uplink-Hilfe</h1></body>'),
    /main\.uplink-doc/,
  );
});

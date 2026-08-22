import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { extractUplinkMain, UPLINK_HELP_PAGES, uplinkHelpUrl } from '../src/uplinkHelp';

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

test('extrahiert nur main.uplink-doc und legt die Ueberschrift eine Ebene tiefer', () => {
  // Die Seite hat schon eine h1; drei Fragmente mit eigener h1 zerlegen die
  // Ueberschriftenstruktur fuer Screenreader.
  const html = '<body><main class="uplink-doc" data-doc="obs"><h1>OBS</h1></main></body>';
  assert.equal(
    extractUplinkMain(html),
    '<main class="uplink-doc" data-doc="obs"><h2>OBS</h2></main>',
  );
});

test('nimmt nur bis zum ersten schliessenden main', () => {
  // Gieriges Matching zoege fremdes Markup in das dangerouslySetInnerHTML.
  const html =
    '<main class="uplink-doc" data-doc="obs"><p>A</p></main><main><p>fremd</p></main>';
  assert.equal(extractUplinkMain(html), '<main class="uplink-doc" data-doc="obs"><p>A</p></main>');
});

test('macht relative Links im Fragment absolut', () => {
  // Gerendert wird das Fragment auf /twitch/uplink: `obs.html` zeigte dort auf
  // /twitch/obs.html, eine Route, die es nicht gibt.
  const html =
    '<main class="uplink-doc" data-doc="was-ist"><a href="obs.html">OBS</a>' +
    '<a href="https://twitch.tv">extern</a><a href="#anker">Anker</a></main>';
  const fragment = extractUplinkMain(html);
  assert.match(fragment, new RegExp(`href="${uplinkHelpUrl('obs.html')}"`));
  assert.match(fragment, /href="https:\/\/twitch\.tv"/);
  assert.match(fragment, /href="#anker"/);
});

test('jede eingebettete Seite existiert als Datei', () => {
  // Ohne diese Pruefung faellt eine neue Hilfeseite lautlos aus der Einbettung.
  for (const page of UPLINK_HELP_PAGES) {
    assert.ok(existsSync(join(HELP_ROOT, page.file)), `Hilfequelle fehlt: ${page.file}`);
  }
});

test('weist eine HTML-Seite ohne main.uplink-doc zurück', () => {
  assert.throws(
    () => extractUplinkMain('<body><h1>Keine Uplink-Hilfe</h1></body>'),
    /main\.uplink-doc/,
  );
});

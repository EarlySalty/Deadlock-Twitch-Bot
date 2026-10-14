import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  extractUplinkMain,
  fetchUplinkHelp,
  titelUeberschriftEntfernen,
  UPLINK_HELP_PAGES,
  uplinkHelpUrl,
} from '../src/uplinkHelp';

const DASHBOARD_ROOT = join(import.meta.dirname, '..');
const HELP_ROOT = join(DASHBOARD_ROOT, 'public', 'uplink');
const UPLINK_PAGE = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'Uplink.tsx'), 'utf8');
const UPLINK_DESTINATION = readFileSync(join(DASHBOARD_ROOT, 'src', 'pages', 'UplinkZiel.tsx'), 'utf8');

const HELP_PAGES = [
  ['index.html', 'index', 'Uplink Hilfe'],
  ['was-ist.html', 'was-ist', 'Uplink nimmt deinen OBS-Stream entgegen'],
  ['obs.html', 'obs', 'Einstellungen → Ausgabe'],
  ['stoerungen.html', 'stoerungen', 'Internet bricht weg'],
] as const;

test('Dashboard bindet die Uplink-Hilfe mit main.uplink-doc ein', () => {
  assert.match(UPLINK_PAGE, /UPLINK_HELP_PAGES/);
  assert.match(UPLINK_PAGE, /dangerouslySetInnerHTML/);
  assert.match(UPLINK_PAGE, /value=\{obs\.server\}/);
  assert.match(UPLINK_PAGE, /value=\{obs\.key\}/);
  assert.doesNotMatch(UPLINK_PAGE, /srt_hint/);
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

test('extrahiert nur main.uplink-doc und legt die Ueberschriften eine Ebene tiefer', () => {
  const html =
    '<body><main class="uplink-doc" data-doc="obs"><h1>OBS</h1><h2>Ablauf</h2><p>Text</p></main></body>';
  assert.equal(
    extractUplinkMain(html),
    '<div class="uplink-doc" data-doc="obs"><h3>Ablauf</h3><p>Text</p></div>',
  );
});

test('nimmt nur bis zum ersten schliessenden main', () => {
  const html =
    '<main class="uplink-doc" data-doc="obs"><p>A</p></main><main><p>fremd</p></main>';
  assert.equal(extractUplinkMain(html), '<div class="uplink-doc" data-doc="obs"><p>A</p></div>');
});

test('macht relative Links im Fragment absolut', () => {
  const html =
    '<main class="uplink-doc" data-doc="was-ist"><a href="obs.html">OBS</a>' +
    '<a href="https://twitch.tv">extern</a><a href="#anker">Anker</a></main>';
  const fragment = extractUplinkMain(html);
  assert.match(fragment, new RegExp(`href="${uplinkHelpUrl('obs.html')}"`));
  assert.match(fragment, /href="https:\/\/twitch\.tv"/);
  assert.match(fragment, /href="#anker"/);
});

test('Zahlen in HTML- und Markdown-Fassung stimmen ueberein', () => {
  const KNOWLEDGE_ROOT = join(DASHBOARD_ROOT, '../../rust/knowledge/bot');
  const PAARE = [
    ['obs.html', 'uplink-obs.md'],
    ['was-ist.html', 'uplink-was-ist.md'],
    ['stoerungen.html', 'uplink-stoerungen.md'],
  ] as const;
  const ZAHLEN =
    /\b\d+(?:[.,]\d+)?\s*(?:kbps|Kbps|kbit\/s|Mbit\/s|Mbit|mbit|s\b|p\b|fps)|\b\d{4}\b/g;

  for (const [htmlDatei, mdDatei] of PAARE) {
    const html = readFileSync(join(HELP_ROOT, htmlDatei), 'utf8').replace(/<[^>]+>/g, ' ');
    const md = readFileSync(join(KNOWLEDGE_ROOT, mdDatei), 'utf8').replace(/^---[\s\S]*?---/, '');
    const ausHtml = new Set((html.match(ZAHLEN) ?? []).map((t) => t.replace(/\s+/g, '')));
    const ausMd = new Set((md.match(ZAHLEN) ?? []).map((t) => t.replace(/\s+/g, '')));
    const nurInMd = [...ausMd].filter((z) => !ausHtml.has(z));
    const nurInHtml = [...ausHtml].filter((z) => !ausMd.has(z));
    assert.deepEqual(
      nurInMd,
      [],
      `${mdDatei} nennt Werte, die in ${htmlDatei} nicht vorkommen: ${nurInMd.join(', ')}`,
    );
    assert.deepEqual(
      nurInHtml,
      [],
      `${htmlDatei} nennt Werte, die in ${mdDatei} nicht vorkommen: ${nurInHtml.join(', ')}`,
    );
  }
});

test('jede eingebettete Seite existiert als Datei', () => {
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

test('nimmt nur die Titelueberschrift, nicht die Zwischenueberschriften', () => {
  const fragment = '<h1>Was ist Uplink</h1>\n<p>Text</p><h1>Zweite</h1>';
  const ohne = titelUeberschriftEntfernen(fragment);
  assert.equal(ohne, '<p>Text</p><h1>Zweite</h1>');
});

test('laesst ein Fragment ohne Titelueberschrift unveraendert', () => {
  const fragment = '<p>Nur Text</p><h2>Ablauf</h2>';
  assert.equal(titelUeberschriftEntfernen(fragment), fragment);
});

test('jedes eingebettete Kapitel verliert seinen Titel und behaelt den Inhalt', () => {
  for (const page of UPLINK_HELP_PAGES) {
    const roh = readFileSync(join(HELP_ROOT, page.file), 'utf8');
    const eingebettet = extractUplinkMain(roh);
    assert.ok(
      !eingebettet.includes(`>${page.label}</h2>`),
      `${page.file}: Titel steht doppelt, im Klapptitel und im Inhalt`,
    );
    assert.ok(eingebettet.length > 200, `${page.file}: Inhalt ist verschwunden`);
  }
});

test('die Hilfe startet zugeklappt', () => {
  assert.ok(UPLINK_PAGE.includes('<details'), 'Die Hilfe ist nicht mehr klappbar');
  assert.ok(
    !/<details[^>]*\sopen[\s>]/.test(UPLINK_PAGE),
    'Ein Kapitel startet aufgeklappt',
  );
});

test('Bildpfade in den Fragmenten werden auf die echte Adresse gezogen', () => {
  const html = extractUplinkMain(
    '<main class="uplink-doc" data-doc="obs"><figure><img src="bilder/1-stream.svg" alt="x"></figure></main>',
  );
  assert.match(html, /src="\/uplink\/bilder\/1-stream\.svg"/);
});

test('absolute und Daten-Bildquellen bleiben unberuehrt', () => {
  const html = extractUplinkMain(
    '<main class="uplink-doc" data-doc="obs"><img src="https://x.test/a.png"><img src="/schon/absolut.svg"></main>',
  );
  assert.match(html, /src="https:\/\/x\.test\/a\.png"/);
  assert.match(html, /src="\/schon\/absolut\.svg"/);
});

test('die URL behält Deutsch als Standard und trennt englische Kapitel und Bilder', () => {
  assert.equal(uplinkHelpUrl('obs.html'), uplinkHelpUrl('obs.html', 'de'));
  assert.equal(uplinkHelpUrl('obs.html', 'en'), '/uplink/en/obs.html');
  assert.equal(uplinkHelpUrl('bilder/1-stream.svg', 'en'), '/uplink/en/bilder/1-stream.svg');
  assert.match(UPLINK_PAGE, /queryKey: \['uplink-help', language\]/);
  assert.match(UPLINK_PAGE, /queryFn: \(\) => fetchUplinkHelp\(language\)/);
  assert.match(UPLINK_PAGE, /uplinkHelpUrl\('index.html', language\)/);
  assert.match(UPLINK_PAGE, /uplinkHelpUrl\('obs.html', language\)/);
  assert.match(UPLINK_DESTINATION, /\{[^}]*\blanguage\b[^}]*\} = useLanguage\(\)/);
  assert.match(UPLINK_DESTINATION, /uplinkHelpUrl\('obs.html', language\)/);
  assert.doesNotMatch(UPLINK_DESTINATION, /uplinkHelpUrl\('obs.html'\)/);
});

test('englische Fragmente behalten ihre Sprache in Links mit Ankern und in Bildern', () => {
  const fragment = extractUplinkMain(
    '<main class="uplink-doc" data-doc="obs"><h1>OBS</h1><h2>Output</h2>' +
    '<p class="nur-standalone"><a href="index.html">Back</a></p>' +
    '<a href="obs.html#vod">VOD</a><img src="bilder/1-stream.svg">' +
    '<a href="#vod">Anchor</a><a href="https://example.com/obs.html">External</a></main>',
    'en',
  );
  assert.match(fragment, /href="\/uplink\/en\/obs\.html#vod"/);
  assert.match(fragment, /src="\/uplink\/en\/bilder\/1-stream\.svg"/);
  assert.match(fragment, /href="#vod"/);
  assert.match(fragment, /href="https:\/\/example\.com\/obs\.html"/);
  assert.match(fragment, /<h3>Output<\/h3>/);
  assert.doesNotMatch(fragment, /<h1|<h2|<main|nur-standalone/);
});

test('alle englischen Seiten erhalten Kapitelstruktur, Code, Quellen und lokale Assets', () => {
  for (const [file] of HELP_PAGES) {
    const german = readFileSync(join(HELP_ROOT, file), 'utf8');
    const path = join(HELP_ROOT, 'en', file);
    const english = readFileSync(path, 'utf8');
    assert.match(english, /<html lang="en">/);
    assert.equal((english.match(/<section\b/g) ?? []).length, (german.match(/<section\b/g) ?? []).length);
    assert.equal((english.match(/<h2\b/g) ?? []).length, (german.match(/<h2\b/g) ?? []).length);
    assert.deepEqual(english.match(/<code>[^<]*<\/code>/g), german.match(/<code>[^<]*<\/code>/g));
    assert.deepEqual(english.match(/href="https:[^"]*"/g), german.match(/href="https:[^"]*"/g));
    assert.deepEqual(english.match(/id="[^"]*"/g), german.match(/id="[^"]*"/g));
    for (const match of english.matchAll(/(?:href|src)="(?!https?:)([^"]+)"/g)) {
      assert.ok(existsSync(join(HELP_ROOT, 'en', match[1])), `${file}: ${match[1]}`);
    }
  }
  for (const file of ['1-stream.svg', '2-docks-menue.svg', '3-docks-dialog.svg', '4-ausgabe.svg']) {
    const german = readFileSync(join(HELP_ROOT, 'bilder', file), 'utf8');
    const english = readFileSync(join(HELP_ROOT, 'en', 'bilder', file), 'utf8');
    assert.equal(english.match(/viewBox="[^"]*"/)?.[0], german.match(/viewBox="[^"]*"/)?.[0]);
    assert.deepEqual(english.match(/<(?:rect|circle|line)\b[^>]*>/g), german.match(/<(?:rect|circle|line)\b[^>]*>/g));
    assert.notEqual(english.match(/<title[^>]*>[\s\S]*?<\/title>/)?.[0], german.match(/<title[^>]*>[\s\S]*?<\/title>/)?.[0]);
  }
});

test('der Loader lädt die echten Kapitel getrennt für Deutsch und Englisch', async (context) => {
  const requests: string[] = [];
  context.mock.method(globalThis, 'fetch', async (input: string, options: RequestInit) => {
    requests.push(input);
    assert.equal(options.credentials, 'same-origin');
    return new Response(readFileSync(join(HELP_ROOT, input.replace('/uplink/', '')), 'utf8'));
  });
  const german = await fetchUplinkHelp();
  const english = await fetchUplinkHelp('en');
  assert.equal(german.length, UPLINK_HELP_PAGES.length);
  assert.equal(english.length, UPLINK_HELP_PAGES.length);
  assert.deepEqual(requests, [
    ...UPLINK_HELP_PAGES.map((page) => uplinkHelpUrl(page.file)),
    ...UPLINK_HELP_PAGES.map((page) => uplinkHelpUrl(page.file, 'en')),
  ]);
  for (const [index, page] of english.entries()) {
    assert.equal(page.file, german[index].file);
    assert.notEqual(page.html, german[index].html);
    assert.equal(page.html, extractUplinkMain(readFileSync(join(HELP_ROOT, 'en', page.file), 'utf8'), 'en'));
  }
});

test('ein fehlendes englisches Kapitel ersetzt die anderen nicht durch Deutsch', async (context) => {
  context.mock.method(globalThis, 'fetch', async (input: string) => {
    assert.match(input, /^\/uplink\/en\//);
    return input.endsWith('obs.html')
      ? new Response('', { status: 404 })
      : new Response(readFileSync(join(HELP_ROOT, input.replace('/uplink/', '')), 'utf8'));
  });
  const pages = await fetchUplinkHelp('en');
  assert.deepEqual(pages.map((page) => page.file), ['was-ist.html', 'stoerungen.html']);
});

test('wenn alle Kapitel fehlen, schlägt der Loader weiter fehl', async (context) => {
  context.mock.method(globalThis, 'fetch', async () => new Response('', { status: 404 }));
  await assert.rejects(fetchUplinkHelp('en'), /404/);
  await assert.rejects(fetchUplinkHelp('de'), /404/);
});

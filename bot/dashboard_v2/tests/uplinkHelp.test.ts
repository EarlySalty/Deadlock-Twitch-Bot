import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import {
  extractUplinkMain,
  titelUeberschriftEntfernen,
  UPLINK_HELP_PAGES,
  uplinkHelpUrl,
} from '../src/uplinkHelp';

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

test('extrahiert nur main.uplink-doc und legt die Ueberschriften eine Ebene tiefer', () => {
  // Zwei Dinge in einem Durchlauf: die Titel-h1 faellt weg, weil ihr Text schon
  // im Klapptitel steht, und alles Uebrige rutscht eine Ebene tiefer. Die Seite
  // hat bereits eine h1, und drei Fragmente mit eigener h1 zerlegten die
  // Ueberschriftenstruktur fuer Screenreader.
  const html =
    '<body><main class="uplink-doc" data-doc="obs"><h1>OBS</h1><h2>Ablauf</h2><p>Text</p></main></body>';
  assert.equal(
    extractUplinkMain(html),
    '<div class="uplink-doc" data-doc="obs"><h3>Ablauf</h3><p>Text</p></div>',
  );
});

test('nimmt nur bis zum ersten schliessenden main', () => {
  // Gieriges Matching zoege fremdes Markup in das dangerouslySetInnerHTML.
  const html =
    '<main class="uplink-doc" data-doc="obs"><p>A</p></main><main><p>fremd</p></main>';
  assert.equal(extractUplinkMain(html), '<div class="uplink-doc" data-doc="obs"><p>A</p></div>');
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

/**
 * Derselbe Inhalt liegt zweimal: als HTML unter public/uplink/ und als Markdown
 * unter rust/knowledge/bot/. Formuliert sind beide bewusst verschieden, die
 * harten Zahlen muessen aber uebereinstimmen: laufen sie auseinander, gibt der
 * Chatbot eine andere Empfehlung als die Hilfeseite.
 */
test('Zahlen in HTML- und Markdown-Fassung stimmen ueberein', () => {
  const KNOWLEDGE_ROOT = join(DASHBOARD_ROOT, '../../rust/knowledge/bot');
  const PAARE = [
    ['obs.html', 'uplink-obs.md'],
    ['was-ist.html', 'uplink-was-ist.md'],
    ['stoerungen.html', 'uplink-stoerungen.md'],
  ] as const;
  // Zahlen mit Einheit: Bitraten (auch nackte vierstellige), Keyframe-Sekunden,
  // Aufloesungen, CQP-Werte.
  const ZAHLEN =
    /\b\d+(?:[.,]\d+)?\s*(?:kbps|Kbps|kbit\/s|Mbit\/s|Mbit|mbit|s\b|p\b|fps)|\b\d{4}\b/g;

  for (const [htmlDatei, mdDatei] of PAARE) {
    const html = readFileSync(join(HELP_ROOT, htmlDatei), 'utf8').replace(/<[^>]+>/g, ' ');
    // Frontmatter raus: last_updated traegt eine Jahreszahl, die in der
    // HTML-Fassung nichts zu suchen hat.
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

test('nimmt nur die Titelueberschrift, nicht die Zwischenueberschriften', () => {
  // Der Kapitelname steht eingebettet schon im Klapptitel. Bliebe die h1 im
  // Inhalt, stuende derselbe Satz zweimal untereinander.
  const fragment = '<h1>Was ist Uplink</h1>\n<p>Text</p><h1>Zweite</h1>';
  const ohne = titelUeberschriftEntfernen(fragment);
  assert.equal(ohne, '<p>Text</p><h1>Zweite</h1>');
});

test('laesst ein Fragment ohne Titelueberschrift unveraendert', () => {
  const fragment = '<p>Nur Text</p><h2>Ablauf</h2>';
  assert.equal(titelUeberschriftEntfernen(fragment), fragment);
});

test('jedes eingebettete Kapitel verliert seinen Titel und behaelt den Inhalt', () => {
  // Gegen den echten Dateien, nicht gegen ein gebasteltes Fragment: hier faellt
  // auf, wenn eine Hilfeseite ihre Struktur aendert.
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
  // Ein `open` am details-Element machte den Umbau wirkungslos: die Hilfe
  // fuellte wieder mehrere Bildschirmhoehen.
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

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { erkenneBrowser, kartenPosition, lesezeichenAnleitung } from '../src/utils/browserErkennung';

const CHROME_WIN =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';
const CHROME_MAC =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';
const EDGE_WIN = `${CHROME_WIN} Edg/120.0.0.0`;
const OPERA_WIN = `${CHROME_WIN} OPR/106.0.0.0`;
const VIVALDI_WIN = `${CHROME_WIN} Vivaldi/6.5.3206.63`;
const FIREFOX_WIN = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:120.0) Gecko/20100101 Firefox/120.0';
const SAFARI_MAC =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15';
const ANDROID_CHROME =
  'Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36';
const IOS_SAFARI =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1';

const anleitungFuer = (eingabe: {
  userAgent: string;
  brave?: boolean;
  platform?: string;
  mobile?: boolean;
}) =>
  lesezeichenAnleitung(
    erkenneBrowser({
      userAgent: eingabe.userAgent,
      brave: eingabe.brave ?? false,
      platform: eingabe.platform ?? 'Win32',
      mobile: eingabe.mobile ?? false,
    }),
  );

test('Brave erkennt den Stern links neben der Adresse', () => {
  const erkennung = erkenneBrowser({
    userAgent: CHROME_WIN,
    brave: true,
    platform: 'Win32',
    mobile: false,
  });
  assert.equal(erkennung.browser, 'brave');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.position, 'links');
  assert.equal(anleitung.symbol, 'stern');
});

test('Chrome, Edge, Firefox und Vivaldi zeigen den Stern rechts', () => {
  for (const userAgent of [CHROME_WIN, EDGE_WIN, FIREFOX_WIN, VIVALDI_WIN]) {
    const anleitung = anleitungFuer({ userAgent });
    assert.equal(anleitung.position, 'rechts', userAgent);
    assert.equal(anleitung.symbol, 'stern', userAgent);
  }
  assert.equal(erkenneBrowser({ userAgent: EDGE_WIN, brave: false, platform: 'Win32', mobile: false }).browser, 'edge');
  assert.equal(
    erkenneBrowser({ userAgent: VIVALDI_WIN, brave: false, platform: 'Win32', mobile: false }).browser,
    'vivaldi',
  );
  assert.equal(
    erkenneBrowser({ userAgent: FIREFOX_WIN, brave: false, platform: 'Win32', mobile: false }).browser,
    'firefox',
  );
  assert.equal(
    erkenneBrowser({ userAgent: CHROME_WIN, brave: false, platform: 'Win32', mobile: false }).browser,
    'chrome',
  );
});

test('Opera zeigt das Herz rechts', () => {
  const erkennung = erkenneBrowser({ userAgent: OPERA_WIN, brave: false, platform: 'Win32', mobile: false });
  assert.equal(erkennung.browser, 'opera');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.position, 'rechts');
  assert.equal(anleitung.symbol, 'herz');
});

test('Safari auf macOS nutzt den Teilen-Knopf', () => {
  const erkennung = erkenneBrowser({ userAgent: SAFARI_MAC, brave: false, platform: 'MacIntel', mobile: false });
  assert.equal(erkennung.browser, 'safari');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.symbol, 'teilen');
  assert.deepEqual(anleitung.tastenkombi, ['⌘', 'D']);
});

test('Android bekommt den Menue-Text statt einer Pfeilkarte', () => {
  const erkennung = erkenneBrowser({
    userAgent: ANDROID_CHROME,
    brave: false,
    platform: 'Linux armv8l',
    mobile: true,
  });
  assert.equal(erkennung.mobil, 'android');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.position, 'menue');
  assert.match(anleitung.hinweis, /Stern/);
});

test('iOS bekommt den Home-Bildschirm-Text', () => {
  const erkennung = erkenneBrowser({
    userAgent: IOS_SAFARI,
    brave: false,
    platform: 'iPhone',
    mobile: true,
  });
  assert.equal(erkennung.mobil, 'ios');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.position, 'menue');
  assert.match(anleitung.hinweis, /Home-Bildschirm/);
});

test('macOS liefert das Cmd-Kuerzel, Windows das Strg-Kuerzel', () => {
  const mac = anleitungFuer({ userAgent: CHROME_MAC, platform: 'MacIntel' });
  assert.deepEqual(mac.tastenkombi, ['⌘', 'D']);
  const win = anleitungFuer({ userAgent: CHROME_WIN, platform: 'Win32' });
  assert.deepEqual(win.tastenkombi, ['Strg', 'D']);
});

test('Brave setzt die Karte links der Mitte mit Pfeil links', () => {
  const pos = kartenPosition(anleitungFuer({ userAgent: CHROME_WIN, brave: true }));
  assert.equal(pos.seite, 'links');
  assert.equal(pos.top, '16px');
  assert.equal(pos.left, 'max(16px, calc(50% - 560px))');
  assert.equal(pos.right, undefined);
  assert.equal(pos.pfeilLinks, true);
});

test('Chrome setzt die Karte rechts mit 140px Abstand und Pfeil rechts', () => {
  const pos = kartenPosition(anleitungFuer({ userAgent: CHROME_WIN }));
  assert.equal(pos.seite, 'rechts');
  assert.equal(pos.top, '16px');
  assert.equal(pos.right, '140px');
  assert.equal(pos.left, undefined);
  assert.equal(pos.pfeilLinks, false);
});

test('Safari setzt die Karte rechts mit 60px Abstand', () => {
  const pos = kartenPosition(anleitungFuer({ userAgent: SAFARI_MAC, platform: 'MacIntel' }));
  assert.equal(pos.seite, 'rechts');
  assert.equal(pos.right, '60px');
  assert.equal(pos.pfeilLinks, false);
});

test('Mobil bleibt beim Menue statt einer Pfeilkarte', () => {
  const pos = kartenPosition(anleitungFuer({ userAgent: ANDROID_CHROME, platform: 'Linux armv8l', mobile: true }));
  assert.equal(pos.seite, 'menue');
  assert.equal(pos.top, undefined);
  assert.equal(pos.pfeilLinks, false);
});

test('Unbekannter Browser bleibt ohne Positionsangabe', () => {
  const erkennung = erkenneBrowser({
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64) UnknownBrowser/1.0',
    brave: false,
    platform: 'Linux x86_64',
    mobile: false,
  });
  assert.equal(erkennung.browser, 'unbekannt');
  const anleitung = lesezeichenAnleitung(erkennung);
  assert.equal(anleitung.position, 'unbekannt');
  assert.equal(anleitung.symbol, null);
});

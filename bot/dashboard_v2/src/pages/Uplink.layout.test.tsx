/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const PAGES_ROOT = import.meta.dirname;
const UPLINK = readFileSync(join(PAGES_ROOT, 'Uplink.tsx'), 'utf8');
const ZIEL = readFileSync(join(PAGES_ROOT, 'UplinkZiel.tsx'), 'utf8');
const FIXTURES = readFileSync(join(PAGES_ROOT, '../preview/fixtures.ts'), 'utf8');

test('der Kopf zeigt nur den Streamstatus und dupliziert keine Plattformzustände', () => {
  assert.doesNotMatch(UPLINK, /data-section="uplink-status"/);
  assert.match(UPLINK, /role="status"[\s\S]{0,500}\{streamStatus\.text\}/);
  assert.match(UPLINK, /Stream offline/);
  assert.doesNotMatch(UPLINK, />OBS verbunden</);
});

test('OBS ist eine geordnete Liste aus vier nativen Disclosures', () => {
  assert.match(UPLINK, /<ol[^>]+aria-label="OBS einrichten"/);
  assert.equal((UPLINK.match(/data-obs-step=/g) ?? []).length, 1);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<details/);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<summary/);
});

test('Disclosure-Zustände werden mit geschlossenen Startwerten gespeichert', () => {
  assert.match(UPLINK, /useUplinkDisclosure\('obs-docks', false\)/);
  assert.match(UPLINK, /useUplinkDisclosure\('uplink-hilfe', false\)/);
  assert.match(UPLINK, /useUplinkDisclosure\(`obs-\$\{nummer\}`, offenStart\)/);
  assert.match(ZIEL, /useUplinkDisclosure\(`plattform-\$\{platform\}`, offenStart\)/);
  assert.match(UPLINK, /data-section="obs-docks"[\s\S]{0,100}open=\{docksOffen\}/);
  assert.match(UPLINK, /data-section="uplink-help"[\s\S]{0,100}open=\{hilfeOffen\}/);
});

test('jede Plattformkarte exponiert Plattform und ausgeschriebenen Zustand', () => {
  assert.match(ZIEL, /data-platform=\{platform\}/);
  assert.match(ZIEL, /data-state=\{/);
  assert.match(ZIEL, /aria-label=\{`\$\{label\}-Einstellungen`\}/);
});

test('alle vier Plattformkarten verwenden echte lokale Logos', () => {
  const markenfarben = {
    twitch: '9146' + 'FF',
    youtube: 'FF00' + '00',
    kick: '53FC' + '18',
    tiktok: '25F4' + 'EE',
  } as const;
  for (const [platform, farbe] of Object.entries(markenfarben)) {
    assert.match(ZIEL, new RegExp(`import ${platform}Logo from '@/assets/platforms/${platform}\\.svg'`));
    const logo = readFileSync(join(PAGES_ROOT, `../assets/platforms/${platform}.svg`), 'utf8');
    assert.match(logo, new RegExp(`fill="#${farbe}"`, 'i'));
  }
  assert.match(ZIEL, /src=\{PLATTFORM_LOGOS\[platform\]\}/);
  assert.doesNotMatch(ZIEL, /const kuerzel/);
});

test('die parallel gelieferte Reconnect-Wartezeit bleibt im neuen Layout erhalten', () => {
  assert.match(UPLINK, /function ReconnectWaitKarte/);
  assert.match(UPLINK, /saveUplinkReconnectWait/);
  assert.match(UPLINK, /<ReconnectWaitKarte/);
  assert.match(FIXTURES, /reconnect_wait_s:\s*90/);
  assert.match(FIXTURES, /reconnect_wait_max_s:\s*300/);
});

test('Laden und Fehler erfinden keine leeren Plattformziele', () => {
  assert.match(UPLINK, /zieleFehler\s*\|\|\s*zieleLaden\s*\?\s*'hidden'/);
  assert.match(UPLINK, /gespeicherteZiele\.length === 0 && !zieleFehler && !zieleLaden/);
  assert.match(UPLINK, /Ziele werden geladen/);
  assert.match(UPLINK, /Status unbekannt/);
});

test('Clipboard-Fehler hinterlassen ein fokussiertes, auswählbares Feld', () => {
  assert.match(UPLINK, /feldRef\.current\?\.focus\(\)/);
  assert.match(UPLINK, /feldRef\.current\?\.select\(\)/);
  assert.match(UPLINK, /readOnly[\s\S]{0,100}type=\{offen \? 'text' : 'password'\}/);
});

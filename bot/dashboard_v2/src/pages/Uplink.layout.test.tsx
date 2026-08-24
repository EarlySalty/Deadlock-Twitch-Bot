/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

const PAGES_ROOT = import.meta.dirname;
const UPLINK = readFileSync(join(PAGES_ROOT, 'Uplink.tsx'), 'utf8');
const ZIEL = readFileSync(join(PAGES_ROOT, 'UplinkZiel.tsx'), 'utf8');

test('der belegbare Uplink-Status steht vor dem Einrichtungsflow', () => {
  const status = UPLINK.indexOf('data-section="uplink-status"');
  const obs = UPLINK.indexOf('aria-label="OBS einrichten"');

  assert.ok(status >= 0, 'Statusleiste fehlt');
  assert.ok(obs > status, 'OBS-Flow steht vor der Statusleiste');
  assert.match(UPLINK, /data-section="uplink-status"[\s\S]{0,500}role="status"/);
  assert.doesNotMatch(UPLINK, />OBS verbunden</);
});

test('OBS ist eine geordnete Liste aus vier nativen Disclosures', () => {
  assert.match(UPLINK, /<ol[^>]+aria-label="OBS einrichten"/);
  assert.equal((UPLINK.match(/data-obs-step=/g) ?? []).length, 1);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<details/);
  assert.match(UPLINK, /function ObsSchritt[\s\S]+<summary/);
});

test('sekundaere Docks und Hilfe starten als geschlossene Disclosures', () => {
  for (const section of ['obs-docks', 'uplink-help']) {
    const disclosure = new RegExp(
      `<details[^>]+data-section="${section}"(?![^>]*\\sopen(?:=|[\\s>]))`,
    );
    assert.match(UPLINK, disclosure, `${section} ist nicht als geschlossenes details markiert`);
  }
});

test('jede Plattformkarte exponiert Plattform und ausgeschriebenen Zustand', () => {
  assert.match(ZIEL, /data-platform=\{platform\}/);
  assert.match(ZIEL, /data-state=\{/);
  assert.match(ZIEL, /aria-label=\{`\$\{label\}-Einstellungen`\}/);
});

test('alle vier Plattformkarten verwenden echte lokale Logos', () => {
  for (const platform of ['twitch', 'youtube', 'kick', 'tiktok']) {
    assert.match(ZIEL, new RegExp(`import ${platform}Logo from '@/assets/platforms/${platform}\\.svg'`));
  }
  assert.match(ZIEL, /src=\{PLATTFORM_LOGOS\[platform\]\}/);
  assert.doesNotMatch(ZIEL, /const kuerzel/);
});

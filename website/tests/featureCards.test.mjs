import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// Die Überschrift der Feature-Sektion nennt die Anzahl der Module als Wort.
// Beim Umbau der Karten ist genau diese Zahl schon einmal stehen geblieben,
// waehrend eine Karte verschwand: der Text sagte sechs, die Seite zeigte fuenf.
const SECTION = fileURLToPath(new URL('../src/components/sections/Features.tsx', import.meta.url));
const DATA = fileURLToPath(new URL('../src/data/features.ts', import.meta.url));
const CARD = fileURLToPath(new URL('../src/components/ui/FeatureCard.tsx', import.meta.url));

const ZAHLWORT = ['null', 'eine', 'zwei', 'drei', 'vier', 'fünf', 'sechs', 'sieben', 'acht', 'neun', 'zehn'];

function featureIds() {
  return [...readFileSync(DATA, 'utf8').matchAll(/^\s{4}id:\s*"([^"]+)"/gm)].map((m) => m[1]);
}

test('die Subline nennt so viele Module, wie Karten existieren', () => {
  const ids = featureIds();
  assert.ok(ids.length > 0, 'keine Feature-Karten gefunden');
  assert.ok(ids.length < ZAHLWORT.length, `Zahlwort fuer ${ids.length} Karten fehlt im Test`);

  const subtitle = readFileSync(SECTION, 'utf8').match(/subtitle="([^"]+)"/);
  assert.ok(subtitle, 'subtitle der Feature-Sektion nicht gefunden');

  const erwartet = ZAHLWORT[ids.length];
  assert.match(
    subtitle[1].toLowerCase(),
    new RegExp(`\\b${erwartet}\\b`),
    `Subline nennt nicht "${erwartet}" Module, obwohl ${ids.length} Karten existieren`,
  );
});

test('jede Feature-Karte hat ein Icon in der iconMap', () => {
  const data = readFileSync(DATA, 'utf8');
  const icons = [...data.matchAll(/^\s{4}icon:\s*"([^"]+)"/gm)].map((m) => m[1]);
  assert.equal(icons.length, featureIds().length, 'Karte ohne icon-Feld');

  const map = readFileSync(CARD, 'utf8').match(/const iconMap[^=]*=\s*\{([\s\S]*?)\n\}/);
  assert.ok(map, 'iconMap nicht gefunden');
  const bekannt = new Set(map[1].split(',').map((s) => s.trim()).filter(Boolean));

  // Fehlt das Icon, rendert FeatureCard stumm den ersten Buchstaben des Namens
  // statt eines Symbols. Das faellt im Build nicht auf.
  assert.deepEqual(icons.filter((i) => !bekannt.has(i)), []);
});

test('Feature-Ids sind eindeutig, sonst kollidiert der React-key', () => {
  const ids = featureIds();
  assert.deepEqual([...new Set(ids)], ids);
});

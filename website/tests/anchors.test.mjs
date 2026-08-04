import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

// Ein globales Suchen-Ersetzen von Hex-Farben hat schon einmal "#features" zu
// "#c8a86btures" verstümmelt: dreistellige Farben wie #fea matchen mitten im Wort.
// Jeder Sprungmarken-Link muss deshalb ein reales id-Ziel haben.

const SRC = new URL('../src', import.meta.url).pathname;

function sourceFiles(dir) {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry) ? [path] : [];
  });
}

const files = sourceFiles(SRC);
const ids = new Set();
const anchors = [];

for (const file of files) {
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(/id="([A-Za-z0-9_-]+)"/g)) ids.add(m[1]);
  // href/to als JSX-Prop (=) oder als Objekt-Key (:), auch in Template-Literalen
  for (const m of text.matchAll(/(?:href|to)\s*[:=]\s*[{"'`][^"'`]*#([A-Za-z0-9_-]+)/g)) {
    anchors.push({ anchor: m[1], file: relative(SRC, file) });
  }
}

test('jeder Sprungmarken-Link zeigt auf eine existierende id', () => {
  const tot = anchors.filter(({ anchor }) => !ids.has(anchor));
  assert.deepEqual(
    tot.map(({ anchor, file }) => `#${anchor} (${file})`),
    [],
    'Sprungmarken ohne Ziel',
  );
});

// Die Navbar verlinkt ihre Sektionen nicht per href, sondern über den
// Objekt-Schlüssel `id` in NAV_LINKS. Der Test oben sieht diese Links deshalb
// nicht: ein Menüpunkt auf eine geloeschte oder umbenannte Sektion scrollt
// stillschweigend nirgendwohin.
test('jeder Navbar-Menuepunkt zeigt auf eine existierende Sektion', () => {
  const navbar = readFileSync(join(SRC, 'components/layout/Navbar.tsx'), 'utf8');
  const block = navbar.match(/const NAV_LINKS[^=]*=\s*\[([\s\S]*?)\n\]/);
  assert.ok(block, 'NAV_LINKS nicht gefunden');

  const menuIds = [...block[1].matchAll(/\bid:\s*'([^']+)'/g)].map((m) => m[1]);
  assert.ok(menuIds.length > 0, 'keine Sektions-Menuepunkte gefunden');

  // Nur die Sektionen der Landing zaehlen. Der globale id-Pool des Tests oben
  // sammelt auch aus v2/ und den Unterseiten: "ablauf" existiert dort ebenfalls,
  // eine geloeschte Landing-Sektion bliebe damit unbemerkt.
  const sectionIds = new Set(
    sourceFiles(join(SRC, 'components/sections'))
      .flatMap((file) => [...readFileSync(file, 'utf8').matchAll(/id="([A-Za-z0-9_-]+)"/g)])
      .map((m) => m[1]),
  );

  assert.deepEqual(
    menuIds.filter((id) => !sectionIds.has(id)),
    [],
    'Menuepunkte ohne Sektion',
  );
});

test('Anker enthalten keinen eingebetteten Hex-Farbwert', () => {
  const verstuemmelt = anchors.filter(({ anchor }) => /^[0-9a-f]{6}[a-z]/i.test(anchor));
  assert.deepEqual(verstuemmelt.map((a) => `#${a.anchor} (${a.file})`), []);
});

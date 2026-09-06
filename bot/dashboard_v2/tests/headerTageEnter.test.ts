/// <reference types="node" />
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';

import { clampDays } from '../src/utils/zeitraum';

const HEADER = readFileSync(
  join(import.meta.dirname, '..', 'src', 'components', 'layout', 'Header.tsx'),
  'utf8',
);

function uebernahme(tageInput: string, days: number): { onDaysChange: number | null; feld: string } {
  const parsed = Number.parseInt(tageInput, 10);
  if (!Number.isFinite(parsed)) {
    return { onDaysChange: null, feld: String(days) };
  }
  const naechster = clampDays(parsed);
  return { onDaysChange: naechster !== days ? naechster : null, feld: String(naechster) };
}

test('eine gueltige Zahl loest onDaysChange aus und setzt den Feldwert', () => {
  assert.deepEqual(uebernahme('14', 30), { onDaysChange: 14, feld: '14' });
});

test('ein Wert unter der Untergrenze wird auf 7 geklemmt', () => {
  assert.deepEqual(uebernahme('3', 30), { onDaysChange: 7, feld: '7' });
});

test('ein Wert ueber der Obergrenze wird auf 365 geklemmt', () => {
  assert.deepEqual(uebernahme('400', 30), { onDaysChange: 365, feld: '365' });
});

test('ein leeres Feld aendert nichts und stellt den aktuellen Wert wieder her', () => {
  assert.deepEqual(uebernahme('', 30), { onDaysChange: null, feld: '30' });
});

test('der gleiche Wert loest kein onDaysChange aus', () => {
  assert.deepEqual(uebernahme('30', 30), { onDaysChange: null, feld: '30' });
});

test('das Tage-Feld faengt Enter ab, uebernimmt und gibt den Fokus frei', () => {
  const inputStart = HEADER.indexOf('type="number"');
  assert.ok(inputStart >= 0, 'das Tage-Feld muss ein Zahlenfeld sein');
  const inputBlock = HEADER.slice(inputStart, inputStart + 600);
  assert.match(inputBlock, /onKeyDown=\{event =>/, 'das Tage-Feld braucht einen onKeyDown-Handler');
  assert.match(inputBlock, /event\.key === 'Enter'/, 'Enter muss abgefangen werden');
  assert.match(inputBlock, /uebernehmeTage\(\);/, 'Enter muss die Uebernahme ausloesen');
  assert.match(inputBlock, /event\.currentTarget\.blur\(\)/, 'Enter muss den Fokus freigeben');
  assert.match(inputBlock, /onBlur=\{uebernehmeTage\}/, 'Verlassen des Feldes muss ebenfalls uebernehmen');
});

test('die Uebernahme klemmt, meldet nur echte Aenderungen und faengt leere Eingaben ab', () => {
  const start = HEADER.indexOf('const uebernehmeTage');
  assert.ok(start >= 0, 'uebernehmeTage muss existieren');
  const block = HEADER.slice(start, start + 400);
  assert.match(block, /Number\.parseInt\(tageInput, 10\)/);
  assert.match(block, /!Number\.isFinite\(parsed\)/, 'leere oder ungueltige Eingabe muss abgefangen werden');
  assert.match(block, /clampDays\(parsed\)/, 'die Eingabe muss geklemmt werden');
  assert.match(block, /naechster !== days/, 'onDaysChange darf nur bei echter Aenderung laufen');
  assert.match(block, /onDaysChange\(naechster\)/);
});

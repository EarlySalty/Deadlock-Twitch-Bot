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
    return { onDaysChange: null, feld: '' };
  }
  const naechster = clampDays(parsed);
  return { onDaysChange: naechster !== days ? naechster : null, feld: '' };
}

test('eine gueltige Zahl loest onDaysChange aus und leert danach das Eingabefeld', () => {
  assert.deepEqual(uebernahme('14', 30), { onDaysChange: 14, feld: '' });
});

test('ein Wert unter der Untergrenze wird auf 7 geklemmt', () => {
  assert.deepEqual(uebernahme('3', 30), { onDaysChange: 7, feld: '' });
});

test('mehr als ein Jahr wird uebernommen und erst bei zehn Jahren geklemmt', () => {
  assert.deepEqual(uebernahme('400', 30), { onDaysChange: 400, feld: '' });
  assert.deepEqual(uebernahme('730', 30), { onDaysChange: 730, feld: '' });
  assert.deepEqual(uebernahme('5000', 30), { onDaysChange: 3650, feld: '' });
});

test('ein leeres Feld aendert nichts und bleibt leer', () => {
  assert.deepEqual(uebernahme('', 30), { onDaysChange: null, feld: '' });
});

test('der gleiche Wert loest kein onDaysChange aus', () => {
  assert.deepEqual(uebernahme('30', 30), { onDaysChange: null, feld: '' });
});

test('das Tage-Feld faengt Enter ab, uebernimmt und gibt den Fokus frei', () => {
  const labelPos = HEADER.indexOf("aria-label={t('Tage')}");
  assert.ok(labelPos >= 0, 'das Tage-Feld muss per aria-label Tage erkennbar sein');
  const feldStart = HEADER.lastIndexOf('<input', labelPos);
  assert.ok(feldStart >= 0 && HEADER.slice(feldStart, labelPos).includes('type="number"'), 'das Tage-Feld muss ein Zahlenfeld sein');
  const inputBlock = HEADER.slice(feldStart, feldStart + 700);
  assert.match(inputBlock, /onKeyDown=\{event =>/, 'das Tage-Feld braucht einen onKeyDown-Handler');
  assert.match(inputBlock, /event\.key === 'Enter'/, 'Enter muss abgefangen werden');
  assert.match(inputBlock, /uebernehmeTage\(\);/, 'Enter muss die Uebernahme ausloesen');
  assert.match(inputBlock, /event\.currentTarget\.blur\(\)/, 'Enter muss den Fokus freigeben');
  assert.match(inputBlock, /onBlur=\{uebernehmeTage\}/, 'Verlassen des Feldes muss ebenfalls uebernehmen');
});

test('die Uebernahme klemmt, meldet nur echte Aenderungen und faengt leere Eingaben ab', () => {
  const start = HEADER.indexOf('const uebernehmeTage');
  assert.ok(start >= 0, 'uebernehmeTage muss existieren');
  const block = HEADER.slice(start, start + 500);
  assert.match(block, /Number\.parseInt\(tageInput, 10\)/);
  assert.match(block, /!Number\.isFinite\(parsed\)/, 'leere oder ungueltige Eingabe muss abgefangen werden');
  assert.match(block, /clampDays\(parsed\)/, 'die Eingabe muss geklemmt werden');
  assert.match(block, /naechster !== days/, 'onDaysChange darf nur bei echter Aenderung laufen');
  assert.match(block, /onDaysChange\(naechster\)/);
});

test('eine manuell eingegebene 365 bleibt am Tage-Feld sichtbar ausgewaehlt', () => {
  assert.match(HEADER, /customRangeSelected/, 'Header muss die Quelle der Zeitraumwahl merken');
  assert.match(HEADER, /setCustomRangeSelected\(true\)/, 'Tippen im Tage-Feld muss den manuellen Zustand aktivieren');
  assert.match(HEADER, /setCustomRangeSelected\(false\)/, 'Preset-Klicks muessen den manuellen Zustand deaktivieren');
  assert.match(HEADER, /\{customRangeSelected && \(/, 'der goldene Auswahlmarker muss vom manuellen Zustand abhaengen');
});

test('das Tage-Feld bietet mehr als ein Jahr ohne native Spinner an', () => {
  const labelPos = HEADER.indexOf("aria-label={t('Tage')}");
  const feldStart = HEADER.lastIndexOf('<input', labelPos);
  const inputBlock = HEADER.slice(feldStart, feldStart + 1100);
  assert.match(inputBlock, /max=\{MAX_ANALYTICS_DAYS\}/);
  assert.match(inputBlock, /appearance-none/, 'native Zahlenspinner sollen nicht die Auswahl verdecken');
});

test('der aktuelle Zeitraum ist nur Placeholder und echte Eingabe ist weiss', () => {
  assert.match(HEADER, /const \[tageInput, setTageInput\] = useState\(''\)/);
  const labelPos = HEADER.indexOf("aria-label={t('Tage')}");
  const feldStart = HEADER.lastIndexOf('<input', labelPos);
  const inputBlock = HEADER.slice(feldStart, feldStart + 1200);
  assert.match(inputBlock, /placeholder=\{String\(days\)\}/, 'aktueller Zeitraum darf nur als Placeholder sichtbar sein');
  assert.match(inputBlock, /text-white/, 'vom Nutzer eingegebene Zahl muss weiss sein');
  assert.match(inputBlock, /placeholder:text-text-secondary\/70/, 'Placeholder muss optisch im Hintergrund bleiben');
  assert.match(inputBlock, /onFocus=\{\(\) => setCustomRangeSelected\(true\)\}/);
  const commitStart = HEADER.indexOf('const uebernehmeTage');
  assert.match(HEADER.slice(commitStart, commitStart + 650), /setTageInput\(''\)/, 'nach Uebernahme muss das Feld wieder leer sein');
});

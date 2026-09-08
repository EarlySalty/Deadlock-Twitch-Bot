import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import { UplinkHochkantEditor } from '../src/components/uplink/UplinkHochkantEditor';
import { hochkantAnfang } from '../src/components/uplink/hochkantLayout';
import type { HochkantLayout, HochkantStand, UplinkHochkantEditorProps } from '../src/components/uplink/UplinkHochkantEditor';

const ZIEL = { breite: 1080, hoehe: 1920 };
const HD = { breite: 1920, hoehe: 1080 };

function basis(): UplinkHochkantEditorProps {
  return {
    quelle: HD,
    ziel: ZIEL,
    standbildUrl: null,
    standbildHinweis: null,
    stand: null,
    beschaeftigt: false,
    fehlerText: null,
    onSpeichern: () => {},
    onZuruecksetzen: () => {},
  };
}

function render(props: UplinkHochkantEditorProps): string {
  return renderToStaticMarkup(<UplinkHochkantEditor {...props} />);
}

test('ohne Stand zeigt der Editor den Anfangswert und den Leertext', () => {
  const html = render(basis());
  assert.ok(html.includes('Noch kein Stand gespeichert.'));
  assert.ok(html.includes('Gameplay 608×1080 Pixel, Kamera 384×384 Pixel'));
});

test('mit Stand und aktiver Revision stehen beide Saetze', () => {
  const stand: HochkantStand = {
    revision: 3,
    layout: hochkantAnfang(HD, ZIEL),
    gespeichertAm: '2026-09-08T10:00:00Z',
    aktiveRevision: 2,
  };
  const html = render({ ...basis(), stand });
  assert.ok(html.includes('Gespeichert als Stand 3'));
  assert.ok(html.includes('Im laufenden Stream aktiv: Stand 2. Ein neuer Stand gilt ab dem nächsten Stream.'));
});

test('ohne Quelle steht der 1920x1080-Hinweis', () => {
  const html = render({ ...basis(), quelle: null });
  assert.ok(html.includes('Vorschau nimmt 1920×1080 an, bis dein Stream gemessen ist.'));
});

test('genau drei Modus-Radios', () => {
  const html = render(basis());
  assert.equal((html.match(/type="radio"/g) ?? []).length, 3);
});

test('Kamera aus rendert keinen Kamerarahmen', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'nur_gameplay',
    gameplay: hochkantAnfang(HD, ZIEL).gameplay,
    kamera: null,
    kameraBand: null,
    kameraBox: null,
  };
  const stand: HochkantStand = { revision: 1, layout, gespeichertAm: null, aktiveRevision: null };
  const html = render({ ...basis(), stand });
  assert.ok(!html.includes('Kamera-Rahmen'));
});

test('beschaeftigt sperrt beide Knoepfe', () => {
  const html = render({ ...basis(), beschaeftigt: true });
  assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
});

test('Prueffehler sperren Speichern und zeigen den Grund', () => {
  const layout: HochkantLayout = {
    version: 1,
    modus: 'nur_gameplay',
    gameplay: { x: 0.2, y: 0.2, w: 0.6, h: 0.6 },
    kamera: null,
    kameraBand: null,
    kameraBox: null,
  };
  const stand: HochkantStand = { revision: 1, layout, gespeichertAm: null, aktiveRevision: null };
  const html = render({ ...basis(), stand });
  assert.ok(html.includes('Gameplay-Seitenverhältnis passt nicht'));
  assert.ok((html.match(/disabled=""/g) ?? []).length >= 1);
});

test('Quelltext bleibt frei von Fetch, Query, Storage, Gedankenstrich und Social-Media-Importen', () => {
  const gedankenstrich = String.fromCharCode(0x2014);
  const dateien = ['../src/components/uplink/UplinkHochkantEditor.tsx', '../src/components/uplink/hochkantLayout.ts'];
  for (const rel of dateien) {
    const text = readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8');
    assert.ok(!text.includes('fetch('), `${rel} enthaelt fetch(`);
    assert.ok(!text.includes('useQuery'), `${rel} enthaelt useQuery`);
    assert.ok(!text.includes('localStorage'), `${rel} enthaelt localStorage`);
    assert.ok(!text.includes(gedankenstrich), `${rel} enthaelt einen Gedankenstrich`);
    assert.ok(!text.includes('socialmedia'), `${rel} importiert socialmedia`);
    assert.ok(!text.includes('utils/socialMediaLayout'), `${rel} importiert socialMediaLayout`);
  }
});

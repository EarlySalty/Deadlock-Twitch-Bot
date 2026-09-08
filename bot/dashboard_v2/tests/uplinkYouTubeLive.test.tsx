import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

(globalThis as { React?: typeof React }).React = React;

import {
  UplinkYouTubeLive,
  entwurfAus,
  speichernErlaubt,
  type UplinkYouTubeLiveProps,
  type YouTubeLiveEinstellungen,
  type YouTubeLiveStatus,
  type YouTubeLiveZustand,
} from '../src/components/uplink/UplinkYouTubeLive';

const QUELLE = readFileSync(
  join(import.meta.dirname, '../src/components/uplink/UplinkYouTubeLive.tsx'),
  'utf8',
);

function basis(): UplinkYouTubeLiveProps {
  return {
    verbunden: true,
    kanalName: null,
    einstellungen: null,
    status: null,
    beschaeftigt: false,
    fehlerText: null,
    onSpeichern: () => {},
    onBeenden: () => {},
  };
}

function render(teil: Partial<UplinkYouTubeLiveProps>): string {
  return renderToStaticMarkup(<UplinkYouTubeLive {...basis()} {...teil} />);
}

function inputMit(html: string, teilAttribut: string): string {
  const treffer = html.match(new RegExp(`<input[^>]*${teilAttribut}[^>]*>`));
  assert.ok(treffer, `kein Input mit ${teilAttribut}`);
  return treffer[0];
}

function knopf(html: string, name: string): string {
  const treffer = html.match(new RegExp(`<button[^>]*data-knopf="${name}"[^>]*>`));
  assert.ok(treffer, `kein Knopf ${name}`);
  return treffer[0];
}

function status(zustand: YouTubeLiveZustand, extra: Partial<YouTubeLiveStatus> = {}): YouTubeLiveStatus {
  return { zustand, hinweis: null, broadcastId: null, seit: null, wiederaufnehmbar: false, ...extra };
}

test('nicht verbunden zeigt nur den Hinweis, kein Formular', () => {
  const html = render({ verbunden: false });
  assert.match(html, /Verbinde zuerst dein YouTube-Konto/);
  assert.doesNotMatch(html, /<form/);
  assert.doesNotMatch(html, /<input/);
});

test('ohne Einstellungen sind die Anfangswerte gesetzt', () => {
  const html = render({ einstellungen: null });
  assert.ok(inputMit(html, 'value="private"').includes('checked'));
  assert.ok(!inputMit(html, 'value="unlisted"').includes('checked'));
  assert.ok(!inputMit(html, 'value="public"').includes('checked'));
  assert.ok(!inputMit(html, 'id="[^"]*-auto-start"').includes('checked'));
  assert.ok(!inputMit(html, 'id="[^"]*-auto-stop"').includes('checked'));
  assert.ok(!inputMit(html, 'id="[^"]*-freigabe"').includes('checked'));
  assert.match(html, /Noch nicht freigegeben/);
});

test('mit Freigabe steht das Datum statt der Warnung', () => {
  const einstellungen: YouTubeLiveEinstellungen = {
    titel: 'Ranked heute',
    sichtbarkeit: 'public',
    autoStart: true,
    autoStop: false,
    freigegebenAm: '2026-09-08T10:00:00Z',
  };
  const html = render({ einstellungen });
  assert.match(html, /Freigegeben am/);
  assert.doesNotMatch(html, /Noch nicht freigegeben/);
  assert.ok(inputMit(html, 'id="[^"]*-freigabe"').includes('checked'));
  assert.ok(inputMit(html, 'value="public"').includes('checked'));
});

test('jeder Zustand ergibt seinen Text in Nutzersprache', () => {
  assert.match(render({ status: status('inaktiv') }), /Kein Livestream vorbereitet/);
  assert.match(render({ status: status('vorbereitet') }), /Livestream angelegt, wartet auf dein Bild aus OBS/);
  assert.match(render({ status: status('beendet') }), /Livestream beendet/);
  assert.match(render({ status: status('unklar') }), /Stand wird mit YouTube abgeglichen/);

  const blockiert = render({ status: status('blockiert', { hinweis: 'Kontingent erschöpft' }) });
  assert.match(blockiert, /Angehalten: Kontingent erschöpft/);

  const fehlerWieder = render({ status: status('fehler', { hinweis: 'Netzwerk weg', wiederaufnehmbar: true }) });
  assert.match(fehlerWieder, /Fehler: Netzwerk weg/);
  assert.match(fehlerWieder, /beim nächsten Stream erneut/);

  const fehlerHart = render({ status: status('fehler', { hinweis: 'Kanal gesperrt', wiederaufnehmbar: false }) });
  assert.match(fehlerHart, /Bitte prüfe deinen YouTube-Kanal/);
});

test('live meldet auf YouTube live, sendet bestätigt keine Veröffentlichung', () => {
  const live = render({ status: status('live', { seit: '2026-09-08T18:30:00Z' }) });
  assert.match(live, /Auf YouTube live/);

  const sendet = render({ status: status('sendet') });
  assert.match(sendet, /noch nicht live geschaltet/);
  assert.doesNotMatch(sendet, /Auf YouTube live/);
});

test('die Broadcast-Kennung steht nur in den technischen Details', () => {
  const html = render({ status: status('vorbereitet', { broadcastId: 'BC-XYZ-123' }) });
  const idIndex = html.indexOf('BC-XYZ-123');
  const detailsIndex = html.indexOf('<details');
  assert.ok(idIndex !== -1, 'Broadcast-Kennung fehlt');
  assert.ok(detailsIndex !== -1, 'kein details-Block');
  assert.ok(detailsIndex < idIndex, 'Broadcast-Kennung steht vor dem details-Block');
});

test('Beenden ist nur in den laufenden Zuständen aktiv', () => {
  for (const zustand of ['inaktiv', 'beendet', 'blockiert', 'fehler'] as const) {
    assert.ok(knopf(render({ status: status(zustand) }), 'beenden').includes('disabled=""'), `beenden bei ${zustand}`);
  }
  assert.ok(!knopf(render({ status: status('live') }), 'beenden').includes('disabled=""'));

  const beschaeftigt = render({ status: status('live'), beschaeftigt: true });
  assert.ok(knopf(beschaeftigt, 'beenden').includes('disabled=""'));
  assert.ok(knopf(beschaeftigt, 'speichern').includes('disabled=""'));
});

test('die Quelle hat keinen eigenen Fetch, keine Query, kein localStorage, keinen Gedankenstrich', () => {
  assert.doesNotMatch(QUELLE, /fetch\(/);
  assert.doesNotMatch(QUELLE, /useQuery/);
  assert.doesNotMatch(QUELLE, /localStorage/);
  const gedankenstriche = [String.fromCharCode(0x2014), String.fromCharCode(0x2013)];
  for (const zeichen of gedankenstriche) {
    assert.ok(!QUELLE.includes(zeichen), 'Gedankenstrich in der Quelle');
  }
});

test('die Sichtbarkeit hat genau drei Radios mit den erwarteten Werten', () => {
  const html = render({});
  assert.equal((html.match(/type="radio"/g) ?? []).length, 3);
  assert.match(html, /value="private"/);
  assert.match(html, /value="unlisted"/);
  assert.match(html, /value="public"/);
});

test('entwurfAus setzt die Anfangswerte und übernimmt gespeicherte Werte', () => {
  assert.deepEqual(entwurfAus(null), {
    titel: '',
    sichtbarkeit: 'private',
    autoStart: false,
    autoStop: false,
    liveFreigeben: false,
  });
  assert.deepEqual(
    entwurfAus({ titel: 'Abend', sichtbarkeit: 'unlisted', autoStart: true, autoStop: true, freigegebenAm: '2026-09-08T00:00:00Z' }),
    { titel: 'Abend', sichtbarkeit: 'unlisted', autoStart: true, autoStop: true, liveFreigeben: true },
  );
});

test('speichernErlaubt sperrt leeren und zu langen Titel', () => {
  const basisEntwurf = entwurfAus(null);
  assert.equal(speichernErlaubt({ ...basisEntwurf, titel: '' }), false);
  assert.equal(speichernErlaubt({ ...basisEntwurf, titel: '   ' }), false);
  assert.equal(speichernErlaubt({ ...basisEntwurf, titel: 'a'.repeat(101) }), false);
  assert.equal(speichernErlaubt({ ...basisEntwurf, titel: 'Ranked Grind' }), true);
  assert.equal(speichernErlaubt({ ...basisEntwurf, titel: 'a'.repeat(100) }), true);
});

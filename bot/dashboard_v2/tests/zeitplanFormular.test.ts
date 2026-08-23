/**
 * Abgleich des Zeitplan-Formulars mit der Serverantwort.
 *
 * Jedes Feld der Zeitplan-Karte schickt beim Verlassen eine eigene Mutation
 * los. Deren Antwort bringt einen neuen Serverstand mit, und der Abgleich hat
 * das Formular frueher komplett ueberschrieben: wer zwei Felder schnell
 * hintereinander aenderte, verlor die zweite Eingabe, sobald die Antwort auf
 * die erste eintraf.
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  zeitplanFeldSchluessel,
  zeitplanFormularAbgleichen,
  type ZeitplanFormular,
} from '../src/components/socialmedia/kartenZustand';

function formular(
  postsProWoche: string,
  maxProTag: string,
  zeiten: string,
): ZeitplanFormular {
  return { postsProWoche, maxProTag, zeiten };
}

test('Serverantwort ueberschreibt Felder, an denen niemand haengt', () => {
  const abgeglichen = zeitplanFormularAbgleichen(
    { youtube: formular('4', '1', '18:00') },
    { youtube: formular('5', '2', '18:00, 21:00') },
    new Set(),
  );
  assert.deepEqual(abgeglichen.youtube, formular('5', '2', '18:00, 21:00'));
});

test('eine noch nicht abgeschickte Eingabe ueberlebt die Antwort auf ein anderes Feld', () => {
  // Der Nutzer aendert "Posts pro Woche" auf 5 und verlaesst das Feld: die
  // Mutation laeuft. Danach tippt er "Hoechstens pro Tag" auf 3, ohne das Feld
  // zu verlassen. Jetzt trifft die Antwort auf die erste Mutation ein.
  const lokal = { youtube: formular('5', '3', '18:00') };
  const antwort = { youtube: formular('5', '1', '18:00') };
  const offen = new Set([zeitplanFeldSchluessel('youtube', 'maxProTag')]);

  const abgeglichen = zeitplanFormularAbgleichen(lokal, antwort, offen);
  assert.equal(
    abgeglichen.youtube.maxProTag,
    '3',
    'die zweite Eingabe darf nicht verloren gehen',
  );
  assert.equal(abgeglichen.youtube.postsProWoche, '5');
});

test('der Server gewinnt wieder, sobald das Feld abgeschickt ist', () => {
  // Das Backend sortiert und entdoppelt die Zeiten. Nach dem Abschicken gilt
  // das Feld nicht mehr als offen, die normalisierte Fassung muss ankommen.
  const abgeglichen = zeitplanFormularAbgleichen(
    { youtube: formular('4', '1', '21:00, 18:00, 18:00') },
    { youtube: formular('4', '1', '18:00, 21:00') },
    new Set(),
  );
  assert.equal(abgeglichen.youtube.zeiten, '18:00, 21:00');
});

test('offene Felder anderer Plattformen bleiben unberuehrt', () => {
  const abgeglichen = zeitplanFormularAbgleichen(
    { youtube: formular('5', '1', '18:00'), tiktok: formular('7', '2', '20:00') },
    { youtube: formular('4', '1', '18:00'), tiktok: formular('7', '2', '20:00') },
    new Set([zeitplanFeldSchluessel('tiktok', 'postsProWoche')]),
  );
  assert.equal(abgeglichen.youtube.postsProWoche, '4');
  assert.equal(abgeglichen.tiktok.postsProWoche, '7');
});

test('eine neue Plattform kommt vollstaendig vom Server', () => {
  const abgeglichen = zeitplanFormularAbgleichen(
    {},
    { instagram: formular('3', '1', '19:00') },
    new Set([zeitplanFeldSchluessel('instagram', 'postsProWoche')]),
  );
  assert.deepEqual(abgeglichen.instagram, formular('3', '1', '19:00'));
});

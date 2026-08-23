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
  zeitplanFeldVerlassen,
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

/**
 * Verlassen eines Feldes mit ungueltiger Eingabe.
 *
 * Das Schliessen des Feldes stand in allen drei `onBlur` hinter dem `return`
 * des Validierungsfehlers. Ein Feld mit ungueltiger Eingabe blieb damit
 * dauerhaft offen, und beim naechsten Serverstand raeumte der Effekt die
 * Fehlermeldung weg, waehrend der Abgleich den ungueltigen Text festhielt.
 */
test('ein Feld gilt nach dem Verlassen als geschlossen, auch bei ungueltiger Eingabe', () => {
  const plan = zeitplanFeldVerlassen({ gueltig: false, fehler: 'Bitte eine Zahl angeben.' });
  assert.equal(plan.schliessen, true, 'verlassen heisst verlassen');
  assert.equal(plan.fehler, 'Bitte eine Zahl angeben.');
  assert.equal(plan.absenden, false, 'ungueltige Eingabe geht nie an den Server');
});

test('ein gueltiger, geaenderter Wert geht an den Server und raeumt den Fehler weg', () => {
  const plan = zeitplanFeldVerlassen({ gueltig: true, unveraendert: false });
  assert.equal(plan.fehler, null);
  assert.equal(plan.schliessen, true);
  assert.equal(plan.absenden, true);
});

test('ein unveraenderter Wert schliesst das Feld ohne Mutation', () => {
  const plan = zeitplanFeldVerlassen({ gueltig: true, unveraendert: true });
  assert.equal(plan.fehler, null);
  assert.equal(plan.schliessen, true);
  assert.equal(plan.absenden, false, 'ohne Aenderung braucht es keine Anfrage');
});

test('eine ungueltige Eingabe bleibt nicht als falscher Wert ohne Hinweis stehen', () => {
  // Der Nutzer tippt "abc" bei "Posts pro Woche" und verlaesst das Feld.
  const offen = new Set<string>();
  const schluessel = zeitplanFeldSchluessel('youtube', 'postsProWoche');
  offen.add(schluessel);
  const lokal = { youtube: formular('abc', '1', '18:00') };

  const plan = zeitplanFeldVerlassen({ gueltig: false, fehler: 'Bitte eine Zahl angeben.' });
  if (plan.schliessen) offen.delete(schluessel);

  // Jetzt trifft ein neuer Serverstand ein. Der Effekt raeumt dabei alle
  // Feldfehler weg, der Abgleich muss deshalb auch den ungueltigen Text
  // ersetzen: sonst steht "abc" ohne Fehlerhinweis im Feld.
  const abgeglichen = zeitplanFormularAbgleichen(
    lokal,
    { youtube: formular('4', '1', '18:00') },
    offen,
  );
  assert.equal(
    abgeglichen.youtube.postsProWoche,
    '4',
    'der Serverstand muss die ungueltige Eingabe ersetzen',
  );
});

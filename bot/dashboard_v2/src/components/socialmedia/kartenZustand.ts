/**
 * Sperre und Fehlerzuordnung der Social-Media-Karten.
 *
 * Warum es das gibt: eine Karte, deren GET gescheitert ist, kennt den
 * gespeicherten Stand nicht. Sie faellt auf ihre Vorgabewerte zurueck ("aus",
 * "Privat", "Nur nach Freigabe", "nicht verbunden") und sieht dabei aus wie
 * eine Karte mit echten Daten. Wer darin klickt, schickt den erfundenen Wert
 * an den Server und schreibt ihn fest. Der Serverschutz "fehlendes Feld bleibt
 * beim bisherigen Wert" hilft dann nicht mehr, weil die Oberflaeche das Feld
 * ausdruecklich mitschickt.
 *
 * Deshalb gilt fuer jede dieser Karten dasselbe Muster statt vier
 * Einzelloesungen: Ladefehler sichtbar machen (siehe `LadeFehlerHinweis`) und
 * die Bedienelemente sperren.
 */

/** Zustand einer Karte, die ihren Stand aus einer Abfrage bezieht. */
export interface KartenZustand {
  /** Die Abfrage laeuft noch, angezeigt werden Vorgabewerte. */
  isLoading?: boolean;
  /** Eine Aenderung ist gerade unterwegs. */
  isSaving?: boolean;
  /** Die Abfrage ist gescheitert, der gespeicherte Stand ist unbekannt. */
  ladeFehler?: unknown;
}

/**
 * `true`, solange die Karte nicht bedient werden darf: waehrend des Ladens,
 * waehrend des Speicherns und dauerhaft, solange die Abfrage gescheitert ist.
 *
 * Der Ladefehler ist der wichtige Teil: er bleibt bestehen, `isLoading` faellt
 * danach zurueck auf `false`, und ohne diese Pruefung waere die Karte mit
 * erfundenen Werten wieder bedienbar.
 */
export function istGesperrt({ isLoading, isSaving, ladeFehler }: KartenZustand): boolean {
  return isLoading === true || isSaving === true || ladeFehler != null;
}

/** `true`, wenn die Karte den gespeicherten Stand gar nicht kennt. */
export function istStandUnbekannt(ladeFehler: unknown): boolean {
  return ladeFehler != null;
}

/**
 * Letzte Ausfuehrung einer Clip-Mutation: welcher Clip, welcher Fehler.
 *
 * Die Zuordnung stand vorher als Bedingungskette im JSX, und genau dort ist
 * das Verwerfen vergessen worden: ein fehlgeschlagenes Verwerfen (403 oder
 * 404) endete wortlos, der Nutzer hatte den Dialog bestaetigt, die Karte blieb
 * stehen. Die Kette hatte einen zweiten Fehler: sie prueft nur, ob eine
 * Mutation zuletzt diesen Clip betraf, nicht ob sie ueberhaupt gescheitert
 * ist. Eine erfolgreiche Freigabe verdeckte damit einen gescheiterten Abbruch
 * am selben Clip.
 */
export interface ClipMutationsStand {
  /** Clip der letzten Ausfuehrung, `undefined` wenn die Mutation nie lief. */
  clipDbId: number | undefined;
  error: unknown;
}

/** Der erste echte Fehler, der zu diesem Clip gehoert. */
export function clipFehler(clipDbId: number, staende: ClipMutationsStand[]): unknown {
  for (const stand of staende) {
    if (stand.clipDbId === clipDbId && stand.error != null) return stand.error;
  }
  return null;
}

/** Was in den drei Feldern einer Plattform steht, waehrend getippt wird. */
export interface ZeitplanFormular {
  postsProWoche: string;
  maxProTag: string;
  zeiten: string;
}

/** Die drei Felder einer Plattform, in fester Reihenfolge. */
export const ZEITPLAN_FELDER: (keyof ZeitplanFormular)[] = [
  'postsProWoche',
  'maxProTag',
  'zeiten',
];

/** Schluessel eines einzelnen Feldes, wie ihn die Zeitplan-Karte fuehrt. */
export function zeitplanFeldSchluessel(
  platform: string,
  feld: keyof ZeitplanFormular,
): string {
  return `${platform}:${feld}`;
}

/**
 * Gleicht das Zeitplan-Formular mit einer Serverantwort ab, ohne Eingaben zu
 * verlieren, die noch niemand abgeschickt hat.
 *
 * Warum es das gibt: jedes Feld schickt beim Verlassen eine eigene Mutation
 * los, und deren Antwort bringt einen neuen Serverstand mit. Wer zwei Felder
 * schnell hintereinander aendert, hatte die zweite Eingabe verloren, sobald
 * die Antwort auf die erste eintraf: der Abgleich hat das Formular komplett
 * ueberschrieben, und der neue Serverstand kennt die zweite Eingabe noch
 * nicht.
 *
 * `offeneFelder` sind die Felder, die der Nutzer geaendert und noch nicht
 * abgeschickt hat. Sie behalten ihren lokalen Wert, alle anderen uebernehmen
 * den Server. Damit gewinnt der Server weiterhin dort, wo er den Wert
 * normalisiert (Zeiten sortieren und entdoppeln), denn beim Abschicken gilt
 * ein Feld nicht mehr als offen.
 */
export function zeitplanFormularAbgleichen(
  aktuell: Record<string, ZeitplanFormular>,
  vomServer: Record<string, ZeitplanFormular>,
  offeneFelder: ReadonlySet<string>,
): Record<string, ZeitplanFormular> {
  const naechstes: Record<string, ZeitplanFormular> = {};
  for (const [platform, serverWerte] of Object.entries(vomServer)) {
    const lokal = aktuell[platform];
    if (!lokal) {
      naechstes[platform] = serverWerte;
      continue;
    }
    const zusammengefuehrt = { ...serverWerte };
    for (const feld of ZEITPLAN_FELDER) {
      if (offeneFelder.has(zeitplanFeldSchluessel(platform, feld))) {
        zusammengefuehrt[feld] = lokal[feld];
      }
    }
    naechstes[platform] = zusammengefuehrt;
  }
  return naechstes;
}

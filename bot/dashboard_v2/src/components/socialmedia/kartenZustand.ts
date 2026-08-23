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

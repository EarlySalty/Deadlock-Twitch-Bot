/* Auftritt von Kacheln und Abschnitten — Verzoegerungsrechnung.
 *
 * Das Dashboard staffelte seine Bloecke ueber framer-motion mit
 * `transition={{ delay: 0.05 … 0.65 }}`. Zwei Dinge stimmen daran nicht:
 * die Bewegung laeuft ueber den Hauptthread (framer-motions `y`-Prop ist
 * nicht hardwarebeschleunigt), und der letzte Block einer langen Seite
 * erscheint erst nach zwei Dritteln einer Sekunde. Eine Staffelung soll
 * lebendig wirken, nicht langsam.
 *
 * Quelle: skills/emil-design-eng (Stagger 30-80ms), skills/apple-design §11.
 */

/** Abstand zwischen zwei aufeinanderfolgenden Bloecken. */
export const RISE_STEP_MS = 40;

/** Ab hier laufen alle weiteren Bloecke gemeinsam an. */
export const RISE_MAX_DELAY_MS = 240;

/** Sekundenwert aus einem alten `transition={{ delay }}`-Prop. */
export type RiseSeconds = { seconds: number };

function clamp(ms: number): number {
  if (!Number.isFinite(ms) || ms <= 0) return 0;
  return Math.min(Math.round(ms), RISE_MAX_DELAY_MS);
}

/**
 * Verzoegerung eines Blocks in Millisekunden.
 *
 * Als Zahl uebergeben ist das der Index in der Reihe (0 = erster Block),
 * als `{ seconds }` der uebernommene Wert aus dem abgeloesten framer-motion-Prop.
 */
export function riseDelayMs(step: number | RiseSeconds): number {
  if (typeof step === 'number') return clamp(step * RISE_STEP_MS);
  return clamp(step.seconds * 1000);
}

/**
 * Style-Objekt fuer ein `.rise-in`-Element. Ohne Verzoegerung entfaellt das
 * Attribut, damit im Markup keine leeren `style=""` stehen.
 */
export function riseStyle(step: number | RiseSeconds): React.CSSProperties | undefined {
  const delay = riseDelayMs(step);
  if (delay === 0) return undefined;
  return { '--rise-delay': `${delay}ms` } as React.CSSProperties;
}

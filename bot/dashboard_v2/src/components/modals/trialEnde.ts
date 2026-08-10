/**
 * Entscheidung, ob das Trial-Ende-Fenster aufgeht.
 *
 * Steht ausserhalb der Komponente, weil sie sonst nur im Browser pruefbar
 * waere — und genau diese Entscheidung war vorher kaputt: sie haing an einem
 * Feld, das die API nie geliefert hat.
 */
export function sollTrialEndeZeigen(zustand: {
  /** Ablaufdatum eines abgelaufenen Trials, sonst null. */
  trialEndedAt: string | null;
  hasFullAccess: boolean;
  tier: string;
  /** Fenster fuer genau dieses Ablaufdatum schon weggeklickt. */
  gesehen: boolean;
}): boolean {
  if (!zustand.trialEndedAt) return false;
  if (zustand.hasFullAccess || zustand.tier === 'extended') return false;
  return !zustand.gesehen;
}

export const TRIAL_ENDE_GESEHEN_PRAEFIX = 'trial-ende-gesehen:';

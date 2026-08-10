/** Endpreis-Formatierung fuer die Premium-Flaeche. Kein Steuerausweis (§ 19 UStG). */
export function euroAusCents(cents: number): string {
  return `${(cents / 100).toLocaleString('de-DE', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })} €`;
}

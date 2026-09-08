/** Vom Dienst gelieferte Profilhinweise; null bedeutet noch nicht geprüft. */
export interface UplinkCaps {
  platform: string;
  recommended_width: number | null;
  recommended_height: number | null;
  recommended_fps: number | null;
  recommended_bitrate_kbps: number | null;
  force_cbr: boolean;
}

/** Alte max-Felder werden erkannt, aber niemals als Empfehlung ausgegeben. */
export interface UplinkCapsRoh extends Partial<UplinkCaps> {
  max_width?: number | null;
  max_height?: number | null;
  max_fps?: number | null;
  max_bitrate_kbps?: number | null;
}

/** Nur echte, positive Zahlen sind eine Empfehlung. Alles andere ist `null`. */
function zahlOderNull(wert: number | null | undefined): number | null {
  return typeof wert === 'number' && Number.isFinite(wert) && wert > 0 ? wert : null;
}

export function normalisiereCaps(roh: UplinkCapsRoh): UplinkCaps {
  return {
    platform: roh.platform ?? '',
    recommended_width: zahlOderNull(roh.recommended_width),
    recommended_height: zahlOderNull(roh.recommended_height),
    recommended_fps: zahlOderNull(roh.recommended_fps),
    recommended_bitrate_kbps: zahlOderNull(roh.recommended_bitrate_kbps),
    force_cbr: roh.force_cbr === true,
  };
}

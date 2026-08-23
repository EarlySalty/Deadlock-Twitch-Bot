import type { UplinkDestination } from './api/uplink';

/**
 * Was eine Plattform an Video empfiehlt. Ausdruecklich eine Empfehlung und
 * keine Grenze: das Relay rechnet nichts mehr herunter und lehnt nichts mehr
 * ab, was der Streamer einstellt, geht genau so raus. `null` heisst: fuer
 * diesen Wert gibt die Plattform nichts vor.
 */
export interface UplinkCaps {
  platform: string;
  recommended_width: number | null;
  recommended_height: number | null;
  recommended_fps: number | null;
  recommended_bitrate_kbps: number | null;
  force_cbr: boolean;
}

/**
 * Die Serverantwort, so wie sie ankommen kann.
 *
 * Die Vorgaengerfassung des Endpunkts hiess `max_*` und lieferte zusaetzlich
 * einen `ingest`-Eintrag. Beides ist weg, aber Dashboard und Relay werden nicht
 * in derselben Sekunde ausgerollt. Ohne diese Umschrift stuenden im Zeitfenster
 * dazwischen gar keine Empfehlungen an den Feldern, und das saehe aus wie ein
 * Fehler statt wie ein Deploy.
 */
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
    recommended_width: zahlOderNull(roh.recommended_width ?? roh.max_width),
    recommended_height: zahlOderNull(roh.recommended_height ?? roh.max_height),
    recommended_fps: zahlOderNull(roh.recommended_fps ?? roh.max_fps),
    recommended_bitrate_kbps: zahlOderNull(roh.recommended_bitrate_kbps ?? roh.max_bitrate_kbps),
    force_cbr: roh.force_cbr === true,
  };
}

/**
 * Was in OBS als Bitrate eingetragen werden soll.
 *
 * Frueher stand dort eine feste Spanne von 15000 bis 25000 kbps. Die Zahl war
 * doppelt falsch: kaum ein deutscher Heimanschluss traegt 25 Mbit Upload, und
 * genau deshalb gibt es Uplink ueberhaupt. Statt einer Spanne aus dem Nichts
 * rechnen wir aus dem, was der Streamer als Ziele eingestellt hat.
 *
 * Eigenes Modul ohne Laufzeit-Abhaengigkeit auf `api/uplink`: die Rechnung soll
 * im nackten Node-Testlauf pruefbar sein, ohne dass der Fetch-Unterbau der
 * Oberflaeche mitgeladen wird.
 */

/** Reserve auf die staerkste Zielbitrate. Der Encoder braucht Luft nach oben. */
const RESERVE = 1.2;

/** Auf volle 500 kbps runden: OBS-Bitraten sind ueberall solche Zahlen. */
const SCHRITT = 500;

/** Die Bitrate der Standardstufe (1080p60, 6000 kbps) aus dem Stufenkatalog. */
const STANDARD_ZIEL_KBPS = 6000;

function aufSchritt(kbps: number): number {
  return Math.ceil(kbps / SCHRITT) * SCHRITT;
}

/**
 * Der Startwert, solange noch kein Ziel eingestellt ist. Keine ausgedachte
 * Zahl, sondern dieselbe Rechnung auf die Standardstufe: 6000 plus Reserve.
 */
export const OBS_BITRATE_START = aufSchritt(STANDARD_ZIEL_KBPS * RESERVE);

export interface ObsBitrateEmpfehlung {
  /** Die eine Zahl, die in OBS eingetragen wird. */
  kbps: number;
  /**
   * Die staerkste Zielbitrate, aus der die Empfehlung stammt. `null` heisst:
   * es gibt noch kein Ziel, die Empfehlung ist der Startwert.
   */
  staerkstesZiel: number | null;
}

/**
 * Die hoechste Bitrate ueber alle eingeschalteten Ziele, plus Reserve.
 *
 * Sind alle Ziele pausiert, zaehlen die pausierten mit: der Startwert waere
 * dann eine Zahl aus dem Nichts, obwohl die eingestellten Werte direkt
 * danebenstehen.
 */
export function obsBitrateEmpfehlung(
  ziele: UplinkDestination[] | undefined,
): ObsBitrateEmpfehlung {
  const brauchbar = (ziele ?? [])
    .map((ziel) => ({ enabled: ziel.enabled, kbps: ziel.requested?.bitrate_kbps }))
    .filter((ziel): ziel is { enabled: boolean; kbps: number } =>
      typeof ziel.kbps === 'number' && Number.isFinite(ziel.kbps) && ziel.kbps > 0,
    );
  const eingeschaltet = brauchbar.filter((ziel) => ziel.enabled);
  const quelle = eingeschaltet.length > 0 ? eingeschaltet : brauchbar;
  if (quelle.length === 0) {
    return { kbps: OBS_BITRATE_START, staerkstesZiel: null };
  }
  const staerkstesZiel = Math.max(...quelle.map((ziel) => ziel.kbps));
  return { kbps: aufSchritt(staerkstesZiel * RESERVE), staerkstesZiel };
}

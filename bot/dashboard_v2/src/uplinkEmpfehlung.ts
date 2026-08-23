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
 * Zwei Fassungen davor waren falsch, und beide auf dieselbe Art: sie haben
 * eine Zahl genannt, die kein gewoehnlicher deutscher Heimanschluss traegt.
 * Erst stand hier eine feste Spanne von 15000 bis 25000 kbps, danach die
 * staerkste Zielbitrate mal 1,2, was bei einem YouTube-Ziel auf 22000
 * hinauslief. Genau wegen knapper Upload-Leitungen gibt es Uplink ueberhaupt:
 * einmal hochladen statt viermal.
 *
 * Die Rechnung ist deshalb keine Rechnung mehr, sondern ein Nachschlagen in
 * der Hilfeseite `public/uplink/obs.html`, die auf derselben Dashboard-Seite
 * eingebettet ist. Sie staffelt nach gemessenem Upload und nennt zwei Paare
 * aus Zielbitrate und Maximalbitrate, siehe [`OBS_STUFE_STANDARD`] und
 * [`OBS_STUFE_2K`]. Ein Test prueft, dass beide Paare woertlich in der
 * Hilfeseite stehen: zwei Empfehlungen auf einer Seite, die sich
 * widersprechen, waren der eigentliche Befund.
 *
 * Warum nicht aus der Zielbitrate hochrechnen: zu uns geht HEVC, an die
 * Plattformen geht H.264. HEVC packt dasselbe Bild in deutlich weniger Bits,
 * eine Ingest-Bitrate ueber der Zielbitrate ist damit in der Regel verkehrt
 * herum. Was der Streamer zu uns hochlaedt, haengt an seiner Aufloesung und
 * an seinem Upload, nicht an der Zahl, die spaeter zu Twitch rausgeht.
 *
 * Der Upload ist der harte Deckel, und wir kennen ihn nicht. Die Oberflaeche
 * fragt ihn bewusst nicht ab: eine Zahl, die der Streamer selbst schaetzt,
 * saehe im Feld genauso verbindlich aus wie eine gemessene, und die Messung
 * gehoert ohnehin in die Hilfeseite. Stattdessen sagt der Text dazu, dass die
 * Zahl unter dem gemessenen Upload bleiben muss.
 *
 * Eigenes Modul ohne Laufzeit-Abhaengigkeit auf `api/uplink`: die Auswahl soll
 * im nackten Node-Testlauf pruefbar sein, ohne dass der Fetch-Unterbau der
 * Oberflaeche mitgeladen wird.
 */

export interface ObsBitrateStufe {
  /** Zielbitrate fuer VBR. */
  kbps: number;
  /** Maximalbitrate fuer VBR. */
  maxKbps: number;
}

/**
 * Fuer alles, was bei uns hoechstens 1080p verlaesst.
 *
 * Woertlich aus `obs.html`, Zeile "5 bis 8 Mbit": 1920x1080, 60 fps,
 * "HEVC, VBR 6000 / max 8000". Dieselben Zahlen stehen dort auch als
 * "Standard bei knapper Leitung".
 */
export const OBS_STUFE_STANDARD: ObsBitrateStufe = { kbps: 6000, maxKbps: 8000 };

/**
 * Nur fuer den, der 2K auch wirklich weitersendet.
 *
 * Woertlich aus `obs.html`, Zeile "ab 14 Mbit, wenn du 2K auch rausschicken
 * willst": 2560x1440, 60 fps, "HEVC, VBR 9000 / max 12000".
 *
 * Wer 1440p schickt, damit wir daraus ein schaerferes 1080p rechnen, bleibt
 * bei [`OBS_STUFE_STANDARD`]. Auch das steht so in der Hilfeseite, Zeile
 * "ab 8 Mbit, GPU haelt 1440".
 */
export const OBS_STUFE_2K: ObsBitrateStufe = { kbps: 9000, maxKbps: 12000 };

/** Ab dieser Zielhoehe geht 2K raus und die groessere Stufe gilt. */
const HOEHE_1080 = 1080;

/**
 * Um wie viel die AMD-Encoder im VBR-Modus ueber die eingetragene Bitrate
 * hinausgehen.
 *
 * Bei "AMD HW H.264/H.265/AV1" gibt es kein Feld fuer die Maximalbitrate. OBS
 * setzt sie selbst, und zwar auf das Anderthalbfache der Zielbitrate; im
 * Quelltext steht das als `set_hevc_property(enc, PEAK_BITRATE, bitrate * 1.5)`
 * in `texture-amf.cpp`. Wer 16000 eintraegt, sendet also in Spitzen 24000.
 *
 * Das ist kein Randfall, den man in einem Nebensatz erwaehnt: die Empfehlung
 * neben dem Feld nennt eine Zielbitrate und eine Maximalbitrate, und bei einer
 * AMD-Karte ist die zweite Zahl nicht einstellbar. Wer sie fuer eine Grenze
 * haelt, plant seine Leitung um ein Drittel zu knapp.
 */
export const AMD_VBR_SPITZENFAKTOR = 1.5;

/**
 * Anteil der Leitung, den ein Stream hoechstens belegen soll.
 *
 * Der Rest ist fuer alles andere, was gleichzeitig hochlaedt, und fuer die
 * Schwankung der Leitung selbst. Eine Leitung, die zu 100 Prozent belegt ist,
 * ist eine Leitung, die aussetzt.
 */
export const UPLOAD_RESERVE = 0.8;

/**
 * Die Spitze, die eine AMD-Karte bei dieser Stufe wirklich sendet.
 *
 * Nicht `stufe.maxKbps`: das ist die Zahl fuer die Encoder, bei denen man sie
 * eintragen kann.
 */
export function amdSpitzeKbps(stufe: ObsBitrateStufe): number {
  return Math.round(stufe.kbps * AMD_VBR_SPITZENFAKTOR);
}

/**
 * Welchen gemessenen Upload diese Stufe braucht, in Mbit, aufgerundet.
 *
 * `amd` schaltet auf die Spitze um, die OBS dort selbst setzt. Ohne das stuende
 * bei einer AMD-Karte eine Zahl da, die der Streamer gar nicht erreicht.
 */
export function noetigerUploadMbit(stufe: ObsBitrateStufe, amd = false): number {
  const spitze = amd ? amdSpitzeKbps(stufe) : stufe.maxKbps;
  return Math.ceil(spitze / UPLOAD_RESERVE / 1000);
}

/**
 * Die Zielbitrate, die auf eine gemessene Leitung passt, in kbps.
 *
 * Der Weg andersherum, und der, den ein Streamer mit knapper Leitung braucht:
 * er kennt seinen Upload und sucht die Zahl fuer das Feld. Auf 100 kbps
 * gerundet, weil eine Empfehlung auf 33 kbps genau eine Genauigkeit vortaeuscht,
 * die eine Leitungsmessung nicht hergibt.
 */
export function zielbitrateFuerUploadKbps(uploadKbps: number, amd = false): number {
  const nutzbar = uploadKbps * UPLOAD_RESERVE;
  const ziel = amd ? nutzbar / AMD_VBR_SPITZENFAKTOR : nutzbar;
  return Math.floor(ziel / 100) * 100;
}

/**
 * Woher die Empfehlung kommt. Der Unterschied zwischen `start` und
 * `unbekannt` ist der Befund aus dem Review: ein fehlgeschlagener Abruf sah
 * aus wie "noch kein Ziel eingerichtet", und der Text hat einem Streamer mit
 * vier eingerichteten Zielen erzaehlt, er habe keins.
 */
export type ObsBitrateHerkunft = 'ziele' | 'start' | 'unbekannt';

export interface ObsBitrateEmpfehlung extends ObsBitrateStufe {
  herkunft: ObsBitrateHerkunft;
  /**
   * Die hoechste Bildhoehe ueber alle mitgezaehlten Ziele, aus der die Stufe
   * stammt. `null`, wenn es keine Ziele gibt oder sie nicht abrufbar waren.
   */
  hoehe: number | null;
}

/**
 * Die Stufe fuer die eingestellten Ziele.
 *
 * `ladefehler` ist der dritte Zustand neben "keine Ziele" und "Ziele da": der
 * Abruf laeuft ohne Wiederholung, und ohne dieses Flag waere ein Ausfall von
 * einem leeren Konto nicht zu unterscheiden.
 *
 * Sind alle Ziele pausiert, zaehlen die pausierten mit: der Startwert waere
 * dann eine Zahl aus dem Nichts, obwohl die eingestellten Werte direkt
 * danebenstehen.
 */
export function obsBitrateEmpfehlung(
  ziele: UplinkDestination[] | undefined,
  ladefehler = false,
): ObsBitrateEmpfehlung {
  if (ladefehler) {
    return { ...OBS_STUFE_STANDARD, herkunft: 'unbekannt', hoehe: null };
  }
  const brauchbar = (ziele ?? [])
    .map((ziel) => ({ enabled: ziel.enabled, hoehe: ziel.requested?.height }))
    .filter((ziel): ziel is { enabled: boolean; hoehe: number } =>
      typeof ziel.hoehe === 'number' && Number.isFinite(ziel.hoehe) && ziel.hoehe > 0,
    );
  const eingeschaltet = brauchbar.filter((ziel) => ziel.enabled);
  const quelle = eingeschaltet.length > 0 ? eingeschaltet : brauchbar;
  if (quelle.length === 0) {
    return { ...OBS_STUFE_STANDARD, herkunft: 'start', hoehe: null };
  }
  const hoehe = Math.max(...quelle.map((ziel) => ziel.hoehe));
  const stufe = hoehe > HOEHE_1080 ? OBS_STUFE_2K : OBS_STUFE_STANDARD;
  return { ...stufe, herkunft: 'ziele', hoehe };
}

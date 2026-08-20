// Reine Logik und feste Texte der Uplink-Seite.
//
// Alles hier ist ohne React testbar: die Sichtbarkeit des Tabs, die Klemmung
// der Profilwerte gegen die Antwort des Servers und die Plattform-Liste.
// Die woertlichen Texte stehen als Konstanten, damit Seite und Test dieselbe
// Quelle benutzen.

import { ApiHttpError } from '../api/httpError';

export const UPLINK_TAB_ID = 'restream' as const;
export const UPLINK_TAB_LABEL = 'Uplink';

/** Warteliste-Karte, Abschnitt 13 Text 6 der Spezifikation. */
export const UPLINK_WAITLIST_TEXT =
  'Streame über unseren Server auf Twitch, Kick, YouTube und TikTok gleichzeitig, auch mit schwachem Upload. Trag dich auf die Warteliste ein, die Plätze sind begrenzt.';

/** Wenn der Eintrag auf die Warteliste beim Server hängen bleibt. */
export const UPLINK_WAITLIST_FEHLER =
  'Der Eintrag auf die Warteliste hat gerade nicht geklappt. Versuch es bitte gleich noch einmal.';

/** Hinweis, wenn Twitch den Schlüssel nicht herausgibt. */
export const UPLINK_TWITCH_SCOPE_HINT =
  'Twitch gibt den Schlüssel für dieses Konto noch nicht heraus. Verbinde deinen Twitch-Zugang neu, dann holen wir ihn automatisch. Bis dahin kannst du den Schlüssel unten selbst eintragen.';

/** Wenn das Holen aus einem anderen Grund als der Freigabe scheitert. */
export const UPLINK_TWITCH_ALLGEMEINER_FEHLER =
  'Twitch antwortet uns gerade nicht. Versuch es später noch einmal oder trag den Schlüssel unten selbst ein.';

/** Link in den bestehenden OAuth-Flow, erweiterter Scope-Satz. */
export const UPLINK_REAUTH_HREF = '/twitch/raid/auth?scope_profile=dashboard_reauth';

/**
 * Welcher Satz zu einem gescheiterten Twitch-Abruf gehört.
 *
 * Nur wenn Twitch die Freigabe verweigert, hilft ein neues Verbinden. Bei
 * allem anderen wäre dieser Rat falsch und der Streamer klickt umsonst.
 */
export function twitchFehlertext(error: unknown): string {
  if (error instanceof ApiHttpError && (error.status === 401 || error.status === 403)) {
    return UPLINK_TWITCH_SCOPE_HINT;
  }
  return UPLINK_TWITCH_ALLGEMEINER_FEHLER;
}

/**
 * Ob der Uplink-Tab in der Hauptnavigation auftaucht.
 *
 * `public_visible` ist die Freigabe für alle. Solange sie aus ist, sieht nur
 * der Admin-Modus den Tab. Fehlt das Feld in der Antwort, gilt es als aus:
 * eine fehlende Angabe darf nichts aufsperren.
 */
export function isUplinkTabVisible(opts: {
  publicVisible?: boolean | null;
  isAdmin?: boolean | null;
}): boolean {
  if (opts.publicVisible === true) return true;
  return opts.isAdmin === true;
}

/**
 * Nummer des laufenden Streams, sonst nichts.
 *
 * Der Uplink schickt die laufende Session als Objekt unter `session`. Fehlt
 * das Objekt, laeuft nichts, und die Statuskarte darf keine Zahlen anfragen.
 */
export function aktiveSessionId(
  me?: { session?: { id?: number | null } | null } | null
): number | null {
  const id = Number(me?.session?.id);
  return Number.isFinite(id) && id > 0 ? id : null;
}

export interface UplinkProfile {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

export interface ClampedField {
  label: string;
  requested: number;
  effective: number;
}

const PROFILE_LABELS: Array<[keyof UplinkProfile, string]> = [
  ['width', 'Breite'],
  ['height', 'Höhe'],
  ['fps', 'Bilder pro Sekunde'],
  ['bitrate_kbps', 'Datenrate'],
];

/**
 * Welche Werte der Server nach unten gezogen hat.
 *
 * Der Server klemmt gegen die Grenzen der Plattform und antwortet mit dem
 * effektiven Profil. Ohne diesen Vergleich stünde im Feld weiter der Wunsch,
 * und gesendet würde etwas anderes.
 */
export function clampedFields(
  requested: UplinkProfile | null | undefined,
  effective: UplinkProfile | null | undefined
): ClampedField[] {
  if (!requested || !effective) return [];
  const treffer: ClampedField[] = [];
  for (const [feld, label] of PROFILE_LABELS) {
    const soll = Number(requested[feld]);
    const ist = Number(effective[feld]);
    if (Number.isFinite(soll) && Number.isFinite(ist) && ist < soll) {
      treffer.push({ label, requested: soll, effective: ist });
    }
  }
  return treffer;
}

export interface UplinkPlatform {
  id: string;
  label: string;
  /** Vorbelegte Push-Adresse. Leer heisst: die gibt die Plattform je Konto aus. */
  defaultRtmpUrl: string;
  /** Wo der Streamer den Schlüssel findet. */
  hint: string;
}

export const UPLINK_PLATFORMS: UplinkPlatform[] = [
  {
    id: 'twitch',
    label: 'Twitch',
    defaultRtmpUrl: 'rtmp://live.twitch.tv/app',
    hint: 'Holen wir automatisch, sobald dein Twitch-Zugang den Schlüssel freigibt.',
  },
  {
    id: 'kick',
    label: 'Kick',
    defaultRtmpUrl: '',
    hint: 'Kick gibt dir Adresse und Schlüssel im Creator-Dashboard unter Stream.',
  },
  {
    id: 'youtube',
    label: 'YouTube',
    defaultRtmpUrl: 'rtmp://a.rtmp.youtube.com/live2',
    hint: 'YouTube Studio, Live-Streaming, Stream-Schlüssel.',
  },
  {
    id: 'tiktok',
    label: 'TikTok',
    defaultRtmpUrl: '',
    hint: 'TikTok Live Studio zeigt dir Adresse und Schlüssel beim Anlegen des Streams.',
  },
];

/**
 * Ob sich der Knopf abschicken lässt.
 *
 * Halb ausgefüllt lehnt der Server ab, deshalb gehen Adresse und Schlüssel
 * nur zusammen weg. Steht die Adresse nach dem automatischen Verbinden schon
 * im Feld und liegt der Schlüssel beim Server, bleibt trotzdem ein reines
 * Profil-Update möglich: dann fahren wir Adresse und Schlüssel gar nicht mit.
 */
export function canSaveDestination(opts: {
  rtmpUrl: string;
  streamKey: string;
  profileTouched: boolean;
  verbunden: boolean;
}): boolean {
  const url = opts.rtmpUrl.trim();
  const key = opts.streamKey.trim();
  if (url && key) return true;
  if (!url && !key) return opts.profileTouched;
  if (url && !key) return opts.verbunden && opts.profileTouched;
  return false;
}

export interface ZielEingabeWerte {
  platform: string;
  rtmpUrl: string;
  streamKey: string;
  width: string;
  height: string;
  fps: string;
  bitrate: string;
}

/** Positive ganze Zahl aus einem Eingabefeld, sonst nichts. */
function zahlAusFeld(wert: string): number | undefined {
  const getrimmt = wert.trim();
  if (!getrimmt) return undefined;
  const zahl = Number(getrimmt);
  return Number.isFinite(zahl) && zahl > 0 ? Math.round(zahl) : undefined;
}

/**
 * Baut den Rumpf für ein Ziel.
 *
 * Adresse und Schlüssel stehen nur zusammen drin. Ein halb gefülltes Paar
 * lehnt der Server ab, und ein reines Profil-Update auf ein gespeichertes Ziel
 * braucht beides gar nicht.
 */
export function zielRumpf(werte: ZielEingabeWerte): {
  platform: string;
  rtmp_url?: string;
  stream_key?: string;
  width?: number;
  height?: number;
  fps?: number;
  bitrate_kbps?: number;
} {
  const url = werte.rtmpUrl.trim();
  const key = werte.streamKey.trim();
  const rumpf: {
    platform: string;
    rtmp_url?: string;
    stream_key?: string;
    width?: number;
    height?: number;
    fps?: number;
    bitrate_kbps?: number;
  } = { platform: werte.platform };
  if (url && key) {
    rumpf.rtmp_url = url;
    rumpf.stream_key = key;
  }
  const breite = zahlAusFeld(werte.width);
  const hoehe = zahlAusFeld(werte.height);
  const bilder = zahlAusFeld(werte.fps);
  const datenrate = zahlAusFeld(werte.bitrate);
  if (breite !== undefined) rumpf.width = breite;
  if (hoehe !== undefined) rumpf.height = hoehe;
  if (bilder !== undefined) rumpf.fps = bilder;
  if (datenrate !== undefined) rumpf.bitrate_kbps = datenrate;
  return rumpf;
}

/**
 * Die gespeicherten Grenzen als Formularwerte.
 *
 * Der Server darf die Werte anpassen. Ohne diesen Rückweg stünde im Feld
 * weiter die Eingabe, und der Admin hielte sie für gespeichert.
 */
export function formularAusEinstellungen(
  antwort: { max_points?: number | null; load_reject_threshold?: number | null } | null | undefined
): { plaetze: string; lastgrenze: string } {
  const alsText = (wert: number | null | undefined): string =>
    typeof wert === 'number' && Number.isFinite(wert) ? String(wert) : '';
  return {
    plaetze: alsText(antwort?.max_points),
    lastgrenze: alsText(antwort?.load_reject_threshold),
  };
}

/** Der Stream steht noch, obwohl das Beenden schon raus ist. */
export const UPLINK_KILL_LAEUFT_NOCH =
  'Der Stream läuft noch. Wir haben das Beenden angesagt und warten auf die Bestätigung.';

/**
 * Ob der Stream wirklich steht.
 *
 * `ended` heißt nur, dass der Server es vermerkt hat. Erst `stopped` sagt,
 * dass nichts mehr sendet. Ohne diese Trennung meldet das Dashboard Erfolg,
 * während der Stream weiterläuft.
 */
export function killErfolgreich(
  antwort: { stopped?: boolean | null } | null | undefined
): boolean {
  return antwort?.stopped === true;
}

/** Der Server sendet langsamer, als das Bild ankommt. */
export const UPLINK_SPEED_HINTERHER =
  'Unser Server kommt gerade nicht ganz mit. Wenn das so bleibt, kann dein Bild bei den Zuschauern stocken.';

/** Der Server hält Schritt. */
export const UPLINK_SPEED_MITHALTEN = 'Unser Server hält mühelos Schritt.';

/**
 * Lage des Servers in einem Satz, ohne Zahl und ohne Fachwort.
 *
 * Ohne brauchbaren Messwert kommt nichts zurück: eine erfundene Entwarnung
 * wäre schlimmer als eine leere Zeile.
 */
export function speedLage(encoderSpeed: number | null | undefined): string | null {
  if (encoderSpeed === null || encoderSpeed === undefined) return null;
  const wert = Number(encoderSpeed);
  if (!Number.isFinite(wert) || wert < 0) return null;
  return wert < 1 ? UPLINK_SPEED_HINTERHER : UPLINK_SPEED_MITHALTEN;
}

/** Auslastung als lesbarer Prozentwert, sonst nichts. */
export function lastProzent(cpuPct: number | null | undefined): string | null {
  if (cpuPct === null || cpuPct === undefined) return null;
  const wert = Number(cpuPct);
  if (!Number.isFinite(wert) || wert < 0) return null;
  return `${Math.round(wert)} %`;
}

export interface EgressZeile {
  ziel: string;
  kbps: number;
}

/**
 * Ausgehende Datenrate je Plattform, mit dem Namen, den der Streamer kennt.
 *
 * Fehlende Messwerte fliegen raus statt als Null dazustehen.
 */
export function egressJeZiel(
  rohwert: Record<string, unknown> | null | undefined
): EgressZeile[] {
  if (!rohwert || typeof rohwert !== 'object') return [];
  const zeilen: EgressZeile[] = [];
  for (const [schluessel, wert] of Object.entries(rohwert)) {
    if (typeof wert !== 'number' || !Number.isFinite(wert)) continue;
    const plattform = UPLINK_PLATFORMS.find((p) => p.id === schluessel);
    zeilen.push({ ziel: plattform ? plattform.label : schluessel, kbps: wert });
  }
  return zeilen;
}

/**
 * Macht aus der Eingabe eines `datetime-local`-Feldes die Form, die der
 * Uplink erwartet: `YYYY-MM-DDTHH:MM:SSZ` in UTC.
 *
 * Ohne die Umrechnung schickt der Browser Ortszeit ohne Zone, und der Server
 * legte ein Fenster an, das um den Zeitzonenversatz verschoben ist.
 */
export function toRelayZeit(lokal: string): string | null {
  if (!lokal) return null;
  const wert = new Date(lokal);
  const ms = wert.getTime();
  if (!Number.isFinite(ms)) return null;
  return wert.toISOString().replace(/\.\d+Z$/, 'Z');
}

/** Umkehrung von [`toRelayZeit`] für die Anzeige im Eingabefeld. */
export function toEingabeZeit(iso: string): string {
  const wert = new Date(iso);
  const ms = wert.getTime();
  if (!Number.isFinite(ms)) return '';
  const zweistellig = (n: number) => String(n).padStart(2, '0');
  return (
    `${wert.getFullYear()}-${zweistellig(wert.getMonth() + 1)}-${zweistellig(wert.getDate())}` +
    `T${zweistellig(wert.getHours())}:${zweistellig(wert.getMinutes())}`
  );
}

/**
 * Zeitspanne zwischen zwei Zeitpunkten als lesbare Dauer.
 *
 * `now` ist ein Parameter, damit die Funktion ohne Wanduhr prüfbar bleibt.
 */
export function formatDauer(
  startIso: string,
  endIso: string | null,
  now: number = Date.now()
): string {
  const start = Date.parse(startIso);
  const ende = endIso ? Date.parse(endIso) : now;
  if (!Number.isFinite(start) || !Number.isFinite(ende) || ende < start) return '';
  const minuten = Math.floor((ende - start) / 60000);
  const stunden = Math.floor(minuten / 60);
  const rest = minuten % 60;
  return stunden > 0 ? `${stunden} h ${rest} min` : `${rest} min`;
}

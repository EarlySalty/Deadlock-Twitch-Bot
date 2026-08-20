// Reine Logik und feste Texte der Uplink-Seite.
//
// Alles hier ist ohne React testbar: die Sichtbarkeit des Tabs, die Klemmung
// der Profilwerte gegen die Antwort des Servers und die Plattform-Liste.
// Die woertlichen Texte stehen als Konstanten, damit Seite und Test dieselbe
// Quelle benutzen.

export const UPLINK_TAB_ID = 'restream' as const;
export const UPLINK_TAB_LABEL = 'Uplink';

/** Warteliste-Karte, Abschnitt 13 Text 6 der Spezifikation. */
export const UPLINK_WAITLIST_TEXT =
  'Streame über unseren Server auf Twitch, Kick, YouTube und TikTok gleichzeitig, auch mit schwachem Upload. Trag dich auf die Warteliste ein, die Plätze sind begrenzt.';

/** Hinweis, wenn Twitch den Schlüssel nicht herausgibt. */
export const UPLINK_TWITCH_SCOPE_HINT =
  'Twitch gibt den Schlüssel für dieses Konto noch nicht heraus. Verbinde deinen Twitch-Zugang neu, dann holen wir ihn automatisch. Bis dahin kannst du den Schlüssel unten selbst eintragen.';

/** Link in den bestehenden OAuth-Flow, erweiterter Scope-Satz. */
export const UPLINK_REAUTH_HREF = '/twitch/raid/auth?scope_profile=dashboard_reauth';

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
 * Ob Adresse und Schlüssel zusammen abgeschickt werden können.
 *
 * Halb ausgefüllt lehnt der Server ab. Der Knopf bleibt deshalb aus, bis
 * beides steht oder beides leer ist und nur die Profilwerte geändert wurden.
 */
export function canSaveDestination(opts: {
  rtmpUrl: string;
  streamKey: string;
  profileTouched: boolean;
}): boolean {
  const url = opts.rtmpUrl.trim();
  const key = opts.streamKey.trim();
  if (url && key) return true;
  if (!url && !key) return opts.profileTouched;
  return false;
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

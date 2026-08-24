// Reine Logik und feste Texte der Uplink-Seite.
//
// Alles hier ist ohne React testbar: die Sichtbarkeit des Tabs, die Klemmung
// der Profilwerte gegen die Antwort des Servers und die Plattform-Liste.
// Die woertlichen Texte stehen als Konstanten, damit Seite und Test dieselbe
// Quelle benutzen.

import { ApiHttpError } from '../api/httpError';

export const UPLINK_TAB_ID = 'restream' as const;
export const UPLINK_TAB_LABEL = 'Uplink';
export const UPLINK_TWITCH_LOGIN_HINT =
  'Für die persönliche Ansicht ist ein Twitch-Login nötig.';

/** Warteliste-Karte, Abschnitt 13 Text 6 der Spezifikation. */
export const UPLINK_WAITLIST_TEXT =
  'Streame über unseren Server auf Twitch, Kick, YouTube und TikTok gleichzeitig, auch mit schwachem Upload. Trag dich auf die Warteliste ein, die Plätze sind begrenzt.';

/** Wenn der Eintrag auf die Warteliste beim Server hängen bleibt. */
export const UPLINK_WAITLIST_FEHLER =
  'Der Eintrag auf die Warteliste hat gerade nicht geklappt. Versuch es bitte gleich noch einmal.';

/** Der Zeitplan verteilt Erwartungen, entscheidet aber nicht über den Start. */
export const UPLINK_SCHEDULE_TEXT =
  'Trag deine geplanten Zeiten ein. Der Zeitplan hilft dir, Erwartungen zu verteilen und Konflikte sichtbar zu machen. Ob dein Stream starten kann, hängt von der aktuellen Auslastung ab.';

/** Der Wert gilt nur fuer einen unerwarteten Abriss, nicht fuer OBS-Stop. */
export const UPLINK_RECONNECT_WAIT_TEXT =
  'Diese Zeit gilt nur nach einem unerwarteten Internetabriss. Wenn du den Stream in OBS beendest, räumt Uplink sofort auf.';

/** Macht einen Serverwert sicher zu einem kontrollierten Eingabewert. */
export function reconnectWaitEingabe(wert: number | null | undefined): string {
  return typeof wert === 'number' && Number.isFinite(wert) && wert >= 0 ? String(wert) : '';
}

/**
 * Liest Sekunden als ganze Zahl, ohne die Server-Obergrenze zu duplizieren.
 * Das Relay klemmt den Wert und liefert danach den tatsächlich gesetzten Wert.
 */
export function reconnectWaitPayload(wert: string): number | null {
  const getrimmt = wert.trim();
  if (!getrimmt || !/^\d+$/.test(getrimmt)) return null;
  const sekunden = Number(getrimmt);
  return Number.isSafeInteger(sekunden) && sekunden >= 0 ? sekunden : null;
}

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

export type UplinkAnsicht = 'streamer' | 'admin-mit-twitch' | 'admin-ohne-twitch';

/** Trennt die Verwaltungsansicht von der persönlichen Streamer-Abfrage. */
export function uplinkAnsicht(opts: {
  isAdmin?: boolean | null;
  twitchLogin?: string | null;
}): UplinkAnsicht {
  if (opts.isAdmin !== true) return 'streamer';
  return opts.twitchLogin?.trim() ? 'admin-mit-twitch' : 'admin-ohne-twitch';
}

export function uplinkAdminBloeckeSichtbar(ansicht: UplinkAnsicht): boolean {
  return ansicht !== 'streamer';
}

export function uplinkStreamerBloeckeSichtbar(ansicht: UplinkAnsicht): boolean {
  return ansicht !== 'admin-ohne-twitch';
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

/** `/me` muss einen Wechsel der laufenden Session ohne Seitenreload sehen. */
export const UPLINK_ME_REFETCH_INTERVAL_MS = 15_000;

/** React-Query-Schlüssel, damit eine neue Session neue Messwerte anfragt. */
export function uplinkMetricsQueryKey(
  sessionId: number | null | undefined,
): readonly ['uplink-metrics', number | null] {
  return ['uplink-metrics', sessionId ?? null];
}

export interface UplinkScheduleDraftRow {
  von: string;
  bis: string;
}

export interface UplinkScheduleSavePlan {
  entries: Array<{ starts_at: string; ends_at: string }> | null;
  error: string | null;
}

/**
 * Prüft den sichtbaren Zeitplan vor dem PUT.
 *
 * `null` bedeutet: Es darf kein PUT mit einer Ersatzliste abgeschickt werden.
 * So wird ein nicht geladener oder unlesbarer Bestand niemals als Löschung
 * interpretiert.
 */
export function scheduleSavePlan(
  rows: UplinkScheduleDraftRow[],
  status: { loaded: boolean; failed: boolean },
): UplinkScheduleSavePlan {
  if (status.failed) {
    return { entries: null, error: 'Der Zeitplan konnte nicht geladen werden.' };
  }
  if (!status.loaded) {
    return { entries: null, error: 'Der Zeitplan ist noch nicht geladen.' };
  }

  const entries: Array<{ starts_at: string; ends_at: string }> = [];
  for (const [index, row] of rows.entries()) {
    if (!row.von.trim() || !row.bis.trim()) {
      return {
        entries: null,
        error: `Zeit ${index + 1}: Beginn und Ende müssen ausgefüllt sein.`,
      };
    }
    const startsAt = toRelayZeit(row.von);
    const endsAt = toRelayZeit(row.bis);
    if (!startsAt || !endsAt) {
      return {
        entries: null,
        error: `Zeit ${index + 1}: Beginn oder Ende ist ungültig.`,
      };
    }
    entries.push({ starts_at: startsAt, ends_at: endsAt });
  }
  return { entries, error: null };
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

export type UplinkZielAbfrage = 'loading' | 'error' | 'ready';

/** Status für die Zielkarte, ohne eine fehlende Antwort als Trennung zu lügen. */
export function zielVerbindungsLabel(
  status: UplinkZielAbfrage,
  enabled: boolean | null | undefined,
): string {
  if (status === 'loading') return 'Wird geladen';
  if (status === 'error') return 'Status nicht verfügbar';
  return enabled === true ? 'Verbunden' : 'Nicht verbunden';
}

export type UplinkWartelistenAnzeige = 'loading' | 'error' | 'empty' | 'entries';

/** Eine leere Warteliste darf erst nach einer erfolgreichen Antwort erscheinen. */
export function wartelistenAnzeige(opts: {
  isLoading: boolean;
  isError: boolean;
  hasData: boolean;
  entryCount: number;
}): UplinkWartelistenAnzeige {
  if (opts.isError) return 'error';
  if (opts.isLoading || !opts.hasData) return 'loading';
  return opts.entryCount === 0 ? 'empty' : 'entries';
}

/**
 * Ob sich der Knopf abschicken lässt.
 *
 * Halb ausgefüllt lehnt der Server ab, deshalb gehen Adresse und Schlüssel
 * nur zusammen weg. Steht die Adresse nach dem automatischen Verbinden schon
 * im Feld und liegt der Schlüssel beim Server, bleibt trotzdem ein reines
 * Profil-Update möglich: dann fahren wir Adresse und Schlüssel gar nicht mit.
 *
 * Das gilt aber nur, solange die Adresse unangetastet die vorbefüllte ist.
 * Hat der Nutzer die Adresse selbst geändert, aber keinen neuen Schlüssel
 * eingetragen, lehnt der Knopf ab — sonst nimmt zielRumpf() die Adresse ohne
 * Schlüssel gar nicht mit, der Knopf meldet trotzdem "Gespeichert", und die
 * Änderung verschwindet stillschweigend.
 */
export function canSaveDestination(opts: {
  rtmpUrl: string;
  streamKey: string;
  urlTouched: boolean;
  profileTouched: boolean;
  verbunden: boolean;
}): boolean {
  const url = opts.rtmpUrl.trim();
  const key = opts.streamKey.trim();
  if (url && key) return true;
  if (opts.urlTouched) return false;
  if (!url && !key) return opts.verbunden && opts.profileTouched;
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

/** Liest eine positive ganze Zahl, optional mit dem Wert 0. */
export function zahlOderUndefined(wert: string, zeroErlaubt = false): number | undefined {
  const getrimmt = wert.trim();
  if (!getrimmt) return undefined;
  const zahl = Number(getrimmt);
  const erlaubt = zeroErlaubt ? zahl >= 0 : zahl > 0;
  return Number.isFinite(zahl) && erlaubt ? Math.round(zahl) : undefined;
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
  enabled: true;
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
    enabled: true;
    rtmp_url?: string;
    stream_key?: string;
    width?: number;
    height?: number;
    fps?: number;
    bitrate_kbps?: number;
  } = { platform: werte.platform, enabled: true };
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
export const UPLINK_SPEED_MITHALTEN = 'Unser Server hält Schritt.';

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

/**
 * Überschrift über dem Prozentwert aus `cpu_pct`.
 *
 * Der Wert gehört zur Session des Streamers, nicht zur ganzen Maschine. Die
 * Beschriftung muss das sagen, sonst liest er die Last aller anderen mit.
 */
export const UPLINK_LAST_LABEL = 'Rechenlast für deinen Stream';

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

import { fetchJson, withCookieCredentials } from './core';
import { normalisiereCaps } from '../uplinkEmpfehlung';
import type { UplinkCaps, UplinkCapsRoh } from '../uplinkEmpfehlung';

/**
 * `live_status` kommt nicht vom Relay, sondern aus der Twitch-Beobachtung des
 * Bots (tb-dashboard-api, handlers/uplink.rs).
 *
 * `unbekannt` heisst: der Stand ist zu alt oder es gibt keinen. Die Oberflaeche
 * behandelt das wie `live` und deckt nichts auf. Ein aelterer Server, der das
 * Feld noch nicht kennt, landet ueber `undefined` in derselben sicheren Ecke.
 */
export type UplinkLiveStatus = 'live' | 'aus' | 'unbekannt';

export interface UplinkMe {
  enabled: boolean;
  waitlisted: boolean;
  ingest_key: string;
  rtmp_url: string;
  srt_hint: string;
  live_status?: UplinkLiveStatus;
  /** Fuer die fertigen OBS-Dock-Adressen. Fehlt bei aelteren Servern. */
  twitch_login?: string;
  /** Wartezeit nur nach einem unerwarteten Internetabriss. */
  reconnect_wait_s: number;
  /** Vom Relay gelieferte Obergrenze, nicht im Frontend duplizieren. */
  reconnect_wait_max_s: number;
  /**
   * Unsere eigene Dock-Adresse fuer den Multi-Chat, sobald das Relay sie
   * kennt. Fehlt bei aelteren Servern und solange keine erzeugt wurde.
   */
  dock_url?: string;
  /** Ob es schon eine Dock-Adresse gibt, auch wenn sie hier nicht mitkommt. */
  dock_url_vorhanden?: boolean;
  /** Je Plattform ein Eintrag; was nicht gespeichert ist, ist getrennt. */
  verbindungen?: UplinkVerbindung[];
}

export type UplinkVerbindungStatus = 'verbunden' | 'neu_verbinden' | 'getrennt';

export interface UplinkVerbindung {
  platform: string;
  status: UplinkVerbindungStatus;
}

/**
 * Laesst das Relay eine neue Dock-Adresse ausstellen. Die alte gilt danach
 * nicht mehr, die neue kommt genau einmal in dieser Antwort zurueck.
 */
export function rotateUplinkDockToken(): Promise<{ dock_url: string }> {
  return fetchJson<{ dock_url: string }>(
    '/twitch/api/v2/uplink/dock-token/rotate',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    })
  );
}

/**
 * Startet das Verbinden einer Plattform. Das ist eine Browser-Navigation,
 * kein fetch: der Server leitet direkt zur Anmeldeseite der Plattform weiter.
 */
export function uplinkConnectUrl(platform: UplinkPlattform): string {
  return `/twitch/api/v2/uplink/connect/${platform}`;
}

/** Bis jetzt kann nur Twitch verbunden werden; die anderen folgen. */
export function verbindenAktiv(platform: UplinkPlattform): boolean {
  return platform === 'twitch';
}

export interface UplinkPlattformVerbindung {
  id: UplinkPlattform;
  label: string;
  status: UplinkVerbindungStatus;
  /** Ob der Knopf "Verbinden" etwas tut. */
  aktiv: boolean;
  /** Fertiger Statustext fuer die Oberflaeche. */
  statusText: string;
  /** Beschriftung des Verbinden-Links; null, wenn die Plattform noch nicht verbunden werden kann. */
  knopfText: string | null;
}

/** Eine Zeile je Plattform fuer den Block "Plattformen verbinden". */
export function plattformVerbindungen(me: UplinkMe): UplinkPlattformVerbindung[] {
  return UPLINK_PLATTFORMEN.map((p) => {
    const status = me.verbindungen?.find((v) => v.platform === p.id)?.status ?? 'getrennt';
    const aktiv = verbindenAktiv(p.id);
    // Verbinden holt nur den Chat-Zugang (Lesen und Schreiben); der Stream-Key
    // bleibt im Formular. Darum steht in jedem Text das Wort Chat.
    let statusText = 'Chat folgt';
    let knopfText: string | null = null;
    if (aktiv) {
      statusText =
        status === 'verbunden'
          ? 'Chat verbunden'
          : status === 'neu_verbinden'
            ? 'Chat abgelaufen'
            : 'Chat nicht verbunden';
      knopfText = status === 'getrennt' ? `Chat von ${p.label} verbinden` : 'Chat neu verbinden';
    }
    return { id: p.id, label: p.label, status, aktiv, statusText, knopfText };
  });
}

/**
 * Die vier fertigen Twitch-Fenster fuer OBS.
 *
 * Es sind dieselben Adressen, die OBS in seine eigenen Docks laedt: siehe
 * `frontend/oauth/TwitchAuth.cpp` im OBS-Quellcode. Die eingebauten Docks sind
 * selbst nur Browser-Fenster, sie werden nur automatisch angelegt, sobald ein
 * Twitch-Konto verbunden ist. Inhaltlich ist ein eigenes Dock dasselbe Fenster.
 *
 * Drei der vier Adressen kommen ohne Kanalnamen aus: Twitch leitet einen
 * angemeldeten Nutzer auf seinen eigenen Kanal weiter. Das ist robuster als
 * der Namensweg, weil es auch nach einer Namensaenderung noch stimmt. Nur der
 * Chat braucht den Kanal in der Adresse.
 */
export const OBS_DOCKS = [
  {
    titel: 'Chat',
    pfad: (k: string) => (k ? `https://www.twitch.tv/popout/${k}/chat?darkpopout` : ''),
  },
  {
    titel: 'Aktivitätsfeed',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/activity-feed',
  },
  {
    titel: 'Stream-Informationen',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/edit-stream-info',
  },
  {
    titel: 'Kanalpunkte',
    pfad: () => 'https://dashboard.twitch.tv/popout/stream-manager/community-points',
  },
] as const;

export const EIGENES_DOCK_TITEL = 'Multi-Chat';

export interface DockAdresse {
  titel: string;
  url: string;
  /** true fuer unsere eigene Adresse, false fuer die Twitch-Fenster. */
  eigene: boolean;
}

/**
 * Alle Dock-Adressen in Anzeigereihenfolge: unsere eigene zuerst, wenn das
 * Relay sie mitliefert, danach die Twitch-Fenster. Ohne Kanalname faellt der
 * Twitch-Chat weg, die drei uebrigen Fenster brauchen ihn nicht.
 */
export function dockAdressen(me: UplinkMe): DockAdresse[] {
  const liste: DockAdresse[] = [];
  const eigene = me.dock_url?.trim() ?? '';
  if (eigene) liste.push({ titel: EIGENES_DOCK_TITEL, url: eigene, eigene: true });
  for (const dock of OBS_DOCKS) {
    const url = dock.pfad(me.twitch_login ?? '');
    if (url) liste.push({ titel: dock.titel, url, eigene: false });
  }
  return liste;
}

export function fetchUplinkMe(): Promise<UplinkMe> {
  return fetchJson<UplinkMe>('/twitch/api/v2/uplink/me', withCookieCredentials());
}

export function joinUplinkWaitlist(): Promise<{ waitlisted: boolean }> {
  return fetchJson<{ waitlisted: boolean }>(
    '/twitch/api/v2/uplink/waitlist',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    })
  );
}

export interface UplinkAdminWaitlistEntry {
  streamer_id: number;
  requested_at: string;
  note?: string | null;
  enabled: boolean;
}

export function fetchUplinkAdminWaitlist(): Promise<{ entries: UplinkAdminWaitlistEntry[] }> {
  return fetchJson<{ entries: UplinkAdminWaitlistEntry[] }>(
    '/twitch/api/v2/uplink/admin/waitlist',
    withCookieCredentials(),
  );
}

export function acceptUplinkAdminWaitlistEntry(
  streamerId: number,
  csrfToken: string,
): Promise<{ streamer_id: number; enabled: boolean }> {
  return fetchJson<{ streamer_id: number; enabled: boolean }>(
    '/twitch/api/v2/uplink/admin/users',
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      body: JSON.stringify({ streamer_id: streamerId }),
    }),
  );
}

export interface UplinkReconnectWaitSettings {
  reconnect_wait_s: number;
  reconnect_wait_max_s: number;
}

export function saveUplinkReconnectWait(
  reconnectWaitS: number
): Promise<UplinkReconnectWaitSettings> {
  return fetchJson<UplinkReconnectWaitSettings>(
    '/twitch/api/v2/uplink/reconnect-wait',
    withCookieCredentials({
      method: 'PUT',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({ reconnect_wait_s: reconnectWaitS }),
    })
  );
}

/** Der Wert gilt nur fuer einen unerwarteten Abriss, nicht fuer OBS-Stop. */
export const UPLINK_RECONNECT_WAIT_TEXT =
  'Diese Zeit gilt nur nach einem unerwarteten Internetabriss. Wenn du den Stream in OBS beendest, räumt Uplink sofort auf.';

export function reconnectWaitEingabe(wert: number | null | undefined): string {
  return typeof wert === 'number' && Number.isFinite(wert) && wert >= 0 ? String(wert) : '';
}

/** Liest nur ganze, nichtnegative Sekunden; die Obergrenze bleibt beim Relay. */
export function reconnectWaitPayload(wert: string): number | null {
  const getrimmt = wert.trim();
  if (!getrimmt || !/^\d+$/.test(getrimmt)) return null;
  const sekunden = Number(getrimmt);
  return Number.isSafeInteger(sekunden) && sekunden >= 0 ? sekunden : null;
}

/**
 * Ein Videoprofil, wie das Relay es zurueckgibt.
 */
export interface UplinkProfilAnsicht {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

/**
 * Die Profilnamen muessen zum Katalog in `handlers/uplink.rs` passen. Ein Name,
 * den der Server nicht kennt, gibt 400 statt still auf den Standard zu fallen.
 *
 * `warnung` ist gesetzt, wo die Stufe zwar waehlbar, aber nicht unbedenklich
 * ist. Getrennt vom `hinweis`, damit die Oberflaeche sie anders faerben kann:
 * ein Nachteil, den man ueberliest, ist keiner.
 *
 * Die 1440p-Warnung steht auf den Angaben aus dem Twitch-Hilfeartikel "2k
 * Streaming auf Twitch": 2K braucht dort Enhanced Broadcasting, das gibt es nur
 * fuer Partner und Affiliates, und es geht an Twitchs eigenen Ingest. Ueber
 * Uplink laeuft klassisches RTMP, damit ist 1440p bei Twitch offiziell nicht
 * unterstuetzt und es gibt keine Qualitaetsstufen fuer die Zuschauer.
 *
 * Die Bitrate aus demselben Artikel steht bewusst NICHT in der Warnung. Sie
 * gilt fuer jemanden, der 2K direkt an Twitch schickt. Ueber Uplink geht
 * `PROFIL_WERTE['1440p60']` an Twitch, also 12000 kbps, und was der Streamer
 * zu uns hochlaedt, ist davon wieder unabhaengig. Eine Zahl, die auf keinen
 * dieser drei Wege passt, gehoert nicht in eine Warnung.
 */
export const UPLINK_PROFILE = [
  {
    name: '1440p60',
    label: '1440p60 (2K), 12000 kbps',
    hinweis: 'Deine volle 2K-Auflösung, ohne Verkleinerung. Schick uns dafür auch 1440p aus OBS.',
    warnung:
      'Twitch nimmt 2K offiziell nur über Enhanced Broadcasting an, und das läuft über Twitchs eigenen Ingest, nicht über uns. Für deine Zuschauer heißt das keine Qualitätsstufen: wer eine schwache Leitung hat, puffert, statt auf 720p zu wechseln. Probier es einen Abend aus und schick uns dafür auch 1440p aus OBS.',
  },
  {
    name: '1080p60-hoch',
    label: '1080p60, 8000 kbps',
    hinweis: 'Twitch-Maximum für 1080p. Nur mit Partner- oder Affiliate-Status sinnvoll.',
    warnung: '',
  },
  {
    name: '1080p60',
    label: '1080p60, 6000 kbps',
    hinweis: 'Standard. Passt auf jede Leitung, die Twitch akzeptiert.',
    warnung: '',
  },
  {
    name: '720p60',
    label: '720p60, 4500 kbps',
    hinweis: 'Weniger Serverlast und weniger Upload, immer noch 60 Bilder.',
    warnung: '',
  },
  {
    name: '480p30',
    label: '480p30, 1500 kbps',
    hinweis: 'Notfallstufe bei schlechter Leitung.',
    warnung: '',
  },
] as const;

export type UplinkProfilName = (typeof UPLINK_PROFILE)[number]['name'];

/** Die Zahlen hinter jedem Profilnamen, gespiegelt aus `handlers/uplink.rs`. */
export const PROFIL_WERTE: Record<UplinkProfilName, [number, number, number, number]> = {
  '1080p60': [1920, 1080, 60, 6000],
  '1080p60-hoch': [1920, 1080, 60, 8000],
  '1440p60': [2560, 1440, 60, 12000],
  '720p60': [1280, 720, 60, 4500],
  '480p30': [854, 480, 30, 1500],
};

/**
 * Findet den Profilnamen zu einem gespeicherten Ziel.
 *
 * Ohne das startet die Auswahl immer auf dem Standard, und wer nur seinen
 * Stream-Key erneuert, schickt still 1080p60 mit und aendert damit eine
 * Qualitaetsstufe, die er nie angefasst hat. `null` heisst: das gespeicherte
 * Profil steht nicht im Katalog, dann bleibt die Auswahl, wo sie ist.
 */
export function profilNameFuer(werte: UplinkProfilAnsicht | undefined): UplinkProfilName | null {
  if (!werte) return null;
  const treffer = (Object.keys(PROFIL_WERTE) as UplinkProfilName[]).find((name) => {
    const [w, h, f, b] = PROFIL_WERTE[name];
    return werte.width === w && werte.height === h && werte.fps === f && werte.bitrate_kbps === b;
  });
  return treffer ?? null;
}

/**
 * Die vier Plattformen, die das Relay kennt. Reihenfolge ist die der
 * Zielkarten: Twitch zuerst, weil es fuer fast alle das einzige Ziel ist.
 */
export const UPLINK_PLATTFORMEN = [
  { id: 'twitch', label: 'Twitch', rtmp: 'rtmp://live.twitch.tv/app' },
  { id: 'youtube', label: 'YouTube', rtmp: 'rtmp://a.rtmp.youtube.com/live2' },
  { id: 'kick', label: 'Kick', rtmp: 'rtmps://fa723fc1b171.global-contribute.live-video.net' },
  { id: 'tiktok', label: 'TikTok', rtmp: '' },
] as const;

export type UplinkPlattform = (typeof UPLINK_PLATTFORMEN)[number]['id'];

/** Freie Werte aus dem manuellen Modus. */
export interface UplinkManuellesProfil {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

/**
 * Was die Aenderung mit einem gerade laufenden Stream gemacht hat.
 *
 * Fehlt, wenn es nichts zu sagen gibt: kein Stream, oder es lief ohnehin
 * schon so. `applied` heisst, dass nur die Bitrate wechselt und niemand
 * etwas merkt; `applied_restart` heisst, dass das Bildformat wechselt und die
 * Zuschauer kurz ein Stocken sehen; `too_busy` heisst, gespeichert ist es,
 * aber der laufende Stream bleibt bis zum naechsten Mal, wie er ist.
 */
export interface UplinkLiveQualitaet {
  status: 'applied' | 'applied_restart' | 'too_busy';
  message: string;
}

/**
 * Ein Ziel speichern. Drei Faelle, alle ueber denselben Aufruf:
 *
 * - Zugangsdaten neu setzen: `rtmp_url` und `stream_key` zusammen.
 * - Nur die Qualitaet aendern: beides weglassen. Genau das ging vorher nicht,
 *   und deshalb sah es aus, als wuerde die Auswahl nicht gespeichert.
 * - An- oder abschalten: `enabled`.
 *
 * `profil` und `manuell` schliessen sich aus; der Server lehnt beides
 * zusammen mit 400 ab, statt sich still fuer eins zu entscheiden.
 */
export function saveUplinkDestination(body: {
  platform: UplinkPlattform;
  rtmp_url?: string;
  stream_key?: string;
  profil?: UplinkProfilName;
  manuell?: UplinkManuellesProfil;
  enabled?: boolean;
}): Promise<{ destinations: UplinkDestination[]; live_quality?: UplinkLiveQualitaet }> {
  return fetchJson('/twitch/api/v2/uplink/destinations', withCookieCredentials({
    method: 'PUT',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }));
}

/**
 * Was eine Plattform an Video empfiehlt. Liegt in `uplinkEmpfehlung.ts`, damit
 * die Umschrift der Serverantwort ohne den Fetch-Unterbau pruefbar bleibt, und
 * steht hier weiter zur Verfuegung, wo die uebrigen Uplink-Typen liegen.
 */
export type { UplinkCaps } from '../uplinkEmpfehlung';

export interface UplinkCapsAntwort {
  platforms: UplinkCaps[];
}

/**
 * Der Empfehlungskatalog. Kommt vom Server, damit die Oberflaeche ihn nicht
 * doppelt pflegt: `relay.platform_caps` ist eine Tabelle in einem anderen
 * Repo und kann sich per Migration bewegen, ohne dass hier jemand etwas tut.
 */
export async function fetchUplinkCaps(): Promise<UplinkCapsAntwort> {
  const antwort = await fetchJson<{ platforms?: UplinkCapsRoh[] }>(
    '/twitch/api/v2/uplink/caps',
    withCookieCredentials()
  );
  return { platforms: (antwort.platforms ?? []).map(normalisiereCaps) };
}

/**
 * Ein gespeichertes Ziel, wie das Relay es zurueckgibt.
 *
 * Ohne Stream-Key: der liegt verschluesselt in der Datenbank und wird nie
 * wieder ausgeliefert. Fuer die Oberflaeche zaehlt nur, dass es ihn gibt.
 */
export interface UplinkDestination {
  platform: string;
  rtmp_url: string;
  enabled: boolean;
  /** Was der Streamer eingestellt hat, und damit auch das, was rausgeht. */
  requested?: UplinkProfilAnsicht;
  /**
   * Frueher das Ergebnis der Klemmung gegen die Plattform-Grenzen, heute immer
   * identisch mit `requested`. Steht nur noch im JSON, damit aeltere Clients
   * nicht brechen. Die Oberflaeche liest ausschliesslich `requested`: sonst
   * zeigt sie wieder einen anderen Wert an, als im Eingabefeld steht.
   */
  effective?: UplinkProfilAnsicht;
}

export function fetchUplinkDestinations(): Promise<{ destinations: UplinkDestination[] }> {
  return fetchJson<{ destinations: UplinkDestination[] }>(
    '/twitch/api/v2/uplink/destinations',
    withCookieCredentials()
  );
}

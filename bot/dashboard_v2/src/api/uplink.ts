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
 */
export const UPLINK_PROFILE = [
  {
    name: '1440p60',
    label: '1440p60 (2K), 12000 kbps',
    hinweis: 'Deine volle 2K-Auflösung, ohne Verkleinerung. Schick uns dafür auch 1440p aus OBS.',
    warnung:
      '2K gibt es bei Twitch nur mit Enhanced Broadcasting, und das ist Partnern und Affiliates vorbehalten. Enhanced Broadcasting geht an Twitchs eigenen Ingest und schließt unseren Weg aus: über Uplink kommt dein Stream als klassisches RTMP an, und darüber unterstützt Twitch 1440p offiziell nicht. Für deine Zuschauer heißt das keine Qualitätsstufen: wer eine schwache Leitung hat, kann nicht auf 720p wechseln, sondern puffert. Twitch nennt für 2K ohne Enhanced Broadcasting 20 Mbit/s Upload, gesendet wird über Uplink aber von uns, deine eigene Leitung muss das nicht tragen. Wählbar bleibt die Stufe: probier sie einen Abend aus, bevor du dabei bleibst. Nur aus OBS solltest du dann auch 1440p schicken, aus 1080p hochrechnen kostet Bitrate ohne Gegenwert.',
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
}): Promise<{ destinations: UplinkDestination[] }> {
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

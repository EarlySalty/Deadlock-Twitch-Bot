import { fetchJson, withCookieCredentials } from './core';

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
 * Die Profilnamen muessen zum Katalog in `handlers/uplink.rs` passen. Ein Name,
 * den der Server nicht kennt, gibt 400 statt still auf den Standard zu fallen.
 *
 * 1440p fehlt mit Absicht: der normale Twitch-Ingest nimmt nur 1080p60 an,
 * hoehere Aufloesungen gehen dort ausschliesslich ueber Enhanced Broadcasting,
 * und das setzt OBS mit verbundenem Twitch-Konto voraus, also ohne uns.
 */
export const UPLINK_PROFILE = [
  { name: '1080p60', label: '1080p60, 6000 kbps', hinweis: 'Standard. Passt auf jede Leitung, die Twitch akzeptiert.' },
  { name: '1080p60-hoch', label: '1080p60, 8000 kbps', hinweis: 'Twitch-Maximum. Nur mit Partner- oder Affiliate-Status sinnvoll.' },
  { name: '720p60', label: '720p60, 4500 kbps', hinweis: 'Weniger Serverlast und weniger Upload, immer noch 60 Bilder.' },
  { name: '480p30', label: '480p30, 1500 kbps', hinweis: 'Notfallstufe bei schlechter Leitung.' },
] as const;

export type UplinkProfilName = (typeof UPLINK_PROFILE)[number]['name'];

export function saveUplinkTwitchDestination(body: {
  rtmp_url: string;
  stream_key: string;
  profil: UplinkProfilName;
}): Promise<{ platform: string; width: number; height: number }> {
  return fetchJson(
    '/twitch/api/v2/uplink/destinations',
    withCookieCredentials({
      method: 'PUT',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({
        platform: 'twitch',
        rtmp_url: body.rtmp_url,
        stream_key: body.stream_key,
        profil: body.profil,
      }),
    })
  );
}

/**
 * Ein gespeichertes Ziel, wie das Relay es zurueckgibt.
 *
 * Ohne Stream-Key: der liegt verschluesselt in der Datenbank und wird nie
 * wieder ausgeliefert. Fuer die Oberflaeche zaehlt nur, dass es ihn gibt.
 */
export interface UplinkProfilAnsicht {
  width: number;
  height: number;
  fps: number;
  bitrate_kbps: number;
}

export interface UplinkDestination {
  platform: string;
  rtmp_url: string;
  enabled: boolean;
  /** Was der Streamer bestellt hat. */
  requested?: UplinkProfilAnsicht;
  /** Was nach der Klemmung gegen die Plattform-Caps wirklich rausgeht. */
  effective?: UplinkProfilAnsicht;
}

export function fetchUplinkDestinations(): Promise<{ destinations: UplinkDestination[] }> {
  return fetchJson<{ destinations: UplinkDestination[] }>(
    '/twitch/api/v2/uplink/destinations',
    withCookieCredentials()
  );
}

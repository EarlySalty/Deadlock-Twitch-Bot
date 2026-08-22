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

export function saveUplinkTwitchDestination(body: {
  rtmp_url: string;
  stream_key: string;
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
      }),
    })
  );
}

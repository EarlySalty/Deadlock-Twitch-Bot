import { fetchJson, withCookieCredentials } from './core';

export interface UplinkMe {
  enabled: boolean;
  waitlisted: boolean;
  ingest_key: string;
  rtmp_url: string;
  srt_hint: string;
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

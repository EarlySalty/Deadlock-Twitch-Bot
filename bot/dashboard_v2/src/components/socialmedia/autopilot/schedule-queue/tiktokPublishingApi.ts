import { fetchJson, withCookieCredentials } from '@/api/core';

export type TikTokPrivacy = 'PUBLIC_TO_EVERYONE' | 'MUTUAL_FOLLOW_FRIENDS' | 'FOLLOWER_OF_CREATOR' | 'SELF_ONLY';

export interface TikTokPublishing {
  privacy_level: TikTokPrivacy | '';
  allow_comment: boolean;
  allow_duet: boolean;
  allow_stitch: boolean;
  fixed_hashtags: string[];
  caption_footer: string;
  music_consent: boolean;
  account_connected: boolean;
}

const PATH = '/twitch/social-media/api/autopilot/tiktok-publishing';

function url(twitchUserId?: string): string {
  const query = new URLSearchParams();
  if (twitchUserId) query.set('twitch_user_id', twitchUserId);
  return `${PATH}${query.size ? `?${query}` : ''}`;
}

export function loadTikTokPublishing(twitchUserId?: string, signal?: AbortSignal): Promise<TikTokPublishing> {
  return fetchJson(url(twitchUserId), withCookieCredentials({ signal }));
}

export function saveTikTokPublishing(settings: TikTokPublishing, twitchUserId?: string): Promise<TikTokPublishing> {
  return fetchJson(url(twitchUserId), withCookieCredentials({
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  }));
}

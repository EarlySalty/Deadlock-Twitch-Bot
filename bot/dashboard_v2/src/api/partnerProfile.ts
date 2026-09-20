import { fetchAuthStatus } from './auth';
import { fetchJson, withCookieCredentials } from './core';

export interface ProfileEvent { id: string; title: string; description: string; starts_at: string; ends_at: string }
export interface TwitchProfileSchedule {
  id: string;
  title: string;
  starts_at: string;
  ends_at: string;
  is_recurring: boolean;
}
export interface TwitchProfileClip {
  id: string;
  title: string;
  url: string;
  thumbnail_url: string;
  view_count: number;
}
export interface TwitchProfileSnapshot {
  available: boolean;
  display_name: string;
  description: string;
  profile_image_url: string;
  banner_url: string;
  live: { title: string; game_name: string; thumbnail_url: string } | null;
  clips: TwitchProfileClip[];
  schedule: TwitchProfileSchedule[];
}
export interface ProfileContent {
  headline: string;
  about: string;
  avatar_url: string;
  banner_url: string;
  accent: 'gold' | 'violet' | 'teal';
  socials: { label: string; url: string }[];
  featured: string[];
  main_heroes: string[];
  rank: string;
  playstyles: string[];
  preferred_times: string[];
  show_history: boolean;
  sync_twitch_schedule: boolean;
  events: ProfileEvent[];
}
export interface PartnerProfileData {
  login: string;
  public_path: string;
  active: boolean;
  published: boolean;
  revision: number;
  profile: ProfileContent;
  twitch?: TwitchProfileSnapshot | null;
}
const BASE = '/twitch/api/v2/streamer/profile';
function endpoint(streamer?: string, refreshTwitch = false) {
  const params = new URLSearchParams();
  if (streamer) params.set('streamer', streamer);
  if (refreshTwitch) params.set('refresh_twitch', 'true');
  const query = params.toString();
  return query ? `${BASE}?${query}` : BASE;
}
export function fetchPartnerProfile(streamer?: string, refreshTwitch = false) {
  return fetchJson<PartnerProfileData>(endpoint(streamer, refreshTwitch), withCookieCredentials({ cache: 'no-store' }));
}
export async function savePartnerProfile(data: PartnerProfileData, streamer?: string) {
  const auth = await fetchAuthStatus();
  const csrf = auth.csrfToken || auth.csrf_token;
  return fetchJson<PartnerProfileData>(endpoint(streamer), withCookieCredentials({
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', ...(csrf ? { 'X-CSRF-Token': csrf } : {}) },
    body: JSON.stringify({ revision: data.revision, published: data.published, profile: data.profile }),
  }));
}

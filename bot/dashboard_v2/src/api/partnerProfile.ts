import { fetchAuthStatus } from './auth';
import { fetchJson, withCookieCredentials } from './core';

export interface ProfileEvent { id: string; title: string; description: string; starts_at: string; ends_at: string }
export interface ProfileContent {
  headline: string;
  about: string;
  avatar_url: string;
  accent: 'gold' | 'violet' | 'teal';
  socials: { label: string; url: string }[];
  featured: string[];
  show_history: boolean;
  events: ProfileEvent[];
}
export interface PartnerProfileData {
  login: string;
  public_path: string;
  active: boolean;
  published: boolean;
  revision: number;
  profile: ProfileContent;
}
const BASE = '/twitch/api/v2/streamer/profile';
function endpoint(streamer?: string) { return BASE + (streamer ? `?streamer=${encodeURIComponent(streamer)}` : ''); }
export function fetchPartnerProfile(streamer?: string) {
  return fetchJson<PartnerProfileData>(endpoint(streamer), withCookieCredentials({ cache: 'no-store' }));
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

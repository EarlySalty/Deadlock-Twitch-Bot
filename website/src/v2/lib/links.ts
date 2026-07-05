import {
  DISCORD_INVITE_URL,
  TWITCH_AFFILIATE_URL,
  TWITCH_AGB_URL,
  TWITCH_BOT_AUTH_START_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_SECURITY_URL,
} from "../../data/externalLinks";

export {
  DISCORD_INVITE_URL,
  TWITCH_AFFILIATE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_SECURITY_URL,
};

export const V2_BASE = "/streamer/v2/";
export const V2_FEATURES = "/streamer/v2/features/";
export const V2_FAQ = "/streamer/v2/faq/";
export const ROADMAP_URL = "/twitch/roadmap";

/** Statischer No-JS-Fallback des CTA (ohne ts-Cache-Buster). */
export const TWITCH_BOT_AUTH_FALLBACK = `${TWITCH_BOT_AUTH_START_URL}?scope_profile=base&source=website_v2`;

/** CTA mit eigener source, damit V2-Conversions im Funnel unterscheidbar sind. */
export function buildBotAuthUrl(): string {
  const url = new URL(TWITCH_BOT_AUTH_START_URL);
  url.searchParams.set("scope_profile", "base");
  url.searchParams.set("source", "website_v2");
  url.searchParams.set("ts", Date.now().toString());
  return url.toString();
}

export function twitchChannelUrl(login: string): string {
  return `https://twitch.tv/${encodeURIComponent(login)}`;
}

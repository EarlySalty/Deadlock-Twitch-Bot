export const DISCORD_INVITE_URL = "https://dc.earlysalty.com";
export const EARLYSALTY_WEBSITE_URL = "https://earlysalty.de/website/";
export const TWITCH_PUBLIC_ORIGIN = "https://twitch.earlysalty.com";
export const TWITCH_ONBOARDING_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/onboarding`;
export const TWITCH_FAQ_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/faq`;
export const TWITCH_LOGIN_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/auth/login`;
export const TWITCH_DASHBOARD_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/dashboard`;
export const TWITCH_DASHBOARD_V2_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/dashboard-v2`;
export const TWITCH_DEMO_DASHBOARD_URL = "https://demo.earlysalty.com/twitch/demo/";
export const TWITCH_LIVE_ANNOUNCEMENT_URL =
  `${TWITCH_PUBLIC_ORIGIN}/twitch/live-announcement`;
export const TWITCH_ABBO_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/abbo`;
export const TWITCH_AFFILIATE_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/affiliate`;
export const TWITCH_SOCIAL_MEDIA_URL = `${TWITCH_PUBLIC_ORIGIN}/social-media`;
export const TWITCH_IMPRESSUM_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/impressum`;
export const TWITCH_DATENSCHUTZ_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/datenschutz`;
export const TWITCH_AGB_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/agb`;

export const TWITCH_BOT_AUTH_START_URL =
  `${TWITCH_PUBLIC_ORIGIN}/twitch/raid/auth`;

export function buildTwitchDashboardLoginUrl(
  nextPath: string = "/twitch/dashboard",
): string {
  const url = new URL(TWITCH_LOGIN_URL);
  url.searchParams.set("next", nextPath);
  return url.toString();
}

export function buildTwitchBotAuthUrl(): string {
  const url = new URL(TWITCH_BOT_AUTH_START_URL);
  url.searchParams.set("scope_profile", "base");
  url.searchParams.set("source", "website_onboarding");
  // Unique URL per click avoids stale/cached redirects and forces fresh OAuth state.
  url.searchParams.set("ts", Date.now().toString());
  return url.toString();
}

export interface SocialMediaChannel {
  login: string;
  twitchUserId: string | null;
}

/** Ein Name ist nur ein Hinweis; ausgewählt wird genau ein belegter ID-Datensatz. */
export function resolveSocialMediaChannel(
  channels: readonly SocialMediaChannel[],
  twitchUserId: string | null | undefined,
  loginHint?: string | null,
): SocialMediaChannel | undefined {
  if (!twitchUserId || !/^\d+$/.test(twitchUserId)) return undefined;
  const matches = channels.filter((channel) => channel.twitchUserId === twitchUserId);
  if (matches.length !== 1) return undefined;
  const channel = matches[0];
  if (loginHint && channel.login.toLowerCase() !== loginHint.trim().toLowerCase()) return undefined;
  return channel;
}

export function globalTimezoneUrl(twitchUserId: string | undefined): string {
  const params = new URLSearchParams({ twitch_user_id: twitchUserId ?? '' });
  return `/twitch/verwaltung?${params}#konto`;
}

export function globalTimezoneOwner(search: string, authenticatedUserId: string | undefined): string | undefined {
  const params = new URLSearchParams(search);
  const owner = params.has('twitch_user_id') ? params.get('twitch_user_id') : authenticatedUserId;
  return owner && /^\d+$/.test(owner) ? owner : undefined;
}

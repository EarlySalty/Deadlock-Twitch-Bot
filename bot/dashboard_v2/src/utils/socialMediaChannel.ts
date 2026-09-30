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

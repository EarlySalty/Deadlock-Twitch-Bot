export const GUILD_ONBOARDING_ROOT = '/twitch/guild-onboarding';

export interface GuildChoice {
  guild_id: string;
  name: string | null;
  language: string;
  auto_partner_default: boolean;
}

export interface GuildOnboardingSession {
  twitch_login: string;
  discord_linked: boolean;
  guilds: GuildChoice[];
  memberships: { guild_id: string; active: boolean; can_rejoin: boolean }[];
}

export function automaticGuild(session: GuildOnboardingSession): string | null {
  if (!session.discord_linked || session.guilds.length !== 1) return null;
  const id = session.guilds[0].guild_id;
  const membership = session.memberships.find(item => item.guild_id === id);
  return !membership || membership.can_rejoin ? id : null;
}

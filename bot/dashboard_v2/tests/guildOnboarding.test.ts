import { describe, expect, it } from 'vitest';
import { automaticGuild, type GuildOnboardingSession } from '../src/api/guildOnboarding';
import { GUILD_ONBOARDING_EN } from '../src/i18n/guildOnboarding';

const session: GuildOnboardingSession = {
  twitch_login: 'test', discord_linked: true,
  guilds: [{ guild_id: '1289721245281292288', name: 'Community', language: 'de', auto_partner_default: true }],
  memberships: [],
};

describe('guild onboarding selection', () => {
  it('automatically selects exactly one verified server without losing snowflake precision', () => {
    expect(automaticGuild(session)).toBe('1289721245281292288');
  });
  it('never automatically selects before Discord verification or for zero or multiple matches', () => {
    expect(automaticGuild({ ...session, discord_linked: false })).toBeNull();
    expect(automaticGuild({ ...session, guilds: [] })).toBeNull();
    expect(automaticGuild({ ...session, guilds: [...session.guilds, { ...session.guilds[0], guild_id: '700' }] })).toBeNull();
  });
  it('does not re-submit a pending or already active membership', () => {
    for (const active of [true, false]) {
      expect(automaticGuild({ ...session, memberships: [{ guild_id: session.guilds[0].guild_id, active }] })).toBeNull();
    }
  });
  it('translates every dedicated onboarding label to English', () => {
    for (const [de, en] of Object.entries(GUILD_ONBOARDING_EN)) {
      expect(en).not.toBe(de);
      expect(en.length).toBeGreaterThan(0);
    }
  });
});

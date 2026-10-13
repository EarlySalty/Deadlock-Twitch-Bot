# Discord Hub

The Hub (`tb-discord-hub`) is the Twitch bot's Discord integration for foreign servers.

## Database

Set `DATABASE_URL` to the Twitch Postgres DSN. Guild events then persist:

- `GUILD_CREATE` upserts `discord_guilds` (`status = active`)
- `GUILD_DELETE` marks `status = removed`
- `GUILD_MEMBER_REMOVE` deactivates matching `streamer_guilds` via `twitch_streamer_identities.discord_user_id`

Without `DATABASE_URL` the Hub still forwards to the broker and logs gateway events (`LoggingSink`). READY replay stays idempotent.

## `/setup`

Global slash command. English text on foreign servers. Options: `language`, `announce_channel`, `alert_channel`, `streamer_role`, `ping_role`, `invite`, `auto_partner`. The ping role must be mentionable and sit below the Hub role. On the German community guild the command only shows the existing notice and does not write.

## Onboarding Discord OAuth

Streamer onboarding uses the Hub Discord app (`DISCORD_HUB_CLIENT_ID` / `DISCORD_HUB_CLIENT_SECRET`) with scopes `identify guilds`. Redirect URI: `TB_ONBOARDING_DISCORD_REDIRECT_URI`. Access tokens are never stored.

## systemd

Give the Hub unit the same `DATABASE_URL` as the Twitch bot. Restart `tb-discord-hub` after deploying a binary that includes the Postgres sink.

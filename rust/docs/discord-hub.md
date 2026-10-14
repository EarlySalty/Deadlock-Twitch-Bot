# Discord hub: dormant H1 cut

`tb-discord-hub` is a standalone workspace binary with an embedded build revision, graceful shutdown and an authenticated internal HTTP interface. This cut adds no service, launcher, deploy integration, OAuth handler, gateway connection or database access. Do not redirect running bot traffic to it yet.

Requests under `/internal/master/v1/` use `X-Internal-Token`. Without `X-Discord-Guild-Id`, or with the German community guild ID, method, path, raw query, body, content type and idempotency key pass through to the existing broker. Status, content type and response body pass back unchanged. Foreign guilds use Discord REST with channel and guild isolation, restricted mentions, bounded rate-limit retries and process-local idempotency. Unsupported foreign-guild endpoints return 501; without a bot token they return 503, never a broker fallback.

## Existing application only

Use **Deadlock Twitch Bot**, application/client ID `1537305565292924998`. The bot token is injected from the existing Infisical entry `DISCORD_DEADLOCK_TWITCH_BOT`; the binary does not fetch or persist credentials. Its value must be a Discord **bot token**, not an OAuth client secret. The credential value was not inspected for this cut. Internal authentication reuses `MASTER_BROKER_TOKEN`, then `MAIN_BOT_INTERNAL_TOKEN`, then `TWITCH_INTERNAL_API_TOKEN`; no second internal credential is required.

Non-secret configuration uses `DISCORD_DEADLOCK_TWITCH_BOT_HOST` (default numeric loopback), `_PORT` (standalone default 8771, verify availability before use) and `_COMMUNITY_GUILD_ID`. An optional `_INTERNAL_TOKEN` overrides the resolved broker token. Broker routing uses existing `MASTER_BROKER_BASE_URL` or `MASTER_BROKER_HOST`/`MASTER_BROKER_PORT`. These are process configuration names, not a request for new secret entries or an environment file.

The reserved OAuth redirect is `https://deutsche-deadlock-community.de/twitch/auth/discord/hub/callback`. This binary does not perform OAuth and does not need an OAuth client secret. Existing account linking uses its separate application and is unchanged. A later OAuth implementation will need the existing application's client secret separately from its bot token if the current entry contains only the bot token.

The required installation permission mask is `268454912`: view channels, send messages, embed links and manage roles. No administrator or mention-everyone permission. Application identity, redirect and permission mask are fixed constants in the config module.

## Remaining cut

Gateway `/setup`, guild join/leave, member leave and persistence remain unported. Their reference code depends on G0's extended `discord_guilds` and `streamer_guilds` schema, currently reverted on main. A later cut must validate schema readiness before starting foreign-guild operations, test it only against disposable local Postgres, preserve this application's existing slash commands and include interaction-token log/error redaction from reference `5d6b8ade2`. Deploy support from `1b21f0f0c` is intentionally not installed here. No production DB parity or live Discord behavior is claimed.

## Open items after round 3

The fixed installation mask does not include Create Instant Invite for `create-invite`, Read Message History for `message-reactions`, or Manage Messages for deleting another author's message. Review operation-specific installation permissions in a later cut without broadening H1's fixed permission constants.

Idempotency remains process-local and bounded by its TTL and capacity. Gateway and schema persistence remain outside H1.

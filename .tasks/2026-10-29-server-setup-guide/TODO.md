# Server setup guide for partner communities

State: guide page drafted at website/public/faq/server-setup/index.html (route /twitch/faq/server-setup/), NOT merged.
Gate (gpt-6.1-sol, 2026-10-29) BLOCKED because the runtime does not deliver what the guide describes. Verified:

- Go-live posts go to the single global `config.twitch.notify_channel_id` (rust/bin/tb-bot/src/main.rs ~2078, announce/sink.rs ~430); per-guild `announce_channel_id` is not used for live posts; no role pings.
- Moderation alerts and streamer-role sync use global targets (chat_wiring.rs ~1166, streamer_role_sync.rs ~575).
- `/setup` without options never sets the guild active (tb-discord-hub/src/guilds.rs ~204), so onboarding lists nothing.
- Guild onboarding Twitch login has no scopes, so no raid authorization; auto-raid rejects unauthorized sources (auto_raid.rs ~308).
- cross_server_raids defaults true; same-language targets from other communities are admitted (tb-db/src/guild_membership.rs ~103).
- Invalid roles return the generic "Nothing was saved" message, not the role-specific notes.
- Production: guild_runtime.enabled = false, only the home guild registered, Hub gateway never connected in 30 days.

Continue: decide whether to build per-guild delivery (announce channel, ping role, language, alerts, roles) plus raid auth in onboarding, and enable the runtime. Then rewrite the guide against the real behavior and re-run gate-review.

## Session register

| Package | Thread | Creator | Model | Status | Branch |
|---|---|---|---|---|---|
| Multi-server A-E (sub-orchestrator) | mcp:45e98718-6eac-4a78-80cc-cc4870f715db | 3870f9b0-591b-40cd-b430-e09f1136613f | claude-opus-5-5 medium | started 2026-10-29 | orchestrator/multiserver-20261029 |

Briefing: BRIEFING-multiserver.md. Guide rewrite waits for E and the user's activation decision.

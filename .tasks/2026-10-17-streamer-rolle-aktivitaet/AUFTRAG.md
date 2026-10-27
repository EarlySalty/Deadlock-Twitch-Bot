# Streamer role: bot access, activity, live role, per-server policy

## Goal (user, 2026-10-17)
1. No Streamer role unless the bot is usable in the channel: banned, not modded or no working token/auth means no role. No exceptions.
2. Activity: role is removed after 4 weeks (28 days) without a Deadlock stream. It comes back only after Deadlock streams on 4 distinct days (Europe/Berlin) within 30 days. No reward for AFK streamers.
3. Live role "Live Streamer" (1560993753643098153, purple, above Streamer 1313624729466441769) on top of the Streamer role while live with Deadlock, same conditions.
4. Voice dot "🟣 " before a voice channel name while a live streamer sits in it (Deadlock-Bots, dl-voice, branch feat/voice-live-streamer-dot).
5. The bot partner inactivity flag (inactivity_flagged_at via on_auto_archive/on_auto_unarchive) uses the same hysteresis: flag after 28 days without Deadlock, clear only after 4 distinct Deadlock days in 30 days. Today it clears after 1 stream.
6. Per-server policy: the rules must be configurable per Discord server for the multi-server rollout (discord_guilds, Server-Admin-Dashboard package A in .tasks/2026-10-12-multiserver-englisch/PLAN.md section 4.5). Store the policy as columns on discord_guilds with the user's values as defaults; the home guild row keeps those values. Package A exposes them in its UI later.

## Policy columns (new migration, never edit applied ones)
- live_role_id bigint NULL (home row: 1560993753643098153)
- role_activity_method text NOT NULL DEFAULT 'deadlock_days' CHECK IN ('deadlock_days','any_stream_days','none')
- role_keep_window_days int NOT NULL DEFAULT 28
- role_gain_window_days int NOT NULL DEFAULT 30
- role_gain_min_days int NOT NULL DEFAULT 4
- role_require_bot_access boolean NOT NULL DEFAULT true
Sane CHECK bounds (1..365, min_days <= gain_window).

## Acceptance
- tb-bot role sync reads the home guild policy from discord_guilds (fallback to defaults if row/columns missing) and enforces 1-3.
- Inactivity flag follows 5.
- cargo check/clippy/tests for touched crates green via cargo-slot; fresh_schema_snapshot updated.
- Live after deploy: banned/unmodded/token_error partners lose the role; live Deadlock partners get the live role.

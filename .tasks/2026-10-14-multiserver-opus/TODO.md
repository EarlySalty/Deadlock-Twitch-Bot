# Open after round 2 (2026-10-15)

Live: 408557c6a. Everything below is blocked on a user decision; no branch work is open.

1. G #1063 and K: wait for confirmed prod DB credential rotation after the 13.10 leak. Then G first (G0 schema kept on `feat/opus-g0-shadow-schema-20261014` at b3c7b5dc8, reverted on main 47f3e26ab).
2. C #1067 and A #1064: start after G is merged.
3. L #1069: Fireworks returns HTTP 412 (account suspended, 159 LLM_USAGE_RECOVERY 412 entries in 6 h). Needs billing decision by the user.
4. H1 hub `rust/bin/tb-discord-hub` is merged but not wired as a service. Wiring and rollout is a separate package.
5. Canonical checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` is on a stale HEAD with rustls 0.23.45; push-gate scans started there fail. Push from worktrees.

Release build note: use `ops/twitch-release-build <sha>` (stamps build provenance, includes tb-stt-server), then `deploy-twitch-release <sha>`.

# Added 2026-10-17: Streamer role policy for package A (#1064)

Live since 31bafc391 (migration 20261017120000): `discord_guilds` has per-guild role policy columns that the Server-Admin-Dashboard must expose for its own guild only:

- `live_role_id` (optional Live Streamer role, added on top of the Streamer role while live in the target game)
- `role_activity_method`: `deadlock_days` | `any_stream_days` | `none`
- `role_keep_window_days` (default 28), `role_gain_window_days` (default 30), `role_gain_min_days` (default 4); CHECK 1..365 and min <= gain window
- `role_require_bot_access` (default true; false skips everything except `blocked`)

Home guild 1289721245281292288 keeps the defaults (owner's requirement). Read path is `rust/bin/tb-bot/src/streamer_role_sync.rs`; today it only loads the home guild row. Package A must make the sync iterate all active guilds before foreign admins can rely on these settings.

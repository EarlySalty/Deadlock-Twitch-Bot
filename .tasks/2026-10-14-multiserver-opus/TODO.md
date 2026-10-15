# Open after round 2 (2026-10-15)

Live: 408557c6a. Everything below is blocked on a user decision; no branch work is open.

1. G #1063 and K: wait for confirmed prod DB credential rotation after the 13.10 leak. Then G first (G0 schema kept on `feat/opus-g0-shadow-schema-20261014` at b3c7b5dc8, reverted on main 47f3e26ab).
2. C #1067 and A #1064: start after G is merged.
3. L #1069: Fireworks returns HTTP 412 (account suspended, 159 LLM_USAGE_RECOVERY 412 entries in 6 h). Needs billing decision by the user.
4. H1 hub `rust/bin/tb-discord-hub` is merged but not wired as a service. Wiring and rollout is a separate package.
5. Canonical checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` is on a stale HEAD with rustls 0.23.45; push-gate scans started there fail. Push from worktrees.

Release build note: use `ops/twitch-release-build <sha>` (stamps build provenance, includes tb-stt-server), then `deploy-twitch-release <sha>`.

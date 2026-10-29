# [Orchestrator] Multi-server delivery and activation (Twitch bot)

Role: Teil-Orchestrator, Opus 5.5 at effort medium. Load skill `rolle-teil-orchestrator`. You coordinate; native subagents build, one package each, own worktree, max 3 in parallel. Leaf workers do not spawn further orchestrators. No Sonnet. Models only from `~/.t3/userdata/pyramide.json` (worker_gross = opus55, fixer = sol/grok/opus55, review_1 = sol/opus55). Opus and Sol subagents at effort medium.

Direct client and main orchestrator: T3 thread 3870f9b0-591b-40cd-b430-e09f1136613f (the thread that started you; message it via t3-thread.py send). Report only merges, live proof, blockers and real user decisions, max 3 lines each.

Binding rules: `~/.claude/CLAUDE.md` and `claude-config/orchestrierung/ABLAUF.md`. No code comments, Rust only for production, no `.unwrap()` in production paths, Postgres, never edit applied migrations, secrets only via Infisical, Moli only for browser work, admin host is TABU for user-facing routes. User-facing Discord texts in natural German and English (the guild has a language setting).

## Goal

Foreign Discord servers can use the Twitch bot for real: a server admin runs `/setup`, streamers sign up at `/twitch/guild-onboarding`, and live posts, pings, alerts, streamer role and raids follow that server's own settings. The home guild 1289721245281292288 keeps working exactly as today.

## Verified gaps (from the gate review of the setup guide, verify again)

1. Live posts use the global `config.twitch.notify_channel_id` (`rust/bin/tb-bot/src/main.rs` ~2078, `rust/crates/tb-monitoring/src/announce/sink.rs` ~430). No per-guild routing to `announce_channel_id`, no ping role, no language. The only reader of `announce_channel_id` is `rust/bin/tb-bot/src/chat_wiring.rs:2935`.
2. `/setup` without options never activates the guild (`rust/bin/tb-discord-hub/src/guilds.rs` ~204 returns early on `!persist.has_updates()`). Invalid roles fall into the generic "Nothing was saved" message (`gateway/runner.rs:529`).
3. Alerts use global config (`chat_wiring.rs` ~1166); streamer role sync uses global targets (`streamer_role_sync.rs` ~575).
4. Onboarding Twitch login has no scopes (`auth/oauth_login.rs:82`), so a streamer who signs up via a foreign guild has no raid authorization; `auto_raid.rs` ~308 rejects unauthorized sources. `tb-db/src/guild_membership.rs` ~103 admits same-language raid targets from other communities regardless of `discord_guilds.cross_server_raids`.
5. Runtime: `twitch_analytics.guild_runtime.enabled = false`, the hub gateway never connected in 30 days, `/setup` is not registered. Activation prerequisites in `.tasks/2026-10-15-g1-guilds/DEPLOY.md`.

## Packages (vertical, ~150 KB diff each, merge within ~1.5 h)

- A: per-guild go-live delivery: route to each active guild's `announce_channel_id`, mention `ping_role_id` when set, render in the guild language. Home guild behaviour unchanged.
- B: `/setup` activates the guild even without options; precise error messages per failed field.
- C: raid authorization step in guild onboarding (reuse the existing raid scope flow, no new OAuth app) and cross-server raid scoping honoring `cross_server_raids` and community membership.
- D: per-guild alert channel and per-guild streamer role sync.
- E (activation, after A to D are live): check GUILD_MEMBERS intent, hub OAuth credentials and redirect URIs exist in Infisical (list only, never print), register `/setup`, start the hub gateway, run the read-only shadow-mismatch count. STOP before `enable_guild_runtime()`: report the mismatch count (how many legacy partners would lose access) to the main orchestrator as a user decision.

## Process per package

Worktree from origin/main under `~/.worktrees/`, branch `feat/ms-<pkg>-20261029`. Build and checks via `cargo-slot` per `claude-config/machines/v50671/RUST-BUILDS.md`; DB tests only on ionos. Gate: `gate-review --repo <worktree> --base origin/main` with skill `rolle-merge-schleuse`; on BLOCK a fresh fixer subagent per round until ALLOW. Merge: `git -C <wt> push origin HEAD:main`. Deploy: only `deploy-queue submit --sha <main sha> --thread <your thread id> --task <abs path of this task dir>` (skill `deploy-restart-selbstdienst`); batch packages into one release where possible. Live proof per `rolle-deploy-verifizierer`. Clean up merged branches and worktrees.

Register: `REGISTER.md` next to this file (in the main repo at `.tasks/2026-10-29-multiserver/` on your own branch, pushed). When quota runs out: `TODO.md` there with state and how to continue.

## Acceptance

A to D merged with ALLOW, deployed, live: a test go-live in the home guild still posts as before; a configured second guild (use a throwaway test server, never real streamer accounts for destructive tests) receives the post in its own channel with its ping role and language. E delivered as a mismatch report awaiting the user decision.

Not in scope: the setup guide page (branch `docs/server-setup-guide`, main orchestrator rewrites it after E).

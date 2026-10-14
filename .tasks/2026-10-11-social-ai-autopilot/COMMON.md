# Common rules for all packages (P1, P2, P3)

Role: Teil-Orchestrator for one area in the native Claude Code harness. You may start native Agent subagents for research, build and review inside your session, with disjoint write paths. Do not start extra T3 threads or further orchestration levels. Do not ask the user; questions go to the main orchestrator as a turn ending with `FRAGE AN ORCHESTRATOR:` (situation, options, recommendation, branch, SHA, worktree).

Main orchestrator: T3 thread `eddfce29-0b2a-4f47-931f-8b5387672f94` (Claude Opus session `cc9b61b9-7d63-4977-9486-b11a84bdb047`). Contract: `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/AUFTRAG.md`. Spec: `BRIEF.md` in the same folder. Inventory: `INVENTORY-2026-10-07.md` there.

Binding workspace rules: `/home/nathanael/.claude/CLAUDE.md` and `/home/nathanael/Documents/claude-config/orchestrierung/ABLAUF.md`. Key points:
- Production code only in Rust; frontend toolchains stay. No code comments. No `.unwrap()` in production paths. Postgres only, new migrations only (your package prefix), never edit applied ones.
- AI calls only via the central provider (`tb-llm`, `llm_dispatch.rs`); model/provider/timeouts from config, never hard-coded.
- Secrets only via the Infisical export script, never printed.
- English for code, commits, docs; German with real umlauts and ß for all UI text.
- Use the `code-suche` skill / `graphify query` before grep. Browser checks only with Moli (`/home/nathanael/.local/bin/moli`, read `claude-config/wissen/agent-browser.md`); never Brave.
- Rust builds via `cargo-slot <args> --jobs 3` (see `claude-config/machines/v50671/RUST-BUILDS.md`); at most one release build on the host.
- Stage only your own files, commit verified states, publish your branch to origin promptly. No force updates, no bypassed hooks. Do not touch other worktrees or branches that existed before your task.

Delivery loop per milestone (small packages, no batch integration):
1. Build, `cargo fmt`, clippy/tests for touched crates, frontend `npm run build` and relevant tests, browser check for UI.
2. Rebase onto current origin/main. Merge gate: `gate_hook.py --review` per the `rolle-merge-schleuse` skill. On BLOCK start a fresh native fixer subagent per round, rerun until ALLOW; after five failed rounds report cause and proposal to the main orchestrator.
3. You are integrator for your own milestones: merge to main after ALLOW, publish main, deploy the verified origin/main SHA via the deploy wrapper (`deploy-restart-selbstdienst` skill, `/usr/local/bin/deploy-twitch-release <sha>`). The wrapper does NOT run migrations: apply new migrations to prod by hand as documented in the deploy skill, before the restart that needs them. Verify the live function with real data and a browser screenshot for UI.
4. Keep `.tasks` files out of main.

Reporting to the main orchestrator (only at milestone end, decision needed, or blocker), max 5 lines: milestone, SHA, gate verdict, live yes/no with proof, open points. Settle yourself at the very end with `python3 ~/Documents/tools/t3-thread.py settle --selbst`.

P1, P2, P3 run in parallel on separate branches. Shared files (`tb-social-media/src/lib.rs`, `tb-dashboard-api/src/lib.rs`, `rust/bin/tb-bot/src/main.rs`, `bot/dashboard_v2/src/api/socialMedia.ts`) get minimal append-style edits. Rebase before each gate run.

# BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-caster-teams-cams

You are a leaf implementation worker. Do not start T3 threads. Native fixer subagents at medium effort are allowed for gate BLOCK rounds. Do not use Sonnet. Opus 5.5 is rate-limited until 14:00 Europe/Berlin; do not wait for it. Ask the orchestrator with `FRAGE AN ORCHESTRATOR:`, never the user.

## 1. Goal and contract

Spec: `.tasks/2026-10-19-caster-teams-cams/AUFTRAG.md`
Admin: https://admin.deutsche-deadlock-community.de/twitch/admin/content/caster-overlay
Predecessor Opus worker died at the 2pm-reset session limit after finishing P1 on the branch and leaving P2 unwired. Do not resume that idle subagent.

P1 (already committed, not on main): Discord role "Turnier Caster" (id 1495154811799077067, guild 1289721245281292288) feeds the caster picker. One permanent camera per Discord id. Moli proof: `.tasks/2026-10-19-caster-teams-cams/screens/p1-*.png`.

P2 remaining (WIP commit `d33ec5b30`):
- `caster_teams.rs` and migration exist.
- Overlay HTML and `CasterSceneSettings.tsx` already show Coach and a team library UI.
- Missing: `SceneCoach`, `library_id`, `normalize_team`, `team_valid`, `discord_id_valid` on `caster_overlay.rs` (the new module already calls them).
- Missing: `pub mod caster_teams` in `handlers/mod.rs`.
- Missing: admin routes in `lib.rs` next to the other caster-overlay routes.
- Missing: `client.ts` types and functions the UI already imports (`fetchCasterTeamLibrary`, `createCasterLibraryTeam`, `updateCasterLibraryTeam`, `deleteCasterLibraryTeam`, `searchCasterCoachMembers`, `CasterSceneCoach`, `CasterCoachMember`, `libraryId` on `CasterSceneTeam`).
- Reproduce what is broken in the free team editor (create, edit, logo, swap, save, reload) and fix causes.
- Coach is Discord search or free text. Overlays already render `Coach: <name>`. Prove at 1920x1080.

P3 after P2 is on main:
- `caster_camera.html` currently `getUserMedia({video:true})`. Request 2560x1440@60, fall back to best the device supports, contentHint, sender maxBitrate 12-16 Mbps, maxFramerate 60, degradationPreference maintain-resolution, keep resolution over fps.
- Show actual res/fps from `track.getSettings` on the camera page.
- OBS subscriber stays native resolution. Fake media device is enough for proof.

UI German with real umlauts. No em dashes. No code comments. No `.unwrap()` on production paths. Rust backend. No new SQLite. New migration only, never edit applied ones.

## 2. Ownership

Write only caster overlay + team library paths:
- `rust/crates/tb-dashboard-api/src/handlers/caster_*.rs`, `caster_*.html`
- `rust/crates/tb-dashboard-api/src/lib.rs` (routes only)
- `rust/crates/tb-dashboard-api/src/handlers/mod.rs` (mod line)
- `rust/migrations/20261019130000_caster_team_library.sql` and any new P3 migration if needed
- `bot/admin_dashboard/src/pages/content/CasterOverlay.tsx`, `CasterSceneSettings.tsx`, `casterScene.ts`
- `bot/admin_dashboard/src/api/client.ts` (caster types/functions only)
- `.tasks/2026-10-19-caster-teams-cams/` evidence

Do not touch `~/repos/Deadlock-Twitch-Bot`. Do not format unrelated files. Do not release-build or deploy. Do not delete the worktree or branch.

## 3. Working state

Worktree: `/home/nathanael/.worktrees/tb-caster-teams-cams`
Branch: `feat/caster-teams-cams`
HEAD: `d33ec5b30` (pushed). Clean tree expected.
Fetch `origin/main` before merging. Rebase only if needed for the gate.
Integrator is the parent Grok intent thread. You merge each package after ALLOW.

## 4. Proof

Per package: cargo fmt + clippy + affected tests via `cargo-slot` (see `/home/nathanael/Documents/claude-config/machines/v50671/RUST-BUILDS.md`), admin_dashboard lint/typecheck/build, Moli at 1440 and 390 (read `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` first; never Brave). Then `python3 /home/nathanael/Documents/claude-config/hooks/gate_hook.py --review` (rolle-merge-schleuse). On BLOCK: fresh fixer subagent per round, max 5. On ALLOW: merge locally and `git push origin HEAD:main` with literal paths, one git step per bash when main is involved. Inherited findings on origin/main do not block.

P1 was never merged. After P2 compiles and is proven, gate the combined P1+P2 delta vs origin/main and merge that as one package (session death made a clean P1-only merge unsafe). Then P3 separately.

Commit and push WIP after each meaningful step.

## 5. Routing

Auftraggeber and Hauptorchestrator: T3 thread `808e8c98-61e2-4605-b7d0-d4a690450c3b`.
Stop after P3 is on origin/main. Final report max 8 lines: merge SHA per package, gate verdicts, Moli paths, open points.
If a foreign tool hangs >30 minutes, skip that artifact with a note.

Search with graphify first (`code-suche`). Step rules: `~/.claude/agents/sol-worker.md`.

# P3: Social Media Manager dashboard

Base: origin/main `3862b764a`. Worktree: `/home/nathanael/.worktrees/tb-social-dash`. Branch: `feat/social-dashboard-redesign-20261011`.

Goal: refine the existing charcoal and gold Social Media Manager, group existing work areas, and separate account authorization, video approval and publishing status for YouTube, TikTok and Instagram.

Contracts: read COMMON.md, AUFTRAG.md and BRIEF.md in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`. P3 owns existing Social Media pages and components, navigation, studio.css and minimal existing backend handler changes. P1 owns new autopilot components; P2 owns new AI components. Mount their components only after they land on main.

M1 acceptance: grouped navigation, denser readable existing views, truthful generalized platform status, preserved layout editor/preview/approval/accounts/VOD paths, relevant checks and independent acceptance review, local gate ALLOW, merged and live. Moli before/after screenshots at 1440, 768 and 390 px must be opened using dashboard_v2 preview mode and synthetic fixtures. Own free loopback ports only. Live behaviour proof uses the documented X-Internal-Token admin API, read-only DB checks and service logs. No new login, live-site header injection, demo login, or reading saved session files. The main orchestrator owns the user-assisted logged-in UI check; report the remaining pages for that check.

Navigation must reserve an AI editing and review area in M1. P2 exports `AiEditorPanel({ twitchUserId: string, streamer: string, clipDbId?: number })` from `components/socialmedia/ai/AiEditorPanel.tsx`. Its contract is in `/home/nathanael/.worktrees/tb-social-ai/.tasks/2026-10-11-social-ai-autopilot/AUFTRAG.md`, section M1 application/UI contract. Rebase and mount immediately when that file lands on main, independent of full redesign completion. Clip-card/detail entry points pass `clipDbId`. Preserve and relocate any minimal current-shell mount P2 lands first; never change P2 internals.

M2 acceptance: mount P1/P2 landed components consistently, including responsive source/output review comparison. No reimplementation of their functionality.

Proof artifacts stay outside the repository. Task records stay out of main. No production comments, no secrets or token files. Browser only Moli. Build/check commands use the documented wrappers. Only this package integrator merges and deploys after the gate.

Verification decision: P1 measured scoped clippy on origin/main `3862b764a` with exit 101 and four existing `tb-social-media` lints. Treat those four as baseline, not the earlier `tb-raid` note. Do not fix untouched lines, create baseline-comparison tooling, or repeat unrelated suites. Publish build-verified intermediate commits promptly.

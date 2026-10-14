# [Orchestrator] Social AI finish build: sub-orchestrator (GPT-6.1 Sol), 2026-10-14

Role: sub-orchestrator (skill rolle-teil-orchestrator). Main orchestrator: T3 thread 6e3faead-77a5-4642-97ca-be9e870a68f2 (Claude Opus). You run in the Claude Code harness on GPT-6.1 Sol. Spawn the packages below as your own native `Agent` subagents (they inherit your model, Sol). Run independent packages in parallel in the background. Do NOT start T3 threads. Do not implement packages yourself; you coordinate, review reports and judge acceptance.

Folder: /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/
Shared rules for every package: COMMON.md in that folder. Spec: ../BRIEF.md (acceptance = section 32), contracts ../AUFTRAG.md.

## State at handover

All six previous package agents (Claude Opus) stopped on a usage limit mid-work. Their UNCOMMITTED changes are still in their worktrees (all based on origin/main 437b641ad, branches exist, nothing committed). origin/main has since moved to d21e72c6a (REL cleanup merged as 9a3ad772e). Live release is still 00e750557.

| Package | Original brief | Worktree | Branch | Uncommitted files |
|---|---|---|---|---|
| AP autopilot critical path | orig-5.md | ~/.worktrees/tb-social-ap-finish | feat/social-ap-finish-20261014 | 14 |
| AI editor gaps | orig-6.md | ~/.worktrees/tb-social-ai-finish | feat/social-ai-finish-20261014 | 18 |
| LA learning + admin | orig-7.md | ~/.worktrees/tb-social-la-finish | feat/social-la-finish-20261014 | 14 |
| RD dashboard redesign | orig-8.md | ~/.worktrees/tb-social-rd | feat/social-rd-redesign-20261014 | 9 |
| CFG Social AI config into bot.toml + admin Betriebseinstellungen | orig-10.md + BRIEF-CFG.md | ~/.worktrees/tb-social-cfg | feat/social-cfg-bot-toml-20261014 | 18 |
| REL upload cleanup | orig-9.md | worktree removed | merged as 9a3ad772e | 0 |

## What to spawn

One native subagent per package AP, AI, LA, RD, CFG. Each prompt: "Read <folder>/orig-N.md and COMMON.md and follow them" plus this resume note:
- Your worktree and branch already exist with uncommitted work from the previous agent. Do not recreate them. Start with `git status` and `git diff`, compare with your package list, continue from there. Commit verified intermediate states and push the branch promptly.
- Merge origin/main into your branch before the gate.
- Use fresh native subagents for gate fixer rounds.
- Report at most 8 lines: SHA on main, gate verdict, deployed release, live proof, open items.

REL: no new build. After the first package deploys, confirm 9a3ad772e is in the live release. If nothing deploys within the session, deploy origin/main yourself per COMMON.md.

## Ordering and conflicts

- Write paths are split per brief; keep them split. RD restyles around the AP and AI components; when those merge, RD merges main and keeps their behaviour.
- CFG phase 1 must put the `[social_ai]` section into the live bot.toml before any release containing CFG starts. Never print bot.toml.
- Deploys: one at a time (wrapper lock). A later deploy of origin/main covers earlier merges; then verify only.
- The deploy wrapper does not run migrations: check for unapplied migrations before each deploy (memory twitch-connect-rank-live-und-migrator-luecke.md under ~/.claude/projects/-home-nathanael-Documents/memory/).

## Hard limits (unchanged)

No paid AI calls, no real publishing, no enabling the autopilot or changing settings on real streamer accounts. No secrets in output. Moli only for browsers, never Brave. No code comments. German UI text with real umlauts.

## Your report

When all packages are merged, deployed and live-proven (or concretely blocked), run a combined acceptance check against BRIEF section 32, then update ../TODO.md and ../REGISTER.md on branch docs/social-ai-autopilot (commit + push that branch only). Include RD's screenshot paths (1440 and 390) in your report. End your final turn with at most 12 lines: per package SHA on main / blocker, live release, acceptance gaps. For a real product decision, end your turn with `FRAGE AN ORCHESTRATOR:` and the question. Last step: `python3 ~/Documents/tools/t3-thread.py settle --selbst`.

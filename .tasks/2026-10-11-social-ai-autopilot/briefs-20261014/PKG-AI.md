# [Orchestrator] Resume note, Social AI finish build (2026-10-14)

Role: leaf worker for exactly one package (named below). Orchestrator: T3 thread 6e3faead-77a5-4642-97ca-be9e870a68f2 (Claude Opus). Do not start T3 threads or further orchestrators. Native `Agent` subagents are allowed only for gate fixer rounds and independent checks.

Decisions belong to the orchestrator. Implement exactly what the brief says. When something is not decided by the brief (product behaviour, UI structure beyond the brief, scope changes, a choice between two valid designs, migration of live data), do not pick yourself: end your turn with `FRAGE AN ORCHESTRATOR:` plus the question, the options and what you found. Do not stop for things the brief or COMMON.md already settle.

State: the previous agent for your package (Claude Opus) stopped on a usage limit mid-work. Its UNCOMMITTED changes are still in your worktree (based on origin/main 437b641ad; the branch already exists). Do not recreate worktree or branch. Start with `git status` and `git diff`, compare with the package list, continue from there. Commit verified intermediate states and publish the branch to origin promptly.
origin/main has moved (d21e72c6a, includes the REL cleanup 9a3ad772e). Merge origin/main into your branch before the gate. Live release is 00e750557.
Deploy: the wrapper does not run migrations; check for unapplied migrations before deploying (memory twitch-connect-rank-live-und-migrator-luecke.md under ~/.claude/projects/-home-nathanael-Documents/memory/). One deploy at a time (wrapper lock).

Final report, at most 8 lines: SHA on main, gate verdict, deployed release, live proof, open items. Then `python3 ~/Documents/tools/t3-thread.py settle --selbst`.

---
Your package: AI. Worktree /home/nathanael/.worktrees/tb-social-ai-finish.

Read /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/COMMON.md first and follow it. Your package: AI (AI editor gaps). Branch feat/social-ai-finish-20261014, worktree ~/.worktrees/tb-social-ai-finish.

Audit (origin/main 437b641ad) of the manual AI workflow: code wired end to end but never run live (0 AI jobs). Paths: sm = rust/crates/tb-social-media/src, SMA = rust/crates/tb-dashboard-api/src/handlers/social_media_ai.rs, ui = bot/dashboard_v2/src/components/socialmedia.
Build:
1. Publishing language (BRIEF 16): publish_language (de|en|auto, default de) exists in sm/autopilot_settings.rs and is read by the pipeline (pipeline.rs:466) but no route writes it and no UI shows it. Add it to the editor options route in SMA / sm/ai_editor/options.rs (write it with your own narrow update function inside ai_editor, do not edit autopilot_settings.rs, another package owns it) and a select in ai/EditorOptionsPanel.tsx.
2. Publishing metadata (BRIEF 16): per-platform title/description/hashtags are generated (source.rs:117, stage_contract.rs:22) and used at publish (produced_video.rs:534) but never shown or editable. Add a route to read and edit a version's per-platform copy (validated lengths per platform) and a new ai/MetadataPanel.tsx mounted from RevisionControls / AiEditorPanel.
3. Review polish (BRIEF 18): show removed sections derived from kept intervals, and an optional synchronized playback toggle for original vs output in AiEditorPanel.tsx/.css.
4. Auto-pilot revisions (BRIEF 19): only manual jobs can be revised (owned_version requires job_id IS NULL in revision.rs). Allow natural-language revisions for AI videos produced by the autopilot runner too, and add the revision entry point to the autopilot review panel (components/socialmedia/autopilot/publish-review/*), reusing RevisionControls. A revised version replaces the one awaiting approval and any approval bound to the old file hash must be invalidated.
5. Investigate the three "Manual AI production could not finish (Storage)" warnings in the bot journal on Oct 12-13 (journalctl -u for the bot system unit, read-only; no secrets): find the cause and fix it if it is in your paths.
No paid AI calls: tests and fixtures only.
You own: sm/ai_editor/** except learning.rs, learning_tests.rs, skill_registry.rs; SMA; ui ai/**; ui autopilot/publish-review/**; append-only in ui socialMedia api file / ai/api.ts. Do NOT edit sm/autopilot_schedule/**, sm/production_jobs.rs, sm/autopilot_settings.rs, ScheduleQueuePanel, pages/SocialMedia.tsx, the learning/skill files or admin dashboard (other packages).
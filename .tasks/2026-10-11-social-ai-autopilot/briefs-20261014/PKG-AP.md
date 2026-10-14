# [Orchestrator] Resume note, Social AI finish build (2026-10-14)

Role: leaf worker for exactly one package (named below). Orchestrator: T3 thread 6e3faead-77a5-4642-97ca-be9e870a68f2 (Claude Opus). Do not start T3 threads or further orchestrators. Native `Agent` subagents are allowed only for gate fixer rounds and independent checks.

Decisions belong to the orchestrator. Implement exactly what the brief says. When something is not decided by the brief (product behaviour, UI structure beyond the brief, scope changes, a choice between two valid designs, migration of live data), do not pick yourself: end your turn with `FRAGE AN ORCHESTRATOR:` plus the question, the options and what you found. Do not stop for things the brief or COMMON.md already settle.

State: the previous agent for your package (Claude Opus) stopped on a usage limit mid-work. Its UNCOMMITTED changes are still in your worktree (based on origin/main 437b641ad; the branch already exists). Do not recreate worktree or branch. Start with `git status` and `git diff`, compare with the package list, continue from there. Commit verified intermediate states and publish the branch to origin promptly.
origin/main has moved (d21e72c6a, includes the REL cleanup 9a3ad772e). Merge origin/main into your branch before the gate. Live release is 00e750557.
Deploy: the wrapper does not run migrations; check for unapplied migrations before deploying (memory twitch-connect-rank-live-und-migrator-luecke.md under ~/.claude/projects/-home-nathanael-Documents/memory/). One deploy at a time (wrapper lock).

Final report, at most 8 lines: SHA on main, gate verdict, deployed release, live proof, open items. Then `python3 ~/Documents/tools/t3-thread.py settle --selbst`.

---
Your package: AP. Worktree /home/nathanael/.worktrees/tb-social-ap-finish.

Read /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/COMMON.md first and follow it. Your package: AP (Auto-Pilot critical path). Branch feat/social-ap-finish-20261014, worktree ~/.worktrees/tb-social-ap-finish.

Audit (read-only, origin/main 437b641ad) found the autopilot cannot publish end to end. Live DB: 1 settings row with empty target_platforms, 0 jobs, 184 pool clips. Paths: sm = rust/crates/tb-social-media/src, ui = bot/dashboard_v2/src.
Build, in this order, merging and deploying after A if that is quick, otherwise all together:
A. Target platforms: add target_platforms to SchedulePatch with validation against platforms the user has actually connected (sm/autopilot_schedule/settings.rs), schedule PUT handler (tb-dashboard-api, schedule route around api/lib.rs:375-446), client types in ui socialMedia api, platform checkboxes in ScheduleQueuePanel.tsx. Unblocks start, partial approval (PublishReviewPanel builds its list from target_platforms), full auto promote, static publish.
B. Review-job resolution + unsuitable clips (BRIEF section 6 "Unsuitable Clips"/"Uncertain"): jobs in `review` currently have no way out, and has_automatic_review_on / eligible_head_on in sm/production_jobs.rs stop all automatic production while any job sits in review. Add a resolve action (process anyway / skip, with stored reason) in sm/autopilot_schedule/queue.rs + recovery.rs, a route, and a new ui component ReviewJobActions.tsx mounted in ScheduleQueuePanel; one review job must not block the queue (continue with the next clip); add an "unsuitable" reason the AI path can set (keep the AI side minimal: a reason value plus explanation field; do not touch ai_editor/pipeline.rs, another package owns it).
C. Manual clip selection: planner select_on ignores selected_at. Decision (orchestrator): selected clips are produced first in the user's sort order, then the normal filter and sort continue; unselected clips are not excluded. Adjust ImportPoolPanel.tsx wording in German accordingly.
E. Retime and backlog accuracy: saving a changed schedule retimes already queued jobs and stale produced-video slots to the new shared slots (no catch-up burst); review_count_on (sm/autopilot_schedule/mod.rs) must exclude manual AI review videos.
Also add one test asserting static mode produces zero social_editor llm_usage rows if a cheap fixture path exists.
You own: sm/autopilot_schedule/**, sm/production_jobs.rs, sm/clip_pool/**, sm/autopilot_settings.rs, the schedule/queue/pool handlers in tb-dashboard-api, ui components/socialmedia/autopilot/** except publish-review/**, ImportPoolPanel.tsx, append-only in the ui socialMedia api file. Do NOT edit: sm/ai_editor/**, sm/autopilot_publish/**, components/socialmedia/ai/**, autopilot/publish-review/**, pages/SocialMedia.tsx (other packages), except trivial route registration lines.
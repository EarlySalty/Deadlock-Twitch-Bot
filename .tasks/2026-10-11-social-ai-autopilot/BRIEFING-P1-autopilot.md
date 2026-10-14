[Orchestrator] Package P1: Auto-Pilot, clip pool, scheduling, publishing

Read first: COMMON.md, AUFTRAG.md (contracts C1 to C4), BRIEF.md sections 3 to 9, 24, 25, 26, 28, 32 in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`.

Worktree: `/home/nathanael/.worktrees/tb-social-autopilot`, branch `feat/social-autopilot-20261011` from origin/main `3862b764a`. Clean.

Goal: the Auto-Pilot workflow and the Storage/Reliability blocks of BRIEF section 32 work live, built on the existing scheduler, posting plan, approval, clip queue, upload worker, retention and Helix clip client (inventory R02 to R13, R22, R30). Extend, do not rebuild.

Milestones, in order:
- M0 (first; merge quickly because P2 and P3 depend on it): migrations and Rust modules for C1 `autopilot_settings`, C2 `produced_video` (including `enqueue_for_publishing`), C3 `social_production_jobs` schema. No behaviour change for existing users. Report the merged SHA to the main orchestrator immediately.
- M1: historical Deadlock-only clip import into a selectable pool (Helix pagination, date windows, rate limits, resumable import, dedup, selected/processed/published tracking). Strict date filter and user sort order. Importing many clips must not trigger processing.
- M2: one shared schedule with timezone (browser default, override, DST), lead time best effort, flexible late publishing without night posts and without catch-up bursts; sequential production runner (one job at a time, reorder, mark next, remove unstarted). Static mode uses the existing render path and makes no gameplay-analysis model calls. `ai` mode calls `ai_editor::produce` per C3 once P2 provides it, otherwise holds the job for review.
- M3: review mode vs full automation (mode switch dialog with explicit handling of pending videos), review backpressure at two pending, per-platform approval (all or individual), partial approval discards unapproved destinations for that event, independent idempotent retries per platform, upload of the exact produced file without re-render, pause/resume.
- M4: retention (7-day review expiry, keep approved until published, cleanup after publish, no recycling, empty pool pauses and notifies) and notifications via existing channels only (dashboard, Discord; e-mail only if an integration exists), linking to the dashboard.

Backend API in new handler file `social_media_autopilot.rs`. UI for settings, schedule, clip pool and queue as new components under `bot/dashboard_v2/src/components/socialmedia/autopilot/`, mounted minimally; P3 owns final placement and visual redesign of existing pages.

Proof: demonstrate each Auto-Pilot and Storage/Reliability acceptance item from BRIEF 32 against the live system with a real channel. Use review mode for live proof; never publish to real accounts without the user's approval in the existing flow.

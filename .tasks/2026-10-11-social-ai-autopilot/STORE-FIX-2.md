# Persistent job-store repair, round 2

Worktree: `/home/nathanael/.worktrees/tb-social-ai-jobs`.
Branch: `feat/social-ai-jobs-20261011`.
Parent verified clean HEAD: `791b83aba49eca24ef838779b0ace50279c5221e`.
The completed round-1 implementer/reviewer must not be resumed.

## Parent-confirmed bounded findings

1. Source identity is not frozen. Creation stores the numeric source row but no original Twitch clip identifier. Claim/load/retry/finish eligibility checks the numeric row, owner, category, discarded state and source kind only. Changing that row's `clip_id` keeps eligibility true and preserves earlier preparation/checkpoints. Bind jobs to the original Twitch identifier and fail closed on an identity change. The schema and existing disposable fixture both have `twitch_clips_social_media.clip_id TEXT`. Preserve the immutable numeric `clip_db_id`, nullable FK deletion behavior and history. Ordinary title/metadata refresh must not needlessly invalidate a job.
2. Same-lease recovery has a load bypass. `claim_job` sets `lease.claimed_work`, but `load_job_for_runner` accepts an active job under a newly acquired lease without protecting it from subsequent `recover_stale_jobs`. Recovery then marks that same loaded work failed. Either enforce the required ownership/load sequence or establish the loaded active-work guard. Keep terminal-history inspection and fresh-lease stale recovery working; do not blindly mark every terminal read as owned live work.
3. List reads do one version query per job and `SELECT j.*` transfers private checkpoint JSON unnecessarily. A full 50-job page performs 51 queries even with zero versions and can read up to 20 MB of checkpoint data. Use explicit public-view columns and batched bounded version loading. Preserve owner/manual_ai_job_id filters, stable pagination/order and per-job version limit. Also avoid `j.*` on public single-job detail. Private snapshots remain runner-only, never exposed through public labels.

## Ownership and acceptance

Own `ai_editor/store.rs`, its still-unapplied P2 migration `20261011200002_social_ai_jobs.sql`, and `tests/ai_editor_store.rs` only. Fresh origin/main at 07:53 UTC is still `3862b764a56327435860eece92337e5968809c89`; P1 M0 is not landed. Do not change applied migrations, foreign P1 code or neutral shared lease implementation prematurely. Preserve cumulative known-to-unknown provider replacement, compute-only metric preservation, bounded full plan/timed original transcript checkpoints, history/deletion behavior and no second budget.

Check creation/preparation followed by changed Twitch clip_id through claim/load/retry/finish; matching unchanged identity remains usable. Check newly acquired lease loading active work followed by attempted stale recovery; separate fresh lease may recover genuinely stale work, terminal loads stay compatible. Check public list/detail exclude checkpoint payloads and preserve bounded scoped version grouping. Retain all eight existing disposable-PG tests and relevant formatter/build/lint. Stub-schema tests are not P1 M0 integration proof.

Use shared cargo-slot release/jobs3/sccache/central artifacts. No slot bypass, foreign process stops or build/cache copies. Strict previous lint stopped in unchanged tb-raid; distinguish package no-deps lint from transitive lint and report exact failures, not an assumed identical baseline. Commit only verified owned files with an English message ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

No agents/workflows/T3/session coordination, production DB/data, secrets, paid calls, push/main/gate/deploy or publication. Process inspection pid/comm only; never arguments/environment or the excluded core reviewer log. English work/report, natural German product errors, no added code comments or em dashes. Parent owns combined runtime/P1/provider integration, intent acceptance and actual gate/live proof. Report SHA, exact paths/API adjustments, check results and remaining dependencies.

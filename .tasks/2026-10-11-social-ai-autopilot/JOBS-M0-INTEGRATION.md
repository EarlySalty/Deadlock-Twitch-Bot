# Persistent manual jobs: integrate the real M0 contract

## Goal and references

Finish the existing manual job store against actual merged C1/C2/C3, preserving its reviewed behavior. Read COMMON.md and AUFTRAG.md under `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`, STORE-FIX-2.md here and the relevant source. Parent fetched and verified M0 `33656f2c2d2154a8ce888ed1a39180935209ea6e`; genuine orchestrator reports it live with migration applied and disabled/static/Review defaults. Rebase this clean unpushed branch onto that exact ref first. Do not duplicate P1's now-merged Social lint fixes.

## Ownership and preserved state

Owned worktree `/home/nathanael/.worktrees/tb-social-ai-jobs`, branch `feat/social-ai-jobs-20261011`, clean preserved HEAD `d0d421b19dd42938068b6bdcbcd726f0bff886e6`. Prior bounded source/DB ALLOW and twelve disposable-PG tests exist; actual M0 integration did not. All previous workers are completed. You are the sole writer.

Own ai_editor/store.rs, its existing tests and unapplied package migration as necessary, plus the approved additive produced_video::insert_with_metadata helper and a neutral production_lease module with minimal append-style lib.rs exposure. Preserve other packages and actual M0 APIs. Never modify an applied migration or make direct C2 INSERT/UPDATE statements outside produced_video.rs. No C3 runner/startup implementation; the runtime integration leaf owns that later.

Preserve immutable numeric clip/source Twitch identity and deletion history, loaded-work recovery guard, explicit bounded public projections excluding private checkpoints, batched versions, full bounded plan and timed transcription checkpoints, nullable/unknown usage semantics and existing owner checks. No second budget. Manual jobs stay independent of Auto-Pilot off/paused state. Do not conflate manual IDs and C3 production-job IDs.

## Concrete contract work

1. Use the real committed M0 migration/schema and production modules in disposable PG proof, not only simplified bigint stubs. Verify actual clip ID SQL types/casts, FK/deletion behavior, metadata ownership and version allocation assumptions. Apply the relevant real migration prerequisites in an owned disposable DB using the existing test support. Preserve production data and runtime roles.
2. M0 has produced_video::insert but no initial metadata argument. The orchestrator explicitly approved an additive insert_with_metadata helper after M0 reached main, which it now has. Keep existing insert signature and behavior by routing through the same implementation with empty object metadata. The new helper validates object metadata and atomically stores manual_ai_job_id at allocation, preserving all existing clip/job/parent/version/ownership guards. Test the actual atomic association and unchanged existing behavior. No C2 SQL outside its module.
3. The shared whole-job session lock identity is hashtextextended('social_media_video_production', 0). M0 currently has C3 claim_next with a transaction claim lock, not a whole-job neutral helper. Move the existing provisional store-local ProductionLease/acquire helper to a neutral shared module, preserving close-on-drop cancellation release and explicit release. Keep connection access narrowly crate-visible and recovery claimed-work semantics intact. Re-export from the store if needed for compatibility. Static, AI and manual callers must reuse this exact helper; do not wire static production here or invent another identity. Parent informs P1 of the module/API.
4. Preserve actual C1 owner preference reads as needed without adding preferences to YAML. C3 integration must be compatible with its committed types; producing via its runtime entry belongs to the following integration phase.

## Acceptance and handover

Run matching package formatter, release build, strict scoped all-targets Clippy and relevant store/C2/lease tests through cargo-slot release/jobs3/shared sccache/artifacts. Wait on flocks normally, no interruption/bypass/cargo clean/cache copies. Distinguish scoped --no-deps lint from transitive lint. The measured tb-raid main-baseline lint is not a source-review blocker: establish parity and review the owned diff; do not touch tb-raid unless the actual parent gate blocks. Fix only demonstrated own failures. No blanket test addition or broad formatter/refactor.

Prove real-schema owner isolation, atomic C2 metadata, original existing insert guards, frozen Twitch source identity, stale recovery/new versus used lease, global lock exclusion/cancellation and excluded private projections. Reuse existing tests where they already prove these contracts. Commit only verified own files ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Hand over clean full SHA, actual baseline, changed paths/API, real-schema checks and limitations. Parent owns backup/integration/gate/deploy; no push/main/deploy, production DB/consent/data, root/service changes, paid calls or publication.

No extra agents/workflows/T3/SendMessage/session coordination. Process inspection pid/comm only; never arguments/environments, secret files or excluded core reviewer log. English work/report; German product text, no added comments or em dashes. Main orchestrator `eddfce29-0b2a-4f47-931f-8b5387672f94`; direct coordinator P2 session `78e43a4b-d7c4-4910-adb3-26be78165791`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-jobs

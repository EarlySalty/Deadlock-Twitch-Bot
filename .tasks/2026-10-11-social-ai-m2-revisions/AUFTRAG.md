# Track B restart: M2 revisions and versions

## Goal and contract

Implement BRIEF.md sections 10, 17-19, 26 and 32, preserving C1-C5 in /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/AUFTRAG.md. Extend the existing editor: natural-language revisions, cheap reuse of successful analysis, server-confirmed expensive changes, preserved usable versions, explicit platform approval and C2 enqueue_for_publishing transfer without re-rendering. Keep manual operation independent of Auto-Pilot on/paused, the existing ai_editor::produce entry, default-off multi_output_enabled, stable layouts, original audio/language, durable checkpoints, seven-day retention and the central persisted $2 cap. No second production queue or budget.

## Sole ownership and recovery

Fresh registered git worktree: /home/nathanael/.worktrees/tb-social-ai-m2-revisions-r1
Branch: feat/social-ai-m2-revisions-r1-20261011
Clean creation base: 7ae7f2c9e12c1385c53a0b9faf016657e2b1769a, committed M1 integrated HEAD, not A's pending merge.

The initial workflow and all three direct continuations were stopped after actual conflicting edits. Never resume their agent IDs a4ec7585694cc1270, a6068082670216e1c or acfa518dc41004a5c. Direct SendMessage while the workflow was active returned a resume, so parent will not use that path for this restart. All old drafts remain preserved in their old checkouts and in the hashed archive /home/nathanael/.worktrees/tb-social-ai/.tasks/2026-10-11-social-ai-autopilot/evidence/parallel-stop-20261011/. Recover only track-b/tracked.patch and its two known new source files into YOUR fresh worktree after checking hashes and patch applicability. The remote backup stores tracked.patch.gz with verified compressed and original-byte hashes in manifest.json; the unchanged local tracked.patch remains available. Reconcile the inconsistent migration/revision/runtime drafts; they are untested, not accepted code. Do not modify any old checkout or overwrite uninspected files.

Exclusive write list:
- rust/crates/tb-social-media/src/ai_editor/ except skills.rs and learning-only paths reserved to C.
- rust/crates/tb-dashboard-api/src/handlers/social_media_ai.rs.
- bot/dashboard_v2/src/components/socialmedia/ai/ and minimal append-only B functions in bot/dashboard_v2/src/api/socialMedia.ts.
- rust/crates/tb-llm/src/social_editor/schema.rs only if genuinely necessary; no mod.rs, usage.rs or read_side.rs writes. The prior executor confirmed M2 needs no mod.rs change.
- ops/social-ai/editor.yaml only for required technical editor bounds, never provider/model/rate/cap changes.
- New B regression tests and migrations 202610112001*, with exact IDs checked against freshly fetched main. The preserved draft is 20261011200101.
- rust/crates/tb-social-media/tests/ai_editor_store.rs only for additive B migration loading and variant_index in the two existing isolated schema fixtures. Preserve the existing test assertions, exclusions and real disposable-Postgres behavior. New B regressions stay in ai_editor/pipeline_tests.rs; do not weaken existing contracts or expand production ownership.

No P1 C2/scheduler/queue/approval/upload module, C1 setting module, main.rs, lib.rs, P3 shell/pages/CSS, skill files, pricing YAML or admin_dashboard writes. Use existing produced_video ownership/version APIs, approve and enqueue_for_publishing_for_user. P1 owns the caller/scheduler. Preserve the existing C3 signatures and return the actual contract rather than inventing a new caller.

## Sequencing and proof

The latest genuine user decision keeps D running in admin_dashboard only and sequences C AFTER your clean completed handover, because learning consumers overlap your pipeline/service/handler. C is not running while you write. Return exact normalized feedback/accept/reject event types, root/child/C2 version identities and frozen skills [{id,version,sha256,text}]. Reused parent checkpoints must not invent new paid attempts or costs. Keep M2 independent of unlanded M3; do not call nonexistent learning functions. On clean completed handover, stop writing these files so the workflow can start C safely. If blocked or source is not clean/verified, C stays queued.

Use code-suche/Graphify first, then targeted reads. Follow global CLAUDE.md, COMMON.md, ABLAUF.md and RUST-BUILDS.md. The genuine newer build limit is `cargo-slot --release --jobs 2`, shared sccache/central per-workspace intermediates and never two release builds at once. A's checks have priority; no lock bypass, cache copies/restarts or process stops. Run matching formatter/build/owned lint/relevant tests once per changed source, with explicit inherited failures and exclusions. UI additionally needs functional and visual Moli checks after reading agent-browser.md, never Brave or persistent/login sessions. Fixtures do not prove live playback or authentication.

Rust production only, no code comments/production unwrap/needless unsafe; Postgres and new migrations only. No secrets, token/session files, production DB/consent writes, paid/external application calls, installation, service changes, publication or deployment. Process inspection pid/comm only; never environments/arguments or the excluded reviewer log. English internal text and natural German product text, no em dashes. Commit verified owned source only, keep .tasks out of main, normal prompt backup where your permissions allow. Final trailer: Co-Authored-By: Claude Code <noreply@anthropic.com>. If your higher-priority executor requires other metadata or forbids push, return a clean unpublished SHA and do not violate it.

After actual M1 landing, adopt freshly fetched main without force updates, check only affected integration and gate independently. Parent owns final gate/fresh BLOCK fixers, main, deploy and live proof. Do not start a separate source-review layer or merge main yourself. No extra agents/workflows/T3 threads. Send early contracts/blockers to main only; parent replies by updating this brief, never by directly resuming an active workflow agent. Reread it at interface/checkpoint boundaries, not in a polling loop.

Parent: 78e43a4b-d7c4-4910-adb3-26be78165791. Main orchestrator: eddfce29-0b2a-4f47-931f-8b5387672f94. Return actual full SHA/tree, clean state, backup or restriction, checks/counts/exclusions, API/event contracts, remaining dependencies and honest gate/live status. Implement, do not stop at a plan.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 30 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-m2-revisions-r1

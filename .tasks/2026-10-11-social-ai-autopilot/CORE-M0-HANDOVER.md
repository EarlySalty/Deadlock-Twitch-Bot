# Core media: M0 rebase and final source review

Goal: complete the preserved media/plan handover for M1, not rebuild the editor. Read CORE-RESOURCE-PROOF-2.md here and COMMON.md/AUFTRAG.md in `/home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/`.

Owned worktree `/home/nathanael/.worktrees/tb-social-ai-core`, branch `feat/social-ai-core-20261011`, clean HEAD `51f0d4cc523c9f4830f7041825eaf1462132099f`. Sole writer, all previous agents completed. Rebase onto verified live M0 `33656f2c2d2154a8ce888ed1a39180935209ea6e` first, adopting P1's Social lint fixes. Resolve only actual conflicts. Own existing ai_editor/media.rs, model.rs, validation.rs, their tests, video_processor.rs and minimal module wiring. No C2/lease/store/provider/config/installer edits or runtime runner.

Preserve RenderResourceLimits, during-write RLIMIT_FSIZE for frames/preparation/render, configured input/logo decoder/filter/encoder threads, original audio, subtitle alignment, atomic output, previous-file preservation, owned child cancellation/temporary cleanup and unchanged unrestricted static renderer. Retain the reviewed deterministic plan contract. Prior current-commit source review stopped on inherited lint and did not reach the diff; source ALLOW still required.

Run matching formatter, scoped strict all-targets Clippy, release build and relevant AI/subtitle/renderer tests using cargo-slot release/jobs3/shared sccache/artifacts. Wait on flocks normally. No bypass, competing release builds, foreign interruption, cargo clean or copies. Repair only actual own failures, with no lint suppression, blanket test addition or foreign source changes. The main-baseline tb-raid transitive lint is explicitly NOT a source-review blocker. Establish parity against real main, run scoped --no-deps lint and review the actual owned diff even if the inherited transitive command is red. Only the actual parent merge gate can authorize a minimal tb-raid fix.

Acceptance: final diff independently reviewed, actual main baseline, matching scoped checks, bounded-write/thread/cancellation behavior and decoded synthetic original audio/video. Reuse existing proof rather than needless repeated suites. Commit only verified own changes ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`. Report clean SHA, conflict resolutions, exact checks and any real limitations. Parent owns backup/integration/gate/deploy.

No agents/workflows/T3/SendMessage/session coordination, push/main/gate/deploy, secrets, production data/DB, paid calls, root/service changes or publication. Process inspection pid/comm only; no arguments/environments or excluded reviewer log access. English work/report, German product text, no added code comments or em dashes. Main orchestrator `eddfce29-0b2a-4f47-931f-8b5387672f94`; direct coordinator P2 session `78e43a4b-d7c4-4910-adb3-26be78165791`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-social-ai-core

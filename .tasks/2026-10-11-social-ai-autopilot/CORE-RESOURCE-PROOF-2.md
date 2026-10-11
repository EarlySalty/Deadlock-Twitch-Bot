# Complete the preserved core resource proof

Worktree: `/home/nathanael/.worktrees/tb-social-ai-core`.
Branch: `feat/social-ai-core-20261011`.
Parent verified clean HEAD: `51f0d4cc523c9f4830f7041825eaf1462132099f`.
Owned final resource changes: `ai_editor/media.rs` and `video_processor.rs` only.

All previous core workers and reviewers are completed and must not be resumed. Earlier source ALLOW covered a prior patch and selected FFmpeg probes, not the final commit or merge gate. The final release build/formatter and 34 focused tests passed, but strict lint stopped independent reviewers before source review. Keep the existing repair: RLIMIT_FSIZE during frame/preparation/final writes, configured decoder/filter/encoder threads, existing atomic output, owned cancellation cleanup and unchanged unrestricted static renderer. The final internal renderer takes `RenderResourceLimits { encoder_threads, max_output_bytes }`; do not restore the earlier separate-argument signature. No rebuilding or broad refactoring.

## Measured shared validation prerequisites

Parent ran matching strict package commands on the final core branch and unchanged integration base `3862b764a`, both release/jobs3/all-targets/no-deps with `-D warnings`. Both fail with the same four findings in analytics.rs:321, credentials.rs:214, upload_worker.rs:853 and vocab.rs:164. A matching strict transitive command on the unchanged base also fails at tb-raid/signup_denylist.rs:71 (`result_unit_err`). These are measured inherited blockers, not new resource defects or green checks. The pending P1 M0 commit c9f3d111 already repairs all four Social files, so do not duplicate or cherry-pick that unaccepted foreign work. Adopt actual M0 on origin/main first. tb-raid remains outside this package's ownership. Obtain a final independent source review and matching scoped checks after the accepted contract rebase.

## Completion contract

Read the actual patch and concrete tests. Run matching package formatting, relevant AI/subtitle/renderer tests, release build and scoped all-targets Clippy with the shared cargo-slot/sccache/central artifact policy. Include the new bounded output/thread/cancellation regression tests and synthetic decoded audio/video probe. Repair only demonstrated own compile/test/lint failures. Do not suppress lints or modify the foreign tb-raid crate. A package-scoped no-dependencies Clippy run may distinguish package findings, but report precisely which command succeeded and which strict transitive command failed. Merely seeing the foreign file unchanged is not an identical-command baseline comparison.

Inspect the new limited-render argument count when lint reaches it; bundle resource limits minimally if actually needed. Preserve the accepted internal media API semantics and document any additive signature adjustment in the handover.

Process inspection remains pid/comm only. No OS command lines/arguments or environments. A test-only shim asserting known synthetic thread flags is not production process inspection, but keep assertion evidence minimal and avoid dumping entire argument vectors. No new core-dump finding has been established: the parent measured production bot LimitCORE=0 and kernel.core_pattern=core. Do not widen the repair to an invented dump vulnerability.

Wait normally for shared slots. No competing release builds, bypass, foreign process interruption, cargo clean or build/model/cache copies. Preserve dirty work if a real blocker prevents completion. Only commit when matching relevant tests/build/formatter pass; report remaining lint failures honestly. English commit ending `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

No agents/workflows/T3/session coordination, production data, secrets, paid calls, push/main/gate/deploy or publication. No added code comments or em dashes. English report, German only for product errors. Parent owns integration and actual merge gate. Handover full SHA, paths, exact check outcomes, resource/cancellation proof and limitations.

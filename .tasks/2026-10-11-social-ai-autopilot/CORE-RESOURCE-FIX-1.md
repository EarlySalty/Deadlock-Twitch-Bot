# Core resource-limit repair, round 1

## Preserved state and verified findings

Worktree: `/home/nathanael/.worktrees/tb-social-ai-core`.
Branch: `feat/social-ai-core-20261011`.
Preserved clean HEAD: `6b5c62fd71673282e89f8bef290be1ef9c71aa9d`.
The completed proof/review workflow is not to be resumed.

Parent source verification on 2026-10-11 confirms both independent review findings:

1. `ai_editor/media.rs:407-408` checks prepared output size only after FFmpeg exits; `:459` does the same for the branded output. `extract_frames` has the equivalent frame-file pattern at `:303-304`. A configured file-size ceiling currently does not constrain writes during those processes.
2. `ai_editor/media.rs:445-454` invokes the existing default renderer without passing `ResourceLimits.encoder_threads`. `video_processor.rs:680-705` supplies no thread setting. The configured encoder count therefore affects interval preparation, not the final encode.

Goal: make the configured limits real during owned AI media processes while preserving the existing static workflow, render composition, cancellation ownership, original audio content and atomic output replacement.

## Ownership and minimal repair boundary

Sole fresh worker may modify core-owned AI files and tests, plus the minimum additive resource-control changes and concrete regression checks in `rust/crates/tb-social-media/src/video_processor.rs`. Keep the existing unrestricted `render_branded` contract and all its cancellation/cleanup behavior. Do not rewrite the renderer or touch P1/P3/provider/jobs/source/deployment files. Preserve the formatter-only `llm_dispatch.rs` change already in HEAD.

Pass the validated configured encoder count to the final codec and avoid unconstrained filter/decode pools where the AI helper explicitly promises a resource count. Enforce file-size ceilings while the child writes, not only afterwards. Keep the postcondition probe and byte checks as well. Apply the same minimal fix to preparation and extracted-frame twins. Oversized/aborted output must never replace an existing usable file or survive owned cleanup.

A safe Linux kernel-limit launcher is available: `/usr/bin/prlimit`, util-linux 2.39.3, verified locally. It is one possible reuse point, not a claim of completed enforcement. Do not claim FFmpeg `-fs` alone is an exact hard ceiling; its documented behavior can overshoot. No needless unsafe, shell interpolation or new dependency. If a new executable requirement is chosen, name it in the handover so integration verifies the actual release host and configuration.

## Proof target

Run concrete bounded synthetic media/process tests for oversized writes, encoder argument propagation, normal decoded output/audio, timeout/drop cleanup and preservation of unrelated/existing files. These are independently checkable resource contracts, not live AI proof. No ignored focused checks silently omitted.

Use the matching package formatter, release build, targeted Clippy and existing focused tests through the shared build policy. Strict Clippy previously failed in untouched `tb-raid/src/signup_denylist.rs:71` (`clippy::result_unit_err`), identical object at base/HEAD. Do not fix foreign baseline files or claim an unrun/blocked check passed. Do not run competing release builds or create local targets/cache copies.

No paid call, production installation, secrets, service changes, push/merge/deploy or publication. Commit only own verified source changes with the required attribution. English implementation report and commit; German only for product-visible errors. No new code comments.

Final report must give SHA, exact changed paths, concrete enforcement mechanism and its limits, real checks/results, any baseline blocker and deferred integration requirements. Parent P2 owns acceptance, integration, gate and deployment.

# M1 integrated gate round 1, 2026-10-12

## Actual result

Task b03jceck1 completed with exit 1 on clean sending HEAD 2556911f272ba6f1bd6faeab8dc1cfd9aca6202a, tested tree e50a28b7108581af1d974797139b3508d9e54391, base b0be1e4c46776b10d08255623b02e070e1977fbf. Actual reviewer: gpt-6.1-sol. Command used the canonical configured chain with literal paths and `--head HEAD`; no override, fallback reroll or per-leaf review.

Verdict: `BLOCK: rollback compatibility, highlight preservation, and STT verification need fixes.`

## Blocking findings, exact gate keys

1. `ops/systemd/install-twitch-release.sh:208`: New release requirements are unconditional. Twins include the deploy wrapper's required_artifacts; installer lines 93, 140, 184, 208 and 222; editor.yaml, pricing-v1.yaml and all eight skill files at lines 236-245. The requested base lacks ops/social-ai, so archiving it fails; the updated installer cannot redeploy that revision despite retained legacy-deployment tests.
2. `rust/crates/tb-social-media/src/ai_editor/pipeline.rs:0 (plan)`: Analysis highlights are not checked against kept intervals. Twins include fresh plan and resumed planning, consumed by render/validate through Planning::validate and validate_plan, which check only the planner's observed_highlights. Empty planner highlights can discard an analyzed highlight entirely while the dashboard still displays it.
3. `rust/crates/tb-social-media/tests/ai_editor_sources.rs:0 (assert_verified_server)`: Integration tests execute a fixed external-worktree binary and require revision 30d39c. Twins are assert_verified_server, job_config, occupied_port_never_creates_a_job_server and both transient_real_stt_silence_and_cancellation_cleanup starts. They do not execute HEAD's STT binary or detect its regressions and depend on another checkout.

## Non-blocking findings to settle against real evidence

- `rust/crates/tb-social-media/src/ai_editor/source.rs:0 (load_source)`: Layout/template owner columns are manually added in fixtures; production migration declarations were omitted from reviewer context. Inspect the complete actual chain, not fixture assumptions.
- `rust/bin/tb-stt-server/src/main.rs:448`: VAD timestamps are divided by 100; inspect the pinned dependency's actual unit contract before claiming correctness or changing code.
- `bot/dashboard_v2/src/components/socialmedia/ai/AiEditorPanel.tsx:91`: An intentionally selected older playable version can display the newest-unplayable warning; the reported condition compares selection IDs rather than actual latest usability.
- `bot/dashboard_v2/src/components/socialmedia/ai/AiEditorPanel.css:1`: The gate had no registered browser visual project/rendered evidence. Existing parent-inspected Moli evidence is in evidence/ui-manager-mount-20261011. Do not register the sending worktree in the Brave-backed catalog. Synthetic layout is checked separately; real playback remains unproved and needs the authorized login/OK boundary.

## Required response

Fresh sole fixer in /home/nathanael/.worktrees/tb-social-ai-m1-gate-fix-r1, branch feat/social-ai-m1-gate-fix-r1-20261012, creation HEAD 2556911f272ba6f1bd6faeab8dc1cfd9aca6202a. Verify each substantive finding and all twins, fix actual causes without weakening safety or assertions, run only matching affected checks and return a clean verified source checkpoint. Parent owns source-preserving metadata corrections/normal backup, integration into the actual sending branch and its next configured gate. No merge/deploy/main/paid/live acceptance until ALLOW and independent acceptance.

M1 source privacy preflight verified the current manual DTO/version query omits raw C2 metadata and paths. P1 must retain that boundary when exposing its future C3 runner. B/C/D do not write these M1 fixes.

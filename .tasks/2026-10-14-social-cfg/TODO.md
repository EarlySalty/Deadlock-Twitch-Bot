# CFG continuation checkpoint

Date: 2026-10-14. Existing worktree `/home/nathanael/.worktrees/tb-social-cfg`, branch `feat/social-cfg-bot-toml-20261014`. Do not recreate it. Start with status and diff.

## State

- Head `bf796e4d0f063b348de5f82e482ebff638d879ae` is published. It includes the original main-TOML loader, pricing correction `09e14b30b`, worker startup anchor and current Social AI admin main changes.
- Fresh `origin/main` is `d915c030a`; another fetch and ff-only ancestry confirmation found no newer main.
- Formal gate `blhtkh5u6` completed exit 0: ALLOW, no merge-blocking defect. REVIEW.md records the nonblocking phase 2 candidate-validation requirement.
- Compiler including tb-bot passed after the pricing correction. Formatter/diff checks passed. Dependency Clippy has a measured unchanged tb-raid result_unit_err failure; selected-crate --no-deps lint remains pending.
- No CFG live edit, main publication or deploy. Phase 2 implementation waits for phase 1 live proof.

## Running checks

- `bmeg3v289`: `cargo-slot test -p tb-config --test runtime --test shared_options --test stt_file --jobs 3 -- --include-ignored && cargo-slot test -p tb-llm -p tb-social-media --no-fail-fast --jobs 3 -- --include-ignored`.
- `b9eqxbceq`: `cargo-slot clippy -p tb-config -p tb-llm -p tb-social-media -p tb-bot --all-targets --no-deps --jobs 3 -- -D warnings`.

Both have 7,200,000 ms background limits and were still queued after the latest main merge. Do not duplicate them. Check their actual compiler start against source state. Logs: `/tmp/claude-1000/-home-nathanael-repos-Deadlock-Twitch-Bot/05bf6922-2935-465b-ac34-8ef16f1c7fc5/tasks/`. Search markers before reading large logs. Foreign builds have active CPU usage; do not stop them.

## Live baseline and migration

- Current bot/dashboard release: `d21e72c6a82949ee746758848c42f4a6dc4b80be`, PIDs 2354089/2354395. Coaching-watch remains on `00e750557de1dd93571e66285fbb01178605b14c`, PID 3422664. Wrapper read proof returns exit 1 for that mismatch, not a CFG failure. All three NRestarts=0 and nondeleted. Reverify before own activation.
- Production migration metadata: 196 applied, checksums/success match. Pending current-main migrations: 20261014150000 and 20261014150100. Recheck deploy SHA.
- Migration preview passed using `rust/target/verify/tb-config-check`; no --apply. Later apply uses the verified-main release CLI and the old release containing the exact previous editor/pricing files. Helper: migrate_social_ai.py. It captures inspection internally, never prints config, checks exact values/Brain absence and backup revision.

## Next actions

1. Evaluate pending tests/lint and record results. Existing global_file failure has measured baseline; do not silently skip or change it. Update evidence and publish the task records.
2. Integrate into main through owned clean worktree `/home/nathanael/.worktrees/tb-social-cfg-baseline-20261014`, after confirming baseline jobs are finished. Never edit dirty canonical checkout or use its stale local main. Isolated HEAD:main publication requires a completely clean sender. One literal Git command per invocation.
3. Build verified origin/main in a standalone release clone via `CARGO_SLOT_DEPLOY=1 cargo-slot build --release ... --jobs 3`, including normal eight binaries and STT server. Latest main contains frontend changes; rebuild affected frontends rather than copying an outdated admin bundle. Installer requires three dist folders.
4. Install merged installer root-owned after inspecting its current target. Apply exact Social AI through hash-checked CLI with verified backup before invoking deploy-twitch-release. Recheck migrations, lock and current process baseline.
5. Verify all affected PIDs/release executables, journal -p err, binary anchor and SOCIAL_AI_WORKER_STARTED. Public admin routes should remain 401 JSON unauthenticated. No paid or publishing requests.
6. Only then implement phase 2 from PLAN.md and the binding brief. Catalog/reference research is complete but no phase 2 code was written. Existing operating endpoint/page and Discord typed catalog remain the integration points. Read UI/Moli instructions before browser work.
7. Finish phase 2 checks, Moli 390/1440 proof, formal review, main, release and own cleanup. Final report at most eight lines, then t3-thread.py settle --selbst after loading the T3 handover skill.

## Boundaries

No paid AI/STT, publishing, autopilot enablement, real streamer-setting changes, Brain activation, secrets in output, new T3 threads or orchestrators. Native agents only for independent checks/gate fixes. Do not print the bot TOML. Keep Social AI skills Markdown unchanged. No foreign worktree/build/browser/service/stream intervention.

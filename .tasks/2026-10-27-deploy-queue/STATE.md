# State

- Branch `feat/deploy-queue`, worktree `/home/nathanael/.worktrees/twitch-deploy-queue`.
- Done: crate, migrations, build/install/wrapper changes, config and unit; clippy `-D warnings` clean, unit tests green, `ops/tests/test-twitch-release-build.sh` passes, pruefen unittest failures identical to the main baseline (35 stale failures already on main).
- `/compact` via T3 RPC proven on thread ddace3b6-54e8-40fb-ad0f-229cde7e0cb4 (compaction item, 50325 -> 11838 tokens).
- Dry-run `deploy-queue plan` against the live release 63a3e096: all binaries and frontends carried, no deploy needed.
- Next: gate, merge, install wrappers and daemon, first real deploy through the queue.
- Gate round 1 BLOCK (8 findings: spec 5, untested 2, false 1). Fixed: pruefen on every no-deploy path, installer derives its own carry closure (`ops/tests/test-install-carry-closure.sh`), frozen-build resume, atomic run completion with recovery of running tickets, capped notify backoff, advisory daemon lock plus claim count, unapplied unit/conf files reported as not applied. STT finding answered as false: tb-social-media launches tb-stt-server from the current release (stt.rs, media_runtime.rs), so bot and dashboard restarts cover it.
- Gate round 2 BLOCK (#2 still: installer read manifests from the working tree; #6 still: standalone STT). Fixed: closure now read from git objects at the pinned SHA (test covers a tampered working tree); a rebuilt tb-stt-server reports the standalone install (masked deadlock-stt-server.service) as not applied.

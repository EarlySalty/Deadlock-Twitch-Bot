# State

- Branch `feat/deploy-queue`, worktree `/home/nathanael/.worktrees/twitch-deploy-queue`.
- Done: crate, migrations, build/install/wrapper changes, config and unit; clippy `-D warnings` clean, unit tests green, `ops/tests/test-twitch-release-build.sh` passes, pruefen unittest failures identical to the main baseline (35 stale failures already on main).
- `/compact` via T3 RPC proven on thread ddace3b6-54e8-40fb-ad0f-229cde7e0cb4 (compaction item, 50325 -> 11838 tokens).
- Dry-run `deploy-queue plan` against the live release 63a3e096: all binaries and frontends carried, no deploy needed.
- Next: gate, merge, install wrappers and daemon, first real deploy through the queue.

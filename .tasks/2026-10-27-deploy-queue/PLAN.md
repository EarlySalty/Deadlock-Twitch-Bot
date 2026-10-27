# Deploy queue for the Twitch bot

Goal: agents submit a merged main SHA with their T3 thread and task folder; a Rust user daemon batches tickets, compacts the thread, deploys only what changed, restarts only affected services and reports back to every thread.

## Pieces

- `rust/bin/tb-deploy-queue` (binary `deploy-queue`): `submit`, `status [ticket]`, `daemon`, `plan --repo <checkout>`.
- Postgres DB `deploy_queue` (peer auth, own sqlx migrations): tables `tickets`, `runs`.
- `ops/deploy-queue/`: `config.toml`, `deploy-queue.service` (user unit), `install.sh`.
- `ops/twitch-release-build --plan <file>`: builds only listed packages, carries unchanged binaries and frontends from the live parent, prints `artifact built|carried` and `build-step` lines.
- `ops/systemd/install-twitch-release.sh`: accepts a carried artifact only if it carries the parent stamp and none of its inputs (nor workspace-wide rust files for binaries) changed between origin and target; writes `PROVENANCE`.
- `ops/systemd/deploy-twitch-release`: `--restart none`, pruefen accepts binaries identical to the release copy.

## Daemon flow

1. Compaction: when the ticket thread is idle, send `/compact`; confirm via the run whose `userMessageId` is ours and its `compaction` turn item (`Context compacted`). Skip after `idle_wait_minutes`, timeout after `compaction_timeout_minutes`.
2. Batch: deploy when any ticket has `--now`, the newest ticket is `quiet_minutes` old, or the oldest is `max_wait_minutes` old.
3. Run: fetch queue clone, target = origin/main, reject SHAs not on it, plan against the live release, build with the plan, `deploy-twitch-release <sha> --restart ...`, retry `--pruefen` up to `pruefen_wait_seconds`.
4. Notify: every ticket gets a final message (deployed, no deploy needed, failed with step, log and commits), retried with backoff until delivered.

## Selective build rules

- Binary inputs: cargo-metadata path-dependency closure (no dev deps) plus every literal `../` file reference in the crates' `.rs` files (conservative, resolved against file and crate dir).
- Workspace-wide change (`rust/*` outside members, except `rust/scripts/`, `rust/docs/`, `*.md`) rebuilds all binaries; this includes `Cargo.lock`.
- Frontends: rebuild on change in their directory or `bot/shared-theme/`; `npm ci` skipped when `package-lock.json` is unchanged.
- Services restart for rebuilt binaries, rebuilt frontends (dashboard) and changed run scripts or unit files.

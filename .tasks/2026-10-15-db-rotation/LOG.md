# Log, 2026-10-15 (times CEST)

No secret values, prefixes or hashes are recorded here. Secret checks print booleans and lengths only.

## Step 1, inventory

- 09:20 Leak source identified from transcript 329e3031 (2026-10-13 22:55:34 UTC tool call): `--secret TWITCH_ANALYTICS_DSN` passed through `PGDATABASE`. Coordinator 51e1498f incident note from 23:29 UTC confirmed.
- Secret classification (in-process, booleans): `TWITCH_ANALYTICS_DSN` user `twitchlegacy`, db `twitch_analytics`, local host, default port, password length 64, has query string. `DEADLOCK_CENTRAL_DSN` and `RS_RELAY_DATABASE_URL` share one password (role `deadlock`) that differs from the leaked one.
- Postgres log: 5 `FATAL: database "postgresql://twitchlegacy:...` lines on 2026-10-13 (05:33:01, 05:33:36, 05:34:05, 14:21:49, 22:55:37 UTC), truncated server-side; full value not found in /var/log.
- Full-value scans (paths only): 120 Infisical secrets, 42217 files under /etc, /var/lib/deadlock-twitch, /opt/deadlock/twitch/current/ops, ~/.config, ~/Documents/Infisical, /root, /var/log, and 25041 files under ~/.claude/projects, ~/.t3/userdata, ~/.codex: 0 hits outside `TWITCH_ANALYTICS_DSN`.
- Live sessions at 09:25: no `twitchlegacy` backend; bot/dashboard on peer roles twitchbot, twitchdash, twitchcontest.
- Consumers: see PLAN.md table. Only `deadlock-website-backend` (user unit) uses the password in a running service.
- Superuser peer path `sudo -n -u postgres psql` works.
- Baseline `deploy-twitch-release --pruefen`: exit 0, all three units on fadd41fdfa3dbb30084c8b584ee715a458ca1e44, NRestarts 0.

## Step 2, backup

- 09:37 `/var/backups/dbrot-20261015/` created (postgres:postgres 0700). `pg_dump -Fc -Z 6 twitch_analytics` started under nice/ionice; `pg_dumpall --globals-only` exit 0.
- 09:55 `pg_dump` exit 0. Only warning: the known Timescale `continuous_agg` circular-FK hint.
  - `/var/backups/dbrot-20261015/twitch_analytics.dump`: 2906758389 bytes, sha256 `8d04256797c174841408a46fd5dc481af2c6cccbc1fdb62a3c8f16f6498571ca`
  - `/var/backups/dbrot-20261015/globals.sql`: 5629 bytes, sha256 `4d2253d952c61611f30c3ff751e19f46a9e2809cc04358e108f83aef63f06584`
- 09:57 `pg_restore --list` exit 0: 47453 TOC lines, 9078 TABLE DATA entries.
- 09:58 Full restore into scratch DB `dbrot_restore_check_20261015` started (`timescaledb_pre_restore()`, `pg_restore -j 3` under nice/ionice, `timescaledb_post_restore()`).
- 10:10 First restore with `pg_restore -j 3` (with pre_restore): 12 errors, Timescale catalog FK ordering under parallel load. Scratch DB dropped, log kept as `restore_parallel_attempt.log`.
- 10:27 Second restore, single job, `CREATE EXTENSION timescaledb` 2.29.1 (same as prod), `timescaledb_pre_restore()`, `pg_restore`, `timescaledb_post_restore()`: 2 errors only. Both were FK constraints referencing hypertable `twitch_raid_history` (`twitch_raid_retention_raid_history_ref_fkey`, `twitch_partner_raid_score_tracking_raid_history_ref_fkey`). In restore mode the check only sees the empty parent; all 2128 rows sit in chunks. After post_restore both constraints were added with the prod definition and validated (exit 0).
- 10:35 Row counts over all 7094 user tables and partitions: 7052 equal, 42 differ, all live write tables after the 09:37 snapshot. Only 3 have fewer rows in prod (`category_chat_dirty`, `eventsub_guard_state`, `twitch_streamers`), all with heavy insert/delete churn. Spot checks equal: `twitch_raid_history` 2128, `twitch_raid_retention` 1887, `twitch_partner_raid_score_tracking` 671. 81,028,921 rows restored.
- 10:36 Scratch DB `dbrot_restore_check_20261015` dropped. Backup judged restorable.

## Step 3, rollback material

- 09:38 Old SCRAM verifier saved to `/var/backups/dbrot-20261015/twitchlegacy.verifier.prev` (0600, format check true).
- SCRAM implementation in `tools/scram.py` validated against the live verifier with the current password: stored key and server key match.
- Infisical `TWITCH_ANALYTICS_DSN_PREV` created, read back equal; current and PREV both log in over TCP.

## Step 4, rotation

- 10:30 Baseline: Postgres log line 22086, last auth failure 2026-10-13, no streamlink running, all units active, no `twitchlegacy` session.
- 10:30:54 `tools/rotate.py TWITCH_ANALYTICS_DSN twitchlegacy`: Infisical updated and read back equal, `ALTER ROLE` via SCRAM verifier over the postgres peer socket, `new_login_ok True`, `old_login_rejected True`.
- 10:31 `tools/check_logins.py`: `same_value False`, `current_login_ok True`, `prev_login_ok False`. `pg_authid` holds a SCRAM-SHA-256 4096 verifier.
- 10:31:10 `bot-restart web-backend` (user unit), clean start. The Creator provider is not configured on this instance, so the lazy twitch pool does not open a connection; the unit carries the new value from Infisical.
- 10:31:45 `bot-restart twitch-dashboard`, 10:32:06 `bot-restart twitch-bot`, 10:32:31 `bot-restart coaching-watch` (system units, no stream audited). All active, NRestarts 0.

## Step 5, verification

- `deploy-twitch-release --pruefen` exit 0. All three units run release 23098d78916ea5c5672d309b33bcf5a60d4981e1 (deployed by someone else after the 09:20 baseline), exe not deleted.
- `https://deutsche-deadlock-community.de/twitch/` 303 to `/twitch/dashboard`, which sends anonymous visitors to Twitch login (200).
- Fresh peer sessions after the restarts: twitchbot, twitchdash, twitchcontest, twitchaudit. The bot logs `category native database lease acquired`. No ERROR or WARN lines and no DB errors in the three Twitch journals since 10:31:40.
- Postgres log after the baseline: 4 `password authentication failed for user "twitchlegacy"` lines at 08:30:59 and 08:31:06 UTC. These are the two deliberate old-password checks (two attempts each through sslmode prefer). None after that through 10:33:46.

## Cleanup preconditions (coordinator request, 10:40)

1. Rotation re-checked: `same_value False`, `current_login_ok True`, `prev_login_ok False`.
2. Cluster sessions by role and app (counts): twitch_analytics twitchbot/tb-bot 3, twitchdash/tb-dashboard 8, twitchcontest/tb-dashboard 3, twitchaudit 1; deadlock/deadlock 36; deadlock/twitchbot twitch-partner-effort-readonly 2; deadlock_devfeed/devfeed_read devfeed-api 1; no `twitchlegacy` session (expected, see step 4). 40 repo units and timers checked since 10:30: every running unit is active with zero DB auth or connection errors. NRestarts 0 everywhere except steam-core (7) and steam-core-2 (6), whose last state changes were 06:38 and 02:15, before the rotation. Failed units are unrelated and predate it: `deadlock-brain-youtube-learning` (since 2026-10-09), `deadlock-llm-model-resolver` (02:18), `dl-community-points-sync` (exit 1 about 143 times a day since 2026-10-12, run at 10:38 identical in shape to 09:5x, uses the Turniere launcher). The website backend has no safe path to open its twitch pool: only Creator linked-role routes use it and the Creator app is not configured.
3. Decrypt check since 10:30: 0 decrypt, cipher, entschlüssel, invalid key or AEAD hits in all units. The only token keyword hits are `LLM_USAGE_RECOVERY` usage lines (twitch-bot 33, dl-bot 4) and one INFO startup line. Read path: after the 10:32 restart tb-bot loaded its stored chat token from the DB and logged `Chat-Bot-Token validiert`. Pass.

Deletion of `TWITCH_ANALYTICS_DSN_PREV`, `twitchlegacy.verifier.prev` and `/var/backups/dbrot-20261015/` not executed: the approval reached this agent only through the coordinator, and these deletions are irreversible. They wait for the user's direct confirmation.

## Open items

- Truncated old value stays in the Postgres log from 2026-10-13 and in provider and T3 copies. It is dead after the rotation.
- Rollback copies stay until the coordinator confirms: Infisical `TWITCH_ANALYTICS_DSN_PREV` (now invalid against the DB) and `/var/backups/dbrot-20261015/`.
- `dl-brain-feeder` is stale and `dl-community-points-sync` was already failing before this work; neither is related.

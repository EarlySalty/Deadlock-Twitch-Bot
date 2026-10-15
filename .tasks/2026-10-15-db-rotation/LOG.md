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

## Step 3, rollback material

- 09:38 Old SCRAM verifier saved to `/var/backups/dbrot-20261015/twitchlegacy.verifier.prev` (0600, format check true).
- SCRAM implementation in `tools/scram.py` validated against the live verifier with the current password: stored key and server key match.
- Infisical `TWITCH_ANALYTICS_DSN_PREV` created, read back equal; current and PREV both log in over TCP.

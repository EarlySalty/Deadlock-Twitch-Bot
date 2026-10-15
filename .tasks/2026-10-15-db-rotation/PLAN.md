# Postgres credential rotation after the 2026-10-13 leak

Authorized by the user for this Opus agent on 2026-10-15. No secret values, prefixes or hashes in this folder.

## Goal and acceptance

Rotate every production Postgres credential exposed on 2026-10-13 without schema or data change and without losing data access.

Accepted when: old password is rejected over TCP, new password authenticates, Infisical holds the new value, every password consumer works after restart, `deploy-twitch-release --pruefen` passes, the dashboard answers, bot logs show working DB access and Postgres logs show no new auth failures.

## Findings (step 1)

- Leak: 2026-10-13 22:55:37 UTC, Sol session 329e3031 (ttb-multiserver-sol-continue). Command loaded Infisical `TWITCH_ANALYTICS_DSN` and passed it as `PGDATABASE`; libpq treated the URI as a database name and the server error echoed it. The session ran through the GPT proxy, so the echoed text reached the external provider. The same error pattern is in the Postgres server log five times on 2026-10-13 (05:33, 05:33, 05:34, 14:21, 22:55 UTC).
- Server-side identifiers are truncated to 63 bytes, so the echo carried the role name and roughly the first 37 of 64 password characters. Treated as fully compromised anyway.
- Affected credential: Infisical `TWITCH_ANALYTICS_DSN` only. Role `twitchlegacy` (non-superuser, DML compatibility role), database `twitch_analytics`, TCP localhost with scram-sha-256. pg_hba allows `twitchlegacy` only on `twitch_analytics` and rejects every other database.
- Not affected: `deadlock` (`DEADLOCK_CENTRAL_DSN` and `RS_RELAY_DATABASE_URL`, rotated 2026-10-12, different password), `patchnotes_feed_reader`, devfeed roles, Brain roles. The leaked password appears in no other Infisical secret (scan of 120 secrets) and in no file under /etc, /var/lib/deadlock-twitch, /var/log, /root, ~/.config, ~/Documents/Infisical, ~/.claude/projects, ~/.t3/userdata, ~/.codex (full-value scan, paths only).
- The coordinator's guess on 2026-10-14 that it was the `deadlock` password is wrong; the transcript shows `--secret TWITCH_ANALYTICS_DSN`.

### Consumers of the twitchlegacy password

| Consumer | Kind | Uses the password? |
|---|---|---|
| `deadlock-website-backend` (user unit, ddc-website-backend, Creator Linked Role, lazy read-only pool) | `systemctl --user` | Yes, via `dl-infisical-env --profile all` |
| Agent and admin sessions (`~/Documents/Infisical/twitch_db.py`, ad-hoc queries) | manual | Yes, read via loader at call time |
| `dl-brain-feeder` (user oneshot) | timer has no next run, paths stale | Optional, dormant |
| `deadlock-twitch-bot-rust` (twitchbot) | system unit | No. `/etc/deadlock-twitch/bot.conf` overrides with a peer socket DSN |
| `deadlock-twitch-dashboard-rust` (twitchdash, twitchcontest) | system unit | No. `/etc/deadlock-twitch/dashboard.conf` overrides with a peer socket DSN |
| `deadlock-twitch-stream-coaching-watch` (twitchaudit) | system unit | No. tb-stream-audit does not read the variable, but the old value sits unused in its env |
| deploy wrapper, migrate unit, watchdog, llm-usage-recover | system | No. Peer or postgres socket |
| Deadlock-Bots binaries, steam, turniere, rs-relay, patchnotes | user units | No (binary scan for the variable name) |

## Backup (step 2)

- Location `/var/backups/dbrot-20261015/` owned by postgres, mode 0700, files 0600.
- `twitch_analytics.dump` (pg_dump -Fc), `globals.sql` (pg_dumpall --globals-only).
- Verification: `pg_restore --list`, then a full restore into scratch database `dbrot_restore_check_20261015` with `timescaledb_pre_restore()`/`post_restore()`, row-count spot checks, then drop it.
- Path, size and sha256 in LOG.md.

## Rollback material (step 3)

- Infisical `TWITCH_ANALYTICS_DSN_PREV`: full old DSN, written in-process from the current secret through `update_gpt_secret.py --anlegen` stdin.
- `/var/backups/dbrot-20261015/twitchlegacy.verifier.prev` (postgres-only 0600): old SCRAM verifier from `pg_authid`. Restoring it brings back the old password exactly without plaintext.
- Superuser path: `sudo -n -u postgres psql` over the local socket (peer). Verified working 2026-10-15. Does not depend on any password.

## Rotation (step 4)

One role, one process. A single Python process
1. generates a 64-character URL-safe password (`secrets.token_urlsafe(48)`),
2. builds the new DSN by replacing only the password in the current DSN (user, host, port, database, query unchanged),
3. writes it to Infisical `TWITCH_ANALYTICS_DSN` through `update_gpt_secret.py` stdin and reads it back for equality,
4. computes a SCRAM-SHA-256 verifier (4096 iterations, random salt) and pipes `ALTER ROLE twitchlegacy PASSWORD '<verifier>'` to `sudo -n -u postgres psql` stdin.

The plaintext never touches argv, disk or output. `log_statement=none`, and only the verifier is sent anyway.

Then restart, one by one, checking each:
1. `bot-restart web-backend` (`systemctl --user`, real consumer).
2. `bot-restart twitch-dashboard`, `bot-restart twitch-bot`, `bot-restart coaching-watch` (system units through the restart wrapper; their start path loads the new Infisical state). Coaching-watch only when no audited stream is running.

## Verification (step 5)

- TCP login as twitchlegacy with old password fails, new password succeeds (password via `PGPASSWORD` env of a child process, never argv).
- `deploy-twitch-release --pruefen` passes (re-run once if coaching-watch still shows the loader exe).
- `https://deutsche-deadlock-community.de/twitch/` responds 200.
- Bot and dashboard journals without DB errors; `pg_stat_activity` shows twitchbot/twitchdash/twitchcontest sessions.
- Postgres log after the change without `password authentication failed` lines.

## Exact rollback steps (step 6)

1. Restore role password from the saved verifier, as postgres only:
   `sudo -n -u postgres python3 -I -c "import sys;v=open('/var/backups/dbrot-20261015/twitchlegacy.verifier.prev').read().strip();sys.stdout.write(\"ALTER ROLE twitchlegacy PASSWORD '\"+v+\"';\n\")" | sudo -n -u postgres psql -X -v ON_ERROR_STOP=1 -d postgres`
2. Restore Infisical: from `.tasks/2026-10-15-db-rotation/tools/` run `python3 restore_prev.py TWITCH_ANALYTICS_DSN` (loads `TWITCH_ANALYTICS_DSN_PREV` in-process and pipes it into `update_gpt_secret.py --secret TWITCH_ANALYTICS_DSN`).
3. `bot-restart web-backend`, then the Twitch units as above.
4. Verify old password logs in over TCP and `--pruefen` passes.
5. Data loss is impossible from this change; if anything else broke, `/var/backups/dbrot-20261015/twitch_analytics.dump` restores the database (`timescaledb_pre_restore()`, `pg_restore -d`, `timescaledb_post_restore()`).

## Boundaries

No schema or data change, no migrations, no Caddy stop, ai-coach untouched, `sudo -n` only.
